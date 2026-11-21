"""Kenwood 2-pin Sound Card Pairing & Discord-style Mic / Headphone Audio Manager."""

from __future__ import annotations

import dataclasses
import threading
from typing import Any, Callable, Sequence

from .synth import SAMPLE_RATE, TonePlayer, silence_alsa_errors


@dataclasses.dataclass(frozen=True)
class AudioDeviceInfo:
    index: int
    name: str
    max_inputs: int
    max_outputs: int
    default_sample_rate: int = SAMPLE_RATE


def list_audio_devices() -> list[AudioDeviceInfo]:
    """Enumerate available host sound cards (USB sound cards, Digirig, AIOC, PipeWire/ALSA)."""
    devices: list[AudioDeviceInfo] = []
    try:
        import pyaudio

        silence_alsa_errors()
        pa = pyaudio.PyAudio()
        try:
            for i in range(pa.get_device_count()):
                info = pa.get_device_info_by_index(i)
                devices.append(
                    AudioDeviceInfo(
                        index=int(i),
                        name=str(info.get("name", f"Device {i}")),
                        max_inputs=int(info.get("maxInputChannels", 0)),
                        max_outputs=int(info.get("maxOutputChannels", 0)),
                        default_sample_rate=int(info.get("defaultSampleRate", SAMPLE_RATE)),
                    )
                )
        finally:
            pa.terminate()
    except Exception:
        pass
    return devices


class AudioManager:
    """
    Unified PC audio controller for Kern Dock:
      • Headless RigWire tone & beep playback (routed when radio is headless)
      • Kenwood 2-pin USB sound card pairing (RX speaker-in -> PC headphones; PC mic -> TX mic-out)
      • Discord-style Headphone (deafen/monitor) & Microphone (mute/unmute) toggles
    """

    def __init__(self, *, on_change: Callable[[], None] | None = None) -> None:
        self._on_change = on_change
        self._tone_player = TonePlayer()
        self.headphones_enabled: bool = True
        self.mic_enabled: bool = False
        self.volume: float = 0.35
        self.kenwood_rx_device: int | None = None  # Sound card input (from radio speaker jack)
        self.kenwood_tx_device: int | None = None  # Sound card output (to radio mic jack)
        self._stop_rx = threading.Event()
        self._stop_tx = threading.Event()
        self._rx_thread: threading.Thread | None = None
        self._tx_thread: threading.Thread | None = None

    @property
    def kenwood_paired(self) -> bool:
        return self.kenwood_rx_device is not None or self.kenwood_tx_device is not None

    def set_headphones(self, enabled: bool) -> None:
        self.headphones_enabled = bool(enabled)
        self._sync_streams()
        if self._on_change:
            self._on_change()

    def set_mic(self, enabled: bool) -> None:
        self.mic_enabled = bool(enabled)
        self._sync_streams()
        if self._on_change:
            self._on_change()

    def set_volume(self, vol: float) -> None:
        self.volume = max(0.0, min(1.0, float(vol)))
        if self._on_change:
            self._on_change()

    def pair_kenwood(self, rx_input_idx: int | None, tx_output_idx: int | None) -> None:
        self.kenwood_rx_device = rx_input_idx
        self.kenwood_tx_device = tx_output_idx
        self._restart_streams()
        if self._on_change:
            self._on_change()

    def play_headless_notes(self, notes: Sequence[tuple[int, int]]) -> None:
        """Play RigWire-routed system beeps/melodies on PC unless headphones are muted/deafened."""
        if not self.headphones_enabled or not notes:
            return
        self._tone_player.play_notes(notes, volume=self.volume)

    def _restart_streams(self) -> None:
        self._stop_streams()
        self._sync_streams()

    def _sync_streams(self) -> None:
        if self.headphones_enabled and self.kenwood_rx_device is not None:
            if self._rx_thread is None or not self._rx_thread.is_alive():
                self._stop_rx.clear()
                self._rx_thread = threading.Thread(
                    target=self._duplex_pipe,
                    args=(self.kenwood_rx_device, None, self._stop_rx, lambda: self.headphones_enabled),
                    daemon=True,
                    name="kern-kenwood-rx",
                )
                self._rx_thread.start()
        else:
            self._stop_rx.set()

        if self.mic_enabled and self.kenwood_tx_device is not None:
            if self._tx_thread is None or not self._tx_thread.is_alive():
                self._stop_tx.clear()
                self._tx_thread = threading.Thread(
                    target=self._duplex_pipe,
                    args=(None, self.kenwood_tx_device, self._stop_tx, lambda: self.mic_enabled),
                    daemon=True,
                    name="kern-kenwood-tx",
                )
                self._tx_thread.start()
        else:
            self._stop_tx.set()

    def _duplex_pipe(
        self,
        in_idx: int | None,
        out_idx: int | None,
        stop_ev: threading.Event,
        active_fn: Callable[[], bool],
    ) -> None:
        pa: Any = None
        s_in: Any = None
        s_out: Any = None
        try:
            import pyaudio

            silence_alsa_errors()
            pa = pyaudio.PyAudio()
            s_in = pa.open(
                format=pyaudio.paInt16,
                channels=1,
                rate=SAMPLE_RATE,
                input=True,
                input_device_index=in_idx,
                frames_per_buffer=1024,
            )
            s_out = pa.open(
                format=pyaudio.paInt16,
                channels=1,
                rate=SAMPLE_RATE,
                output=True,
                output_device_index=out_idx,
                frames_per_buffer=1024,
            )
            while not stop_ev.is_set() and active_fn():
                data = s_in.read(1024, exception_on_overflow=False)
                s_out.write(data)
        except Exception:
            pass
        finally:
            for s in (s_in, s_out):
                if s is not None:
                    try:
                        s.stop_stream()
                        s.close()
                    except Exception:
                        pass
            if pa is not None:
                try:
                    pa.terminate()
                except Exception:
                    pass

    def _stop_streams(self) -> None:
        self._stop_rx.set()
        self._stop_tx.set()

    def shutdown(self) -> None:
        self._stop_streams()
        self._tone_player.stop()
