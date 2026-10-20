# Hardware — Aura / ARGB (this machine)

Machine-specific notes for controlling ASUS Aura / ARGB via OpenRGB and colorfan.

## Board & controller

| Item | Detail |
|------|--------|
| Motherboard | ASUSTeK **PRIME Z790-P** (Rev 1.xx) |
| Aura USB controller | USB ID `0b05:19af` (ASUS AURA LED Controller) |
| Typical HID node | `/dev/hidraw0` |
| OpenRGB device | **0** — ASUS PRIME Z790-P (Aura USB Mainboard Device) |

Confirm with `lsusb` / `openrgb --list-devices`. Index `0` is what colorfan uses by default (`OPENRGB_DEVICE`).

## OpenRGB zones

| Zone | Name | Physical meaning | LEDs (OpenRGB) |
|------|------|------------------|----------------|
| 0 | Aura Mainboard | 12V RGB headers / onboard RGB | ~2 |
| 1 | Aura Addressable 1 | 5V ARGB **ADD_GEN2** header 1 | ~60 |
| 2 | Aura Addressable 2 | ADD_GEN2 header 2 | ~60 |
| 3 | Aura Addressable 3 | ADD_GEN2 header 3 | ~60 |

- Zone **0** is **not** the ARGB fan chain.
- Addressable zones map **1:1** to physical ADD_GEN2 headers.
- Daisy-chaining or bridging fans onto **one** connector keeps them on **one** zone — not separate zones.

## Wiring on this PC

Fans are on **one** connector with bridges / daisy-chain, so they share a single addressable zone — almost certainly **Aura Addressable 1** (zone `1`).

Changing zones `2` / `3` only affects whatever is plugged into those headers (often nothing).

## Control path (colorfan)

Preferred:

1. OpenRGB **SDK server** (`127.0.0.1:6742`) — colorfan connects or starts `openrgb --server --noautoconnect`.
2. Applies in **Direct** mode via per-zone updates.

Fallback: OpenRGB **CLI** with `--noautoconnect`, same Direct mode. Multi-zone re-apply runs in **one** process so sibling zones are not blanked.

Disable SDK: `COLORFAN_SDK=0` or `prefer_sdk = false` in `~/.config/colorfan/colorfan.toml` (or `$COLORFAN_CONFIG`).

## Soft control alternatives

| Tool | Notes |
|------|--------|
| **colorfan** | TUI over OpenRGB (SDK or CLI) |
| **OpenRGB** GUI / CLI | Same device/zones; useful for listing and profiles |
| **BIOS Aura** | Firmware-level RGB if OS tools are unavailable |

colorfan currently paints a **solid color per zone**. Per-LED on a chain is possible in OpenRGB, but not exposed in colorfan yet.

## Permissions & noise

- Local **udev** rules may grant `hidraw` access so apply works **without sudo**.
- OpenRGB **I2C / SMBus** warnings are irrelevant for this **USB Aura** path; safe to ignore for fan/ARGB control.

## Quick checks

```bash
lsusb | grep -i '0b05:19af\|Aura'
openrgb --noautoconnect --list-devices
ls -l /dev/hidraw0   # or whichever node owns the Aura controller
```
