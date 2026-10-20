# colorfan

Small terminal UI to set **ASUS Aura / ARGB** colors through system **OpenRGB**.

Typical setup: OpenRGB device `0` is the mainboard Aura controller, with zones such as *Aura Mainboard* and *Aura Addressable 1/2/3*. Confirm indices with `openrgb --list-devices`.

## Build & run

```bash
cargo run
# or
cargo build --release && ./target/release/colorfan
```

Requires `openrgb` on `PATH` (or set `OPENRGB_PATH`).

## Keybindings

| Key | Action |
|-----|--------|
| `q` / `Esc` | Quit |
| `Tab` / `←` `→` / `h` `l` | Change focus (Zones → Presets → Hex) |
| `↑` `↓` / `j` `k` | Move selection |
| `Enter` | Apply current color (`-m direct` by default) |
| `1`–`8` | Select preset and apply immediately |
| `a` | Target all zones |
| `p` | Load OpenRGB profile `solid-red` |
| `r` | Reload device/zone list from OpenRGB |

Presets: 1 Red · 2 Green · 3 Blue · 4 White · 5 Off · 6 Purple · 7 Cyan · 8 Orange

## Config

Optional `colorfan.toml` in the current directory, or `~/.config/colorfan/colorfan.toml`, or `COLORFAN_CONFIG=/path/to.toml`.

See `colorfan.toml.example`.

Environment overrides:

| Variable | Meaning |
|----------|---------|
| `OPENRGB_PATH` | Binary path (default `openrgb`) |
| `OPENRGB_DEVICE` | Device index (default `0`) |
| `OPENRGB_USE_SUDO` | `0` / `1` / `auto` |
| `OPENRGB_MODE` | Mode string (default `direct`; `static` also OK) |

Apply pattern used:

```bash
openrgb --noautoconnect -d 0 -m direct -c FF0000
openrgb --noautoconnect -d 0 -z 1 -m direct -c FF0000   # Addressable 1
openrgb --noautoconnect -p solid-red                     # ~/.config/OpenRGB/solid-red.orp
```

`--noautoconnect` avoids hanging on SDK “Connection attempt failed”.

## Permissions

colorfan tries **without sudo** first (`use_sudo = "auto"`). If Aura HID is readable (udev / `hidraw` perms), that is enough.

If apply fails with a permission error:

```bash
OPENRGB_USE_SUDO=1 cargo run
```

`sudo` may prompt for your password, or reuse a cached credential. When forced, colorfan uses non-interactive `sudo -n` so it fails fast if a password would be required.

I2C / SMBus warnings from OpenRGB are often harmless for USB Aura boards and are filtered from the UI.

## Limitations

- Shells out to the OpenRGB CLI (not the SDK). Each apply can take several seconds while OpenRGB probes hardware.
- Default mode is **direct** (Aura); override with `mode = "static"` / `OPENRGB_MODE` if needed.
- Only one device is focused (configured index); zones typically map to Mainboard + Addressable channels.
- I2C PCI ID warnings from OpenRGB are ignored when they are unrelated to the USB Aura path.
- Does not start or manage an OpenRGB GUI/server.
