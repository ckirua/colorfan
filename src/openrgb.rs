use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub index: u32,
    pub name: String,
    pub zones: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct CommandResult {
    pub command_line: String,
    pub success: bool,
    pub output: String,
    pub used_sudo: bool,
}

pub fn list_devices(cfg: &Config) -> Result<Vec<DeviceInfo>> {
    let result = run_openrgb(cfg, &["--list-devices"], cfg.wants_sudo())?;
    if !result.success {
        // Permission-ish failure without sudo: surface a clear hint.
        if !result.used_sudo && looks_like_permission_error(&result.output) {
            bail!(
                "openrgb failed (permission?). Try OPENRGB_USE_SUDO=1 or fix hidraw udev rules.\n{}",
                truncate(&result.output, 400)
            );
        }
        bail!("openrgb --list-devices failed:\n{}", truncate(&result.output, 400));
    }
    Ok(parse_devices(&result.output))
}

/// Apply a solid color via OpenRGB CLI.
///
/// Working pattern on this machine:
/// `openrgb --noautoconnect -d 0 -m direct -c RRGGBB` (optional `-z N`).
pub fn apply_color(
    cfg: &Config,
    device: u32,
    zone: Option<u32>,
    color_hex: &str,
) -> Result<CommandResult> {
    let color = normalize_hex(color_hex)?;
    let mode = if cfg.mode.trim().is_empty() {
        "direct"
    } else {
        cfg.mode.trim()
    };

    // Build: -d N [-z Z] -m MODE -c RRGGBB
    let mut args: Vec<String> = vec!["-d".into(), device.to_string()];
    if let Some(z) = zone {
        args.push("-z".into());
        args.push(z.to_string());
    }
    args.push("-m".into());
    args.push(mode.to_string());
    args.push("-c".into());
    args.push(color);

    let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
    let mut result = run_openrgb(cfg, &arg_refs, cfg.wants_sudo())?;

    // Auto: never silently escalate; point at udev / OPENRGB_USE_SUDO=1.
    if !result.success && !result.used_sudo && looks_like_permission_error(&result.output) {
        result.output = format!(
            "{}\n\nHint: hidraw access denied. Re-run with OPENRGB_USE_SUDO=1, or ensure udev rules grant your user access to the Aura HID device.",
            result.output
        );
    }

    Ok(result)
}

/// Load a saved OpenRGB profile (e.g. `solid-red` at ~/.config/OpenRGB/solid-red.orp).
pub fn load_profile(cfg: &Config, profile: &str) -> Result<CommandResult> {
    let args = ["-p", profile];
    let mut result = run_openrgb(cfg, &args, cfg.wants_sudo())?;
    if !result.success && !result.used_sudo && looks_like_permission_error(&result.output) {
        result.output = format!(
            "{}\n\nHint: hidraw access denied. Try OPENRGB_USE_SUDO=1.",
            result.output
        );
    }
    Ok(result)
}

fn run_openrgb(cfg: &Config, extra: &[&str], use_sudo: bool) -> Result<CommandResult> {
    let mut argv: Vec<String> = Vec::new();
    if use_sudo {
        argv.push("sudo".into());
        argv.push("-n".into()); // non-interactive; fail fast if password needed
        argv.push(cfg.openrgb_path.clone());
    } else {
        argv.push(cfg.openrgb_path.clone());
    }
    if cfg.no_autoconnect {
        argv.push("--noautoconnect".into());
    }
    for a in extra {
        argv.push((*a).to_string());
    }

    let command_line = shell_join(&argv);
    let (program, args) = argv
        .split_first()
        .ok_or_else(|| anyhow!("empty command"))?;

    let mut cmd = Command::new(program);
    cmd.args(args);

    let output = cmd
        .output()
        .with_context(|| format!("spawning `{command_line}`"))?;

    let mut text = String::new();
    if !output.stdout.is_empty() {
        text.push_str(&String::from_utf8_lossy(&output.stdout));
    }
    if !output.stderr.is_empty() {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }
    text = filter_noise(&text);

    Ok(CommandResult {
        command_line,
        success: output.status.success(),
        output: truncate(&text, 800),
        used_sudo: use_sudo,
    })
}

