use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::Config;
use crate::openrgb::{self, DeviceInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Zones,
    Presets,
    Hex,
}

#[derive(Debug, Clone, Copy)]
pub struct Preset {
    pub name: &'static str,
    pub hex: &'static str,
}

pub const PRESETS: &[Preset] = &[
    Preset {
        name: "Red",
        hex: "FF0000",
    },
    Preset {
        name: "Green",
        hex: "00FF00",
    },
    Preset {
        name: "Blue",
        hex: "0000FF",
    },
    Preset {
        name: "White",
        hex: "FFFFFF",
    },
    Preset {
        name: "Off",
        hex: "000000",
    },
    Preset {
        name: "Purple",
        hex: "A020F0",
    },
    Preset {
        name: "Cyan",
        hex: "00FFFF",
    },
    Preset {
        name: "Orange",
        hex: "FF8000",
    },
];

pub struct App {
    pub config: Config,
    pub devices: Vec<DeviceInfo>,
    pub device_idx: usize,
    /// 0 = All zones, 1.. = zone index + 1
    pub zone_sel: usize,
    pub preset_sel: usize,
    pub color_hex: String,
    pub hex_input: String,
    pub focus: Focus,
    pub last_command: String,
    pub last_status: String,
    pub last_ok: Option<bool>,
    pub status_line: String,
    pub should_quit: bool,
    pub busy: bool,
}

impl App {
    pub fn new(config: Config, devices: Vec<DeviceInfo>) -> Self {
        let device_idx = devices
            .iter()
            .position(|d| d.index == config.device)
            .unwrap_or(0);
        let color_hex = PRESETS[0].hex.to_string();
        Self {
            config,
            devices,
            device_idx,
            zone_sel: 0,
            preset_sel: 0,
            color_hex: color_hex.clone(),
            hex_input: color_hex,
            focus: Focus::Presets,
            last_command: String::new(),
            last_status: "Ready. Pick a color and press Enter to apply.".into(),
            last_ok: None,
            status_line: String::new(),
            should_quit: false,
            busy: false,
        }
    }

    pub fn device(&self) -> &DeviceInfo {
        &self.devices[self.device_idx]
    }

    pub fn zone_count(&self) -> usize {
        // +1 for "All zones"
        self.device().zones.len() + 1
    }

    pub fn selected_zone(&self) -> Option<u32> {
        if self.zone_sel == 0 {
            None
        } else {
            Some((self.zone_sel - 1) as u32)
        }
    }

    pub fn zone_label(&self, sel: usize) -> String {
        if sel == 0 {
            "All zones".into()
        } else {
            let zones = &self.device().zones;
            let i = sel - 1;
            zones
                .get(i)
                .cloned()
                .unwrap_or_else(|| format!("Zone {i}"))
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        if self.busy {
            return Ok(());
        }

        match (self.focus, key.code) {
            (_, KeyCode::Char('q')) | (_, KeyCode::Esc) => {
                self.should_quit = true;
            }
            (_, KeyCode::Char('c')) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
            }
            (_, KeyCode::Tab) => self.cycle_focus(1),
            (_, KeyCode::BackTab) => self.cycle_focus(-1),
            (Focus::Hex, KeyCode::Char(c)) if c.is_ascii_hexdigit() || c == '#' => {
                if self.hex_input.len() < 7 {
                    self.hex_input.push(c.to_ascii_uppercase());
                }
            }
            (Focus::Hex, KeyCode::Backspace) => {
                self.hex_input.pop();
            }
            (Focus::Hex, KeyCode::Enter) => {
                self.commit_hex_input();
                self.apply()?;
            }
            (_, KeyCode::Enter) => {
                if self.focus == Focus::Hex {
                    self.commit_hex_input();
                }
                self.apply()?;
            }
            (_, KeyCode::Char(c @ '1'..='8')) => {
                let i = (c as u8 - b'1') as usize;
                self.select_preset(i);
                self.apply()?;
            }
            (Focus::Zones, KeyCode::Up | KeyCode::Char('k')) => {
                self.zone_sel = self.zone_sel.saturating_sub(1);
            }
            (Focus::Zones, KeyCode::Down | KeyCode::Char('j')) => {
                if self.zone_sel + 1 < self.zone_count() {
                    self.zone_sel += 1;
                }
            }
            (Focus::Presets, KeyCode::Up | KeyCode::Char('k')) => {
                self.preset_sel = self.preset_sel.saturating_sub(1);
                self.select_preset(self.preset_sel);
            }
            (Focus::Presets, KeyCode::Down | KeyCode::Char('j')) => {
                if self.preset_sel + 1 < PRESETS.len() {
                    self.preset_sel += 1;
                    self.select_preset(self.preset_sel);
                }
            }
            (_, KeyCode::Left | KeyCode::Char('h')) => self.cycle_focus(-1),
            (_, KeyCode::Right | KeyCode::Char('l')) => self.cycle_focus(1),
            (_, KeyCode::Char('r')) => {
                self.reload_devices();
            }
            (_, KeyCode::Char('a')) => {
                self.zone_sel = 0;
                self.status_line = "Target: all zones".into();
            }
            (_, KeyCode::Char('p')) => {
                self.load_profile("solid-red")?;
            }
            _ => {}
        }
        Ok(())
    }

