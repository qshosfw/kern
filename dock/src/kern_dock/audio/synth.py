"""Warm, click-free PC synthesizer for headless RigWire system beeps and melodies."""

from __future__ import annotations

import ctypes
import math
import queue
import shutil
import struct
import subprocess
import threading
from typing import Sequence

try:
    import numpy as _np
except Exception:
    _np = None  # type: ignore[assignment]

SAMPLE_RATE = 44100

# Keep a module-level reference to the C callback so Python's GC never frees it while libasound holds it.
_ALSA_HANDLER_REF = None
_ALSA_SILENCED = False


def silence_alsa_errors() -> None:
    """Install a no-op libasound error handler so PyAudio/PortAudio doesn't spam stderr on PipeWire Linux."""
    global _ALSA_HANDLER_REF, _ALSA_SILENCED
    if _ALSA_SILENCED:
        return
    _ALSA_SILENCED = True
    try:
        cb_type = ctypes.CFUNCTYPE(None, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p, ctypes.c_int, ctypes.c_char_p)
        _ALSA_HANDLER_REF = cb_type(lambda _f, _l, _fn, _e, _fmt: None)
        for lib_name in ("libasound.so.2", "libasound.so"):
            try:
                asound = ctypes.cdll.LoadLibrary(lib_name)
                asound.snd_lib_error_set_handler(_ALSA_HANDLER_REF)
                return
            except OSError:
                continue
    except Exception:
        pass


def bk4829_quantize_hz(freq_hz: int) -> float:
    """
    Exact mirror of `kernel::service::radio::tone_gen::calc_tone_word` (26 MHz BK4829 DDS):
    quantizes `freq_hz` to the exact register 0x71 DDS frequency word so PC playback matches
    the physical radio speaker pitch bit-for-bit.
    """
    f = max(0, min(6350, int(freq_hz)))
    if f <= 0:
        return 0.0
    frac = (f * 42_525 + 65_536) >> 17
    word = (f * 10 + frac) & 0xFFFF
    return word / 10.32444


