use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SudoMode {
    /// Try without sudo (default). Set `OPENRGB_USE_SUDO=1` if access fails.
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct Config {
    /// Path to the openrgb binary.
    pub openrgb_path: String,
    /// Device index from `openrgb --list-devices` (default `0`).
    pub device: u32,
    /// Whether to prefix commands with sudo.
    pub use_sudo: SudoMode,
    /// Pass `--noautoconnect` to avoid hanging on SDK client connect.
    pub no_autoconnect: bool,
    /// OpenRGB mode for apply (`direct` preferred on Aura; `static` also works).
    pub mode: String,
    /// Prefer OpenRGB SDK server (fast UpdateZoneLEDs). Falls back to CLI if unavailable.
    pub prefer_sdk: bool,
    /// Last-known solid color per zone index (`RRGGBB`), persisted across launches.
    pub zone_colors: BTreeMap<u32, String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            openrgb_path: "openrgb".into(),
            device: 0,
            use_sudo: SudoMode::Auto,
            no_autoconnect: true,
            mode: "direct".into(),
            prefer_sdk: true,
            zone_colors: BTreeMap::new(),
        }
    }
}

impl Config {
    pub fn load() -> Result<Self> {
        let mut cfg = Self::default();
        let path = config_file_path();

        if path.exists() {
            let text = fs::read_to_string(&path)
                .with_context(|| format!("reading {}", path.display()))?;
            let file_cfg: Config = toml::from_str(&text)
                .with_context(|| format!("parsing {}", path.display()))?;
            cfg = file_cfg;
        }

        if let Ok(path) = env::var("OPENRGB_PATH") {
            if !path.is_empty() {
                cfg.openrgb_path = path;
            }
        }
        if let Ok(device) = env::var("OPENRGB_DEVICE") {
            if let Ok(n) = device.parse() {
                cfg.device = n;
            }
        }
        if let Ok(v) = env::var("OPENRGB_USE_SUDO") {
            cfg.use_sudo = match v.trim().to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "always" | "on" => SudoMode::Always,
                "0" | "false" | "no" | "never" | "off" => SudoMode::Never,
                "auto" | "" => SudoMode::Auto,
                other => {
                    eprintln!("warning: unknown OPENRGB_USE_SUDO={other:?}, using auto");
                    SudoMode::Auto
                }
            };
        }
        if let Ok(mode) = env::var("OPENRGB_MODE") {
            if !mode.is_empty() {
                cfg.mode = mode;
            }
        }
        if let Ok(v) = env::var("COLORFAN_SDK") {
            cfg.prefer_sdk = match v.trim().to_ascii_lowercase().as_str() {
                "0" | "false" | "no" | "off" => false,
                "1" | "true" | "yes" | "on" | "" => true,
                other => {
                    eprintln!("warning: unknown COLORFAN_SDK={other:?}, using true");
                    true
                }
            };
        }

        Ok(cfg)
    }

    pub fn wants_sudo(&self) -> bool {
        matches!(self.use_sudo, SudoMode::Always)
    }

    /// Persist config (including last-known zone colors) to the XDG config path
    /// (or `COLORFAN_CONFIG` if set). Creates the parent directory if needed.
    pub fn save(&self) -> Result<PathBuf> {
        let path = config_file_path();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
        }
        let text = toml::to_string_pretty(self).context("serializing colorfan.toml")?;
        fs::write(&path, text).with_context(|| format!("writing {}", path.display()))?;
        Ok(path)
    }

    pub fn color_for_zone(&self, zone: u32) -> Option<&str> {
        self.zone_colors.get(&zone).map(String::as_str)
    }
}

/// Resolved config file path.
///
/// Priority:
/// 1. `COLORFAN_CONFIG` — absolute override
/// 2. `$XDG_CONFIG_HOME/colorfan/colorfan.toml` (default `~/.config/colorfan/colorfan.toml`)
///
/// If the XDG file is missing but `./colorfan.toml` exists (legacy repo/cwd layout),
/// it is copied once to the XDG path, then the new path is used.
pub fn config_file_path() -> PathBuf {
    if let Ok(p) = env::var("COLORFAN_CONFIG") {
        if !p.is_empty() {
            return PathBuf::from(p);
        }
    }

    let path = default_xdg_config_path();
    migrate_legacy_cwd_config(&path);
    path
}

fn default_xdg_config_path() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("colorfan").join("colorfan.toml");
        }
    }
    if let Ok(home) = env::var("HOME") {
        if !home.is_empty() {
            return PathBuf::from(home)
                .join(".config")
                .join("colorfan")
                .join("colorfan.toml");
        }
    }
    // Last resort when HOME / XDG_CONFIG_HOME are unset (unusual).
    PathBuf::from("colorfan.toml")
}

/// One-shot copy from legacy `./colorfan.toml` into the XDG location.
fn migrate_legacy_cwd_config(xdg: &Path) {
    if xdg.exists() {
        return;
    }
    let legacy = Path::new("colorfan.toml");
    if !legacy.is_file() {
        return;
    }
    // Avoid "migrating" onto itself if we fell back to cwd (no HOME).
    if let (Ok(a), Ok(b)) = (legacy.canonicalize(), xdg.canonicalize()) {
        if a == b {
            return;
        }
    } else if xdg == legacy {
        return;
    }

    if let Some(parent) = xdg.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!(
                "warning: could not create {} to migrate config: {e}",
                parent.display()
            );
            return;
        }
    }

    match fs::copy(legacy, xdg) {
        Ok(_) => eprintln!(
            "note: migrated ./colorfan.toml → {} (safe to delete the old file)",
            xdg.display()
        ),
        Err(e) => eprintln!(
            "warning: could not migrate ./colorfan.toml → {}: {e}",
            xdg.display()
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zone_colors_roundtrip() {
        let text = r#"
openrgb_path = "openrgb"
device = 0
use_sudo = "auto"
no_autoconnect = true
mode = "direct"

[zone_colors]
0 = "000000"
1 = "0000FF"
2 = "00FF00"
3 = "FF0000"
"#;
        let cfg: Config = toml::from_str(text).unwrap();
        assert_eq!(cfg.zone_colors.get(&1).unwrap(), "0000FF");
        let out = toml::to_string_pretty(&cfg).unwrap();
        let cfg2: Config = toml::from_str(&out).unwrap();
        assert_eq!(cfg2.zone_colors.get(&3).unwrap(), "FF0000");
    }
}