    fn load_profile(&mut self, profile: &str) -> Result<()> {
        self.busy = true;
        self.status_line = format!("Loading profile `{profile}`…");
        let result = openrgb::load_profile(&self.config, profile);
        self.busy = false;
        match result {
            Ok(r) => {
                self.last_command = r.command_line;
                self.last_ok = Some(r.success);
                self.last_status = if r.success {
                    format!("Loaded profile `{profile}`")
                } else if r.output.is_empty() {
                    format!("Failed to load profile `{profile}`")
                } else {
                    r.output
                };
                self.status_line = if r.success {
                    "OK".into()
                } else {
                    "Failed".into()
                };
            }
            Err(e) => {
                self.last_ok = Some(false);
                self.last_command.clear();
                self.last_status = format!("{e:#}");
                self.status_line = "Failed".into();
            }
        }
        Ok(())
    }

    fn cycle_focus(&mut self, dir: i8) {
        self.focus = match (self.focus, dir >= 0) {
            (Focus::Zones, true) => Focus::Presets,
            (Focus::Presets, true) => Focus::Hex,
            (Focus::Hex, true) => Focus::Zones,
            (Focus::Zones, false) => Focus::Hex,
            (Focus::Presets, false) => Focus::Zones,
            (Focus::Hex, false) => Focus::Presets,
        };
    }

    fn select_preset(&mut self, i: usize) {
        if i >= PRESETS.len() {
            return;
        }
        self.preset_sel = i;
        self.color_hex = PRESETS[i].hex.to_string();
        self.hex_input = self.color_hex.clone();
        self.status_line = format!("Preset: {}", PRESETS[i].name);
    }

    fn commit_hex_input(&mut self) {
        match openrgb::normalize_hex(&self.hex_input) {
            Ok(hex) => {
                self.color_hex = hex.clone();
                self.hex_input = hex;
                self.status_line = format!("Color set to #{}", self.color_hex);
            }
            Err(e) => {
                self.last_ok = Some(false);
                self.last_status = e.to_string();
            }
        }
    }

    pub fn apply(&mut self) -> Result<()> {
        let hex = match openrgb::normalize_hex(&self.color_hex) {
            Ok(h) => h,
            Err(e) => {
                self.last_ok = Some(false);
                self.last_status = e.to_string();
                return Ok(());
            }
        };

        self.busy = true;
        self.status_line = "Applying… (OpenRGB may take a few seconds)".into();

        let device = self.device().index;
        let zone = self.selected_zone();
        let result = openrgb::apply_color(&self.config, device, zone, &hex);

        self.busy = false;

        match result {
            Ok(r) => {
                self.last_command = r.command_line;
                self.last_ok = Some(r.success);
                if r.success {
                    let target = match zone {
                        None => "all zones".into(),
                        Some(z) => self.zone_label((z as usize) + 1),
                    };
                    self.last_status = format!("Applied #{hex} → {target}");
                    if !r.output.is_empty() {
                        self.last_status.push_str("\n");
                        self.last_status.push_str(&r.output);
                    }
                    self.status_line = "OK".into();
                } else {
                    self.last_status = if r.output.is_empty() {
                        "openrgb exited with an error".into()
                    } else {
                        r.output
                    };
                    self.status_line = "Failed".into();
                }
            }
            Err(e) => {
                self.last_ok = Some(false);
                self.last_command.clear();
                self.last_status = format!("{e:#}");
                self.status_line = "Failed".into();
            }
        }
        Ok(())
    }

    fn reload_devices(&mut self) {
        self.status_line = "Reloading devices…".into();
        match openrgb::list_devices(&self.config) {
            Ok(devices) => {
                self.devices = devices;
                if self.device_idx >= self.devices.len() {
                    self.device_idx = 0;
                }
                if self.zone_sel >= self.zone_count() {
                    self.zone_sel = 0;
                }
                self.last_ok = Some(true);
                self.last_status = format!(
                    "Loaded {} device(s). Current: {}",
                    self.devices.len(),
                    self.device().name
                );
                self.status_line = "Devices refreshed".into();
            }
            Err(e) => {
                self.last_ok = Some(false);
                self.last_status = format!("{e:#}");
                self.status_line = "Reload failed".into();
            }
        }
    }
}