def synthesize_notes_pcm(
    notes: Sequence[tuple[int, int]],
    *,
    sample_rate: int = SAMPLE_RATE,
    volume: float = 0.28,
) -> bytes:
    """
    Synthesize `(freq_hz, duration_ms)` tone notes into click-free signed 16-bit mono PCM
    matching the physical BK4829 DDS pure sine tone generator (exact DDS pitch quantization,
    pure fundamental sine with zero octave-shifting harmonics, crisp 1.5 ms anti-click edges,
    and 1 ms inter-note mute gap matching `Bk4829::play_tone_sequence`).
    """
    vol = max(0.0, min(1.0, float(volume)))
    if not notes or vol <= 0.0:
        return b""

    chunks: list[bytes] = []
    gap_samp = max(1, int(sample_rate * 0.001))  # 1 ms REG_TX_MUTE gap between adjacent notes
    gap_bytes = bytes(gap_samp * 2)
    n_notes = len(notes)
    if _np is not None:
        for idx, (freq_hz, dur_ms) in enumerate(notes):
            n_samp = max(1, int(sample_rate * max(1, min(int(dur_ms), 4000)) / 1000.0))
            actual_hz = bk4829_quantize_hz(freq_hz)
            if actual_hz <= 20.0:
                chunks.append(bytes(n_samp * 2))
                continue
            t = _np.arange(n_samp, dtype=_np.float64) / float(sample_rate)
            # Pure BK4829 DDS fundamental sine wave (no octave-shifting harmonics)
            wave = _np.sin(2.0 * math.pi * actual_hz * t) * (vol * 0.88 * 32767.0)
            # Crisp 1.5 ms raised-cosine attack & release (preserves 15-25 ms key pips cleanly)
            fade = min(n_samp // 3, max(4, int(sample_rate * 0.0015)))
            if fade > 1:
                ramp = 0.5 * (1.0 - _np.cos(_np.linspace(0.0, math.pi, fade, dtype=_np.float64)))
                wave[:fade] *= ramp
                wave[-fade:] *= ramp[::-1]
            pcm = _np.clip(wave, -32767.0, 32767.0).astype("<i2")
            chunks.append(pcm.tobytes())
            if idx + 1 < n_notes and notes[idx + 1][0] > 20:
                chunks.append(gap_bytes)
    else:
        for idx, (freq_hz, dur_ms) in enumerate(notes):
            n_samp = max(1, int(sample_rate * max(1, min(int(dur_ms), 4000)) / 1000.0))
            actual_hz = bk4829_quantize_hz(freq_hz)
            if actual_hz <= 20.0:
                chunks.append(bytes(n_samp * 2))
                continue
            fade = min(n_samp // 3, max(4, int(sample_rate * 0.0015)))
            buf = bytearray(n_samp * 2)
            amp = vol * 0.88 * 32767.0
            two_pi_f = 2.0 * math.pi * actual_hz / float(sample_rate)
            for i in range(n_samp):
                env = 1.0
                if fade > 1:
                    if i < fade:
                        env = 0.5 * (1.0 - math.cos(math.pi * i / fade))
                    elif i >= n_samp - fade:
                        env = 0.5 * (1.0 - math.cos(math.pi * (n_samp - 1 - i) / fade))
                s = math.sin(two_pi_f * i)
                val = max(-32767, min(32767, int(s * amp * env)))
                struct.pack_into("<h", buf, i * 2, val)
            chunks.append(bytes(buf))
            if idx + 1 < n_notes and notes[idx + 1][0] > 20:
                chunks.append(gap_bytes)
    return b"".join(chunks)


class TonePlayer:
    """Non-blocking background player for headless RigWire beeps & melodies."""

    def __init__(self) -> None:
        self._q: queue.Queue[bytes | None] = queue.Queue(maxsize=32)
        self._stop = threading.Event()
        self._thread: threading.Thread | None = None

    def start(self) -> None:
        if self._thread is not None and self._thread.is_alive():
            return
        self._stop.clear()
        self._thread = threading.Thread(target=self._loop, daemon=True, name="kern-tone-synth")
        self._thread.start()

    def stop(self) -> None:
        self._stop.set()
        try:
            self._q.put_nowait(None)
        except queue.Full:
            pass

    def play_notes(self, notes: Sequence[tuple[int, int]], *, volume: float = 0.28) -> None:
        pcm = synthesize_notes_pcm(notes, volume=volume)
        if not pcm:
            return
        if self._thread is None or not self._thread.is_alive():
            self.start()
        try:
            self._q.put_nowait(pcm)
        except queue.Full:
            try:
                self._q.get_nowait()
                self._q.put_nowait(pcm)
            except Exception:
                pass

    def _loop(self) -> None:
        pa = None
        stream = None
        try:
            import pyaudio

            silence_alsa_errors()
            pa = pyaudio.PyAudio()
            stream = pa.open(format=pyaudio.paInt16, channels=1, rate=SAMPLE_RATE, output=True)
        except Exception:
            pa = None
            stream = None

        pw_cat = shutil.which("pw-cat") or shutil.which("paplay")
        try:
            while not self._stop.is_set():
                try:
                    pcm = self._q.get(timeout=0.3)
                except queue.Empty:
                    continue
                if pcm is None:
                    break
                if stream is not None:
                    try:
                        stream.write(pcm)
                        continue
                    except Exception:
                        pass
                if pw_cat:
                    try:
                        cmd = (
                            [pw_cat, "--playback", "--raw", "--rate", str(SAMPLE_RATE), "--channels", "1", "--format", "s16", "-"]
                            if pw_cat.endswith("pw-cat")
                            else [pw_cat, "--raw", f"--rate={SAMPLE_RATE}", "--channels=1", "--format=s16le"]
                        )
                        subprocess.run(cmd, input=pcm, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=4.0)
                    except Exception:
                        pass
        finally:
            if stream is not None:
                try:
                    stream.stop_stream()
                    stream.close()
                except Exception:
                    pass
            if pa is not None:
                try:
                    pa.terminate()
                except Exception:
                    pass
