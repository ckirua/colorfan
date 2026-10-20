# Changelog

All notable changes to this project will be documented in this file.

## [0.1.0] — 2026-10-21

Initial release of **colorfan**, a small TUI for setting ASUS Aura / ARGB colors via OpenRGB.

### Highlights

- **TUI** — Ratatui-based interface for zones, presets, and hex color entry; non-blocking applies with spinner feedback
- **OpenRGB SDK** — Prefer the OpenRGB SDK server for fast per-zone applies (`UpdateZoneLEDs`); fall back to the OpenRGB CLI when needed
- **XDG config** — Runtime config under `~/.config/colorfan` (see `colorfan.toml.example`)
- **Zones** — Select individual zones or all zones; track last-known colors for CLI multi-zone re-apply
- **Docs** — README plus machine-specific wiring notes in `docs/HARDWARE.md`
