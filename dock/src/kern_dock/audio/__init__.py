"""PC Audio Subsystem: Headless RigWire tone synthesis & Kenwood 2-pin sound card bridge."""

from __future__ import annotations

from .kenwood import AudioDeviceInfo, AudioManager, list_audio_devices
from .synth import synthesize_notes_pcm

__all__ = ["AudioDeviceInfo", "AudioManager", "list_audio_devices", "synthesize_notes_pcm"]
