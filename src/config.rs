use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SudoMode {
    /// Try without sudo (default). Set `OPENRGB_USE_SUDO=1` if access fails.
    #[default]
    Auto,
    Always,
    Never,
}

#[derive(Debug, Clone, Deserialize)]
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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            openrgb_path: "openrgb".into(),
            device: 0,
            use_sudo: SudoMode::Auto,
            no_autoconnect: true,
            mode: "direct".into(),
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

        Ok(cfg)
    }

    pub fn wants_sudo(&self) -> bool {
        matches!(self.use_sudo, SudoMode::Always)
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
