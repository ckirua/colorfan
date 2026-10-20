# colorfan

Terminal UI (TUI) to set **ASUS Aura / ARGB** colors through system **OpenRGB**. Prefer the OpenRGB SDK server for low-latency per-zone applies; fall back to the CLI when needed.

Typical setup: OpenRGB device `0` is the mainboard Aura controller, with zones such as *Aura Mainboard* and *Aura Addressable 1/2/3*. Confirm indices with `openrgb --list-devices`. Machine-specific wiring: [docs/HARDWARE.md](docs/HARDWARE.md).

## Build & run

```bash
cargo run
# or
cargo build --release && ./target/release/colorfan
```

Requires `openrgb` on `PATH` (or set `OPENRGB_PATH`). `Cargo.lock` is tracked (binary app).

## Keybindings

| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit (works even while apply is running) |
| `Tab` / `←` `→` / `h` `l` | Change focus (Zones → Presets → Hex) |
| `↑` `↓` / `j` `k` | Move selection |
| `Enter` | Apply current color (`direct` mode by default) |
| `1`–`8` | Select preset and apply immediately |
| `a` | Target all zones |
| `p` | Load OpenRGB profile `solid-red` |
| `r` | Reload device/zone list from OpenRGB |

Presets: 1 Red · 2 Green · 3 Blue · 4 White · 5 Off · 6 Purple · 7 Cyan · 8 Orange

## Responsiveness & performance

**UI never blocks on OpenRGB.** Applies, profile loads, and device reloads run on a background thread. While work is in flight you get an animated **Applying…** spinner (yellow header / status), and further apply keys are ignored so you cannot double-submit. Quit (`q`) still works.

### Why apply used to feel slow (~7–8 s)

Measured on this machine (`openrgb --noautoconnect …`):

| Call | Typical duration |
|------|------------------|
| `--list-devices` | ~6.7 s |
| 1-zone CLI apply | ~7.8 s |
| 4-zone CLI apply | ~7.9 s |

Almost all of that is **OpenRGB process cold-start + device/I2C probing**, not the multi-zone `-d/-z` blocks (1 zone ≈ 4 zones). There is no CLI flag that skips probing while still talking to hardware locally (`--nodetect` alone cannot apply; `--client --nodetect` races and fails).

### Fast path: OpenRGB SDK server

By default colorfan:

1. Connects to an existing OpenRGB SDK server on `127.0.0.1:6742`, or starts `openrgb --server --noautoconnect` once (same ~7–8 s probe, **once**).
2. Applies colors over the SDK with **`UpdateZoneLEDs`** (true per-zone; siblings are not blanked).

Measured SDK apply (4 zones): **~80 ms** after the server is up — about **100× faster** than a cold CLI launch.

If the server cannot be started, colorfan falls back to the multi-zone CLI (still non-blocking in the TUI). Disable the SDK path with `COLORFAN_SDK=0` or `prefer_sdk = false` in config.

Servers started by colorfan are stopped on exit. If you already run the OpenRGB GUI/server yourself, colorfan reuses it and does not kill it.

## How apply works

### SDK (default when server available)

Each zone is updated with `UpdateZoneLEDs`. Sibling zones keep their colors without re-sending everything (colorfan still tracks last-known colors for the UI and for CLI fallback).

### CLI fallback

OpenRGB’s CLI is not a true per-zone preserve API: a lone

`openrgb --noautoconnect -d 0 -z 1 -m direct -c COLOR`

paints zone 1, then calls device-wide `UpdateLEDs()` on a **fresh** buffer, so other zones go **black**.

colorfan keeps the last-known color for each zone (shown next to the zone list) and, on CLI apply, runs **one** `openrgb` process that re-applies all tracked zones with repeated `-d` / `-z` / `-m` / `-c` blocks:

```bash
openrgb --noautoconnect \
  -d 0 -z 0 -m direct -c 000000 \
  -d 0 -z 1 -m direct -c 0000FF \
  -d 0 -z 2 -m direct -c 00FF00 \
  -d 0 -z 3 -m direct -c FF0000
```

- **Single zone:** updates that zone’s stored color, then (CLI) re-applies every known zone in one process.
- **All zones:** sets every stored zone color to the chosen color, then applies the same way.
- **Persistence:** after a successful apply, colors are written to `~/.config/colorfan/colorfan.toml` so relaunch does not blank siblings.

**Daisy-chain note:** fans daisy-chained on **one** ARGB header still share a single OpenRGB zone — they cannot get independent colors via multi-zone CLI. Multi-zone re-apply is for **multiple headers** (e.g. Addressable 1/2/3) and for not blanking sibling zones when you change one of them.

## Config

Runtime config (zone colors, device index, etc.) lives in the user XDG config dir — not in the project tree:

| Priority | Path |
|----------|------|
| 1 (override) | `$COLORFAN_CONFIG` — any file path |
| 2 (default) | `$XDG_CONFIG_HOME/colorfan/colorfan.toml` (default `~/.config/colorfan/colorfan.toml`) |

Install from the tracked example:

```bash
mkdir -p ~/.config/colorfan
cp colorfan.toml.example ~/.config/colorfan/colorfan.toml
```

If an old `./colorfan.toml` is still in the working directory and the XDG file is missing, colorfan copies it once to the XDG path on load/save, then uses the new location. Safe to delete the repo-local file afterward (it remains gitignored if someone drops one there).

System-wide paths such as `/etc/colorfan.toml` are not used by default (would need root and is wrong for per-user zone colors); they could be added later as an optional admin install.

See `colorfan.toml.example`.

Environment overrides:

| Variable | Meaning |
|----------|---------|
| `OPENRGB_PATH` | Binary path (default `openrgb`) |
| `OPENRGB_DEVICE` | Device index (default `0`) |
| `OPENRGB_USE_SUDO` | `0` / `1` / `auto` |
| `OPENRGB_MODE` | Mode string (default `direct`; `static` also OK) |
| `COLORFAN_SDK` | `1` (default) use/start SDK server; `0` force CLI applies |
| `COLORFAN_CONFIG` | Path to config TOML (overrides XDG default entirely) |
| `XDG_CONFIG_HOME` | Base for the default config dir (default `~/.config`) |

Also used:

```bash
openrgb --noautoconnect -p solid-red   # ~/.config/OpenRGB/solid-red.orp
```

`--noautoconnect` avoids hanging on SDK “Connection attempt failed” when no server is listening yet.

## Permissions

colorfan tries **without sudo** first (`use_sudo = "auto"`). If Aura HID is readable (udev / `hidraw` perms), that is enough.

If apply fails with a permission error:

```bash
OPENRGB_USE_SUDO=1 cargo run
```

`sudo` may prompt for your password, or reuse a cached credential. When forced, colorfan uses non-interactive `sudo -n` so it fails fast if a password would be required.

I2C / SMBus warnings from OpenRGB are often harmless for USB Aura boards and are filtered from the UI.

## Limitations

- **Irreducible cost:** the first OpenRGB process start (CLI or `--server`) still spends several seconds probing hardware. After that, SDK applies are tens of milliseconds.
- Default mode is **direct** (Aura); override with `mode = "static"` / `OPENRGB_MODE` if needed.
- Only one device is focused (configured index); zones typically map to Mainboard + Addressable channels.
- I2C PCI ID warnings from OpenRGB are ignored when they are unrelated to the USB Aura path.
- CLI fallback cannot skip black “unused” zones safely (sibling blanking); SDK path does not have that problem.
