"""Built-in Plugin: Declarative RF Band & Calling Frequency Presets (Command Palette + Tray + UI)."""

from __future__ import annotations

import dataclasses
from typing import Any

from ..api import PluginContext, PluginManifest


@dataclasses.dataclass(frozen=True)
class FrequencyPreset:
    name: str
    group: str
    freq_hz: int
    modulation: str = "FM"
    note: str = ""


DEFAULT_PRESETS: tuple[FrequencyPreset, ...] = (
    FrequencyPreset("2m FM Calling (EU)", "Amateur VHF", 145_500_000, "FM", "Region 1 calling channel"),
    FrequencyPreset("2m FM Calling (US)", "Amateur VHF", 146_520_000, "FM", "North America national simplex"),
    FrequencyPreset("APRS (EU)", "Amateur VHF", 144_800_000, "FM", "1200 baud packet"),
    FrequencyPreset("ISS Voice Downlink", "Satellite", 145_800_000, "FM", "ARISS FM voice"),
    FrequencyPreset("70cm FM Calling (EU)", "Amateur UHF", 433_500_000, "FM", "Region 1 70cm simplex"),
    FrequencyPreset("70cm FM Calling (US)", "Amateur UHF", 446_000_000, "FM", "North America 70cm simplex"),
    FrequencyPreset("PMR446 Ch 1", "PMR446", 446_006_250, "FM", "License-free EU Ch 1"),
    FrequencyPreset("PMR446 Ch 8", "PMR446", 446_093_750, "FM", "License-free EU Ch 8"),
    FrequencyPreset("Airband Guard", "Airband", 121_500_000, "AM", "International aviation distress (RX)"),
    FrequencyPreset("Marine Ch 16", "Marine VHF", 156_800_000, "FM", "International distress & calling (RX)"),
    FrequencyPreset("NOAA WX 1", "Weather", 162_550_000, "FM", "NOAA Weather Radio (RX)"),
)


class BandPresetsPlugin:
    manifest = PluginManifest(
        id="band-presets",
        name="Band Presets",
        version="2.0.0",
        description="Declarative VHF/UHF/PMR/Airband/Marine quick-tune presets in UI, Command Palette and Tray.",
        capabilities=("ui.page", "ui.action", "ui.tray", "radio.read", "radio.control"),
        icon="starred-symbolic",
        builtin=True,
    )

    def __init__(self) -> None:
        self._ctx: PluginContext | None = None

    def activate(self, ctx: PluginContext) -> None:
        self._ctx = ctx
        for preset in DEFAULT_PRESETS:
            slug = preset.name.lower().replace(" ", "-").replace("(", "").replace(")", "")
            ctx.register_action(
                f"tune-{slug}",
                f"Tune {preset.name}",
                lambda p=preset: self._tune(p),
                subtitle=f"{preset.freq_hz / 1e6:.4f} MHz · {preset.modulation} · {preset.group}",
                icon="network-wireless-symbolic",
                mutates=True,
            )
        ctx.register_tray_item(
            "tune-70cm",
            "Quick Tune 433.500 MHz (FM)",
            lambda: self._tune(DEFAULT_PRESETS[4]),
            enabled=lambda: self._is_online(),
        )
        ctx.register_tray_item(
            "tune-2m",
            "Quick Tune 145.500 MHz (FM)",
            lambda: self._tune(DEFAULT_PRESETS[0]),
            enabled=lambda: self._is_online(),
        )
        ctx.register_page(
            "presets",
            "Band Presets",
            "starred-symbolic",
            self._build_page,
            order=10,
            subtitle="One-click VHF, UHF, PMR, Airband & Marine tuning",
        )

    def deactivate(self) -> None:
        self._ctx = None

    def _is_online(self) -> bool:
        if self._ctx is None:
            return False
        lk = self._ctx.link
        return bool(lk.connected and lk.link_mode == "rigwire")

    def _tune(self, preset: FrequencyPreset) -> None:
        if self._ctx is None:
            return
        if not self._is_online():
            self._ctx.toast("Connect a RigWire radio to tune presets")
            return
        try:
            self._ctx.tune(preset.freq_hz, preset.modulation)
            self._ctx.toast(f"Tuned {preset.name} ({preset.freq_hz / 1e6:.4f} MHz {preset.modulation})")
        except Exception as exc:
            self._ctx.toast(str(exc))

    def _build_page(self) -> Any:
        import gi

        gi.require_version("Gtk", "4.0")
        gi.require_version("Adw", "1")
        from gi.repository import Adw, GLib, Gtk

        page = Adw.PreferencesPage()
        grouped: dict[str, list[FrequencyPreset]] = {}
        for p in DEFAULT_PRESETS:
            grouped.setdefault(p.group, []).append(p)

        for group_name, items in grouped.items():
            grp = Adw.PreferencesGroup(title=group_name)
            for preset in items:
                row = Adw.ActionRow(
                    title=GLib.markup_escape_text(preset.name),
                    subtitle=GLib.markup_escape_text(
                        f"{preset.freq_hz / 1e6:.4f} MHz · {preset.modulation}" + (f" · {preset.note}" if preset.note else "")
                    ),
                    activatable=True,
                )
                badge = Gtk.Label(label=preset.modulation, valign=Gtk.Align.CENTER)
                badge.add_css_class("freq-badge")
                row.add_suffix(badge)
                row.add_suffix(Gtk.Image.new_from_icon_name("go-next-symbolic"))
                row.connect("activated", lambda _r, p=preset: self._tune(p))
                grp.add(row)
            page.add(grp)
        return page
