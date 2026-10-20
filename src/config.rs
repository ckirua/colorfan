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

        if let Some(path) = config_file_path() {
            if path.exists() {
                let text = fs::read_to_string(&path)
                    .with_context(|| format!("reading {}", path.display()))?;
                let file_cfg: Config = toml::from_str(&text)
                    .with_context(|| format!("parsing {}", path.display()))?;
                cfg = file_cfg;
            }
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

    /// Persist config (including last-known zone colors) to the resolved config path.
    pub fn save(&self) -> Result<PathBuf> {
        let path = config_file_path().unwrap_or_else(|| PathBuf::from("colorfan.toml"));
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

fn config_file_path() -> Option<PathBuf> {
    if let Ok(p) = env::var("COLORFAN_CONFIG") {
        return Some(PathBuf::from(p));
    }
    let cwd = Path::new("colorfan.toml");
    if cwd.exists() {
        return Some(cwd.to_path_buf());
    }
    if let Ok(home) = env::var("HOME") {
        let p = PathBuf::from(home).join(".config/colorfan/colorfan.toml");
        if p.exists() {
            return Some(p);
        }
    }
    Some(PathBuf::from("colorfan.toml"))
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