/// Strip known-harmless I2C HTML warning blobs so the TUI stays readable.
fn filter_noise(s: &str) -> String {
    let mut out = String::new();
    for line in s.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.contains("i2c_smbus_linux")
            || t.contains("Failed to read i2c")
            || t.contains("Connection attempt failed")
        {
            continue;
        }
        if t.starts_with("<h") || t.starts_with("<p") || t.starts_with("<b") || t.contains("</") {
            continue;
        }
        if t.contains("I2C or SMBus interfaces failed")
            || t.contains("i2c-dev module")
            || t.contains("help.openrgb.org")
            || t.contains("If you are not using internal RGB")
        {
            continue;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

pub fn parse_devices(text: &str) -> Vec<DeviceInfo> {
    let mut devices = Vec::new();
    let mut current: Option<DeviceInfo> = None;

    for line in text.lines() {
        let trimmed = line.trim_end();
        if let Some((idx, name)) = parse_device_header(trimmed) {
            if let Some(dev) = current.take() {
                devices.push(dev);
            }
            current = Some(DeviceInfo {
                index: idx,
                name: name.to_string(),
                zones: Vec::new(),
            });
            continue;
        }
        let Some(dev) = current.as_mut() else {
            continue;
        };
        let t = trimmed.trim_start();
        if let Some(rest) = t.strip_prefix("Zones:") {
            dev.zones = parse_quoted_list(rest);
        }
    }
    if let Some(dev) = current {
        devices.push(dev);
    }

    // Generic fallback if parsing yields nothing useful.
    if devices.is_empty() {
        devices.push(DeviceInfo {
            index: 0,
            name: "OpenRGB device 0 (fallback)".into(),
            zones: vec![
                "Aura Mainboard".into(),
                "Aura Addressable 1".into(),
                "Aura Addressable 2".into(),
                "Aura Addressable 3".into(),
            ],
        });
    }

    devices
}

fn parse_device_header(line: &str) -> Option<(u32, &str)> {
    let line = line.trim();
    let (num, rest) = line.split_once(':')?;
    let idx: u32 = num.trim().parse().ok()?;
    let name = rest.trim();
    if name.is_empty() {
        return None;
    }
    // Avoid matching "Type: Motherboard" style lines.
    if num.chars().all(|c| c.is_ascii_digit()) {
        Some((idx, name))
    } else {
        None
    }
}

fn parse_quoted_list(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\'' {
            let mut name = String::new();
            for c2 in chars.by_ref() {
                if c2 == '\'' {
                    break;
                }
                name.push(c2);
            }
            if !name.is_empty() {
                out.push(name);
            }
        }
    }
    out
}

pub fn normalize_hex(input: &str) -> Result<String> {
    let s = input.trim().trim_start_matches('#').to_ascii_uppercase();
    if s.len() != 6 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
        bail!("invalid hex color `{input}` (expected RRGGBB)");
    }
    Ok(s)
}

fn looks_like_permission_error(output: &str) -> bool {
    let lower = output.to_ascii_lowercase();
    lower.contains("permission denied")
        || lower.contains("operation not permitted")
        || lower.contains("failed to open")
        || lower.contains("access denied")
        || lower.contains("a password is required")
        || lower.contains("no udev")
}

fn truncate(s: &str, max: usize) -> String {
    let mut t = s.trim().to_string();
    if t.chars().count() > max {
        t = t.chars().take(max).collect::<String>() + "…";
    }
    t
}

fn shell_join(parts: &[String]) -> String {
    parts
        .iter()
        .map(|p| {
            if p.contains(char::is_whitespace) {
                format!("'{p}'")
            } else {
                p.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Soft timeout hint for UI (openrgb itself is synchronous).
#[allow(dead_code)]
pub fn typical_duration() -> Duration {
    Duration::from_secs(20)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_aura_board() {
        let sample = r#"
0: Example Aura Motherboard
  Type:           Motherboard
  Zones: 'Aura Mainboard' 'Aura Addressable 1' 'Aura Addressable 2' 'Aura Addressable 3'
"#;
        let devices = parse_devices(sample);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].index, 0);
        assert_eq!(devices[0].name, "Example Aura Motherboard");
        assert_eq!(devices[0].zones.len(), 4);
    }

    #[test]
    fn normalizes_hex() {
        assert_eq!(normalize_hex("#ff0000").unwrap(), "FF0000");
        assert!(normalize_hex("GG0000").is_err());
    }
}
