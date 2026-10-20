use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Instant;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::Config;
use crate::openrgb::{self, CommandResult, DeviceInfo};

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

/// Default when a zone has never been set (avoids omitting it from the multi-zone CLI).
const DEFAULT_ZONE_COLOR: &str = "000000";

const SPINNER: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

enum JobOutcome {
    Apply {
        hex: String,
        target_label: String,
        zone_count: usize,
        result: std::result::Result<CommandResult, String>,
    },
    Profile {
        profile: String,
        result: std::result::Result<CommandResult, String>,
    },
    Reload {
        result: std::result::Result<Vec<DeviceInfo>, String>,
    },
}

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
    /// Last-known color per zone index on the current device (aligned with `device().zones`).
    pub zone_colors: Vec<String>,
    pub last_command: String,
    pub last_status: String,
    pub last_ok: Option<bool>,
    pub status_line: String,
    pub should_quit: bool,
    pub busy: bool,
    pub busy_label: String,
    pub busy_started: Option<Instant>,
    job_rx: Option<Receiver<JobOutcome>>,
}

impl App {
    pub fn new(config: Config, devices: Vec<DeviceInfo>) -> Self {
        let device_idx = devices
            .iter()
            .position(|d| d.index == config.device)
            .unwrap_or(0);
        let color_hex = PRESETS[0].hex.to_string();
        let zone_colors = seed_zone_colors(&config, &devices[device_idx]);
        Self {
            config,
            devices,
            device_idx,
            zone_sel: 0,
            preset_sel: 0,
            color_hex: color_hex.clone(),
            hex_input: color_hex,
            focus: Focus::Presets,
            zone_colors,
            last_command: String::new(),
            last_status: "Ready. Pick a color and press Enter to apply.".into(),
            last_ok: None,
            status_line: String::new(),
            should_quit: false,
            busy: false,
            busy_label: String::new(),
            busy_started: None,
            job_rx: None,
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

    /// Last-known hex for list row `sel` (None for "All zones").
    pub fn zone_stored_color(&self, sel: usize) -> Option<&str> {
        if sel == 0 {
            None
        } else {
            self.zone_colors.get(sel - 1).map(String::as_str)
        }
    }

    /// Animated busy label for the status bar (call each frame while `busy`).
    pub fn busy_display(&self) -> Option<String> {
        if !self.busy {
            return None;
        }
        let tick = self
            .busy_started
            .map(|t| (t.elapsed().as_millis() / 80) as usize)
            .unwrap_or(0);
        let spin = SPINNER[tick % SPINNER.len()];
        let secs = self
            .busy_started
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(0);
        Some(format!("{spin} {} ({secs}s)", self.busy_label))
    }

    /// Poll background OpenRGB work; returns true if UI state changed.
    pub fn poll_jobs(&mut self) -> bool {
        let Some(rx) = self.job_rx.as_ref() else {
            return false;
        };
        match rx.try_recv() {
            Ok(outcome) => {
                self.job_rx = None;
                self.finish_job(outcome);
                true
            }
            Err(TryRecvError::Empty) => false,
            Err(TryRecvError::Disconnected) => {
                self.job_rx = None;
                self.busy = false;
                self.busy_started = None;
                self.last_ok = Some(false);
                self.last_status = "Background OpenRGB task disconnected".into();
                self.status_line = "Failed".into();
                true
            }
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Result<()> {
        // Always allow quit, even while busy.
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                self.should_quit = true;
                return Ok(());
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.should_quit = true;
                return Ok(());
            }
            _ => {}
        }

        if self.busy {
            return Ok(());
        }

        match (self.focus, key.code) {
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
                self.apply();
            }
            (_, KeyCode::Enter) => {
                if self.focus == Focus::Hex {
                    self.commit_hex_input();
                }
                self.apply();
            }
            (_, KeyCode::Char(c @ '1'..='8')) => {
                let i = (c as u8 - b'1') as usize;
                self.select_preset(i);
                self.apply();
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
                self.load_profile("solid-red");
            }
            _ => {}
        }
        Ok(())
    }

    fn start_busy(&mut self, label: impl Into<String>) {
        self.busy = true;
        self.busy_label = label.into();
        self.busy_started = Some(Instant::now());
        self.status_line = self.busy_label.clone();
        self.last_ok = None;
    }

    fn finish_job(&mut self, outcome: JobOutcome) {
        self.busy = false;
        self.busy_started = None;
        self.busy_label.clear();

        match outcome {
            JobOutcome::Apply {
                hex,
                target_label,
                zone_count,
                result,
            } => match result {
                Ok(r) => {
                    self.last_command = r.command_line;
                    self.last_ok = Some(r.success);
                    if r.success {
                        let ms = r.duration.as_millis();
                        self.last_status = format!(
                            "Applied #{hex} → {target_label} ({zone_count} zones via {}, {ms} ms)",
                            r.backend
                        );
                        if !r.output.is_empty() {
                            self.last_status.push('\n');
                            self.last_status.push_str(&r.output);
                        }
                        self.status_line = format!("OK ({})", r.backend);
                        if let Err(e) = self.config.save() {
                            self.last_status
                                .push_str(&format!("\n(warning: could not save colors: {e:#})"));
                        }
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
                    self.last_status = e;
                    self.status_line = "Failed".into();
                }
            },
            JobOutcome::Profile { profile, result } => match result {
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
                    self.last_status = e;
                    self.status_line = "Failed".into();
                }
            },
            JobOutcome::Reload { result } => match result {
                Ok(devices) => {
                    self.devices = devices;
                    if self.device_idx >= self.devices.len() {
                        self.device_idx = 0;
                    }
                    if self.zone_sel >= self.zone_count() {
                        self.zone_sel = 0;
                    }
                    let seeded = seed_zone_colors(&self.config, self.device());
                    let mut merged = seeded;
                    for (i, c) in self.zone_colors.iter().enumerate() {
                        if i < merged.len() {
                            merged[i] = c.clone();
                        }
                    }
                    self.zone_colors = merged;
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
                    self.last_status = e;
                    self.status_line = "Reload failed".into();
                }
            },
        }
    }

    fn load_profile(&mut self, profile: &str) {
        if self.busy {
            return;
        }
        self.start_busy(format!("Loading profile `{profile}`…"));
        let (tx, rx) = mpsc::channel();
        self.job_rx = Some(rx);
        let cfg = self.config.clone();
        let profile = profile.to_string();
        thread::spawn(move || {
            let result = openrgb::load_profile(&cfg, &profile)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(JobOutcome::Profile { profile, result });
        });
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

    pub fn apply(&mut self) {
        if self.busy {
            return;
        }

        let hex = match openrgb::normalize_hex(&self.color_hex) {
            Ok(h) => h,
            Err(e) => {
                self.last_ok = Some(false);
                self.last_status = e.to_string();
                return;
            }
        };

        self.ensure_zone_colors_len();

        let target_zone = self.selected_zone();
        match target_zone {
            None => {
                for c in &mut self.zone_colors {
                    *c = hex.clone();
                }
            }
            Some(z) => {
                let i = z as usize;
                if i < self.zone_colors.len() {
                    self.zone_colors[i] = hex.clone();
                }
            }
        }

        // Sync into config for persistence (saved after successful apply).
        self.config.zone_colors.clear();
        for (i, color) in self.zone_colors.iter().enumerate() {
            self.config.zone_colors.insert(i as u32, color.clone());
        }

        let zones: Vec<(u32, String)> = self
            .zone_colors
            .iter()
            .enumerate()
            .map(|(i, c)| (i as u32, c.clone()))
            .collect();

        if zones.is_empty() {
            self.last_ok = Some(false);
            self.last_status = "No zones available to apply".into();
            self.status_line = "Failed".into();
            return;
        }

        let target_label = match target_zone {
            None => "all zones".into(),
            Some(z) => self.zone_label((z as usize) + 1),
        };
        let zone_count = zones.len();
        let device = self.device().index;
        let cfg = self.config.clone();

        self.start_busy("Applying…");
        self.last_status = "Applying colors via OpenRGB…".into();
        self.last_command.clear();

        let (tx, rx) = mpsc::channel();
        self.job_rx = Some(rx);

        thread::spawn(move || {
            let zone_refs: Vec<(u32, &str)> =
                zones.iter().map(|(z, c)| (*z, c.as_str())).collect();
            let result = openrgb::apply_zones(&cfg, device, &zone_refs)
                .map_err(|e| format!("{e:#}"));
            let _ = tx.send(JobOutcome::Apply {
                hex,
                target_label,
                zone_count,
                result,
            });
        });
    }

    fn ensure_zone_colors_len(&mut self) {
        let n = self.device().zones.len();
        if self.zone_colors.len() < n {
            self.zone_colors
                .resize(n, DEFAULT_ZONE_COLOR.to_string());
        } else if self.zone_colors.len() > n {
            self.zone_colors.truncate(n);
        }
    }

    fn reload_devices(&mut self) {
        if self.busy {
            return;
        }
        self.start_busy("Reloading devices…");
        let (tx, rx) = mpsc::channel();
        self.job_rx = Some(rx);
        let cfg = self.config.clone();
        thread::spawn(move || {
            let result = openrgb::list_devices(&cfg).map_err(|e| format!("{e:#}"));
            let _ = tx.send(JobOutcome::Reload { result });
        });
    }
}

fn seed_zone_colors(config: &Config, device: &DeviceInfo) -> Vec<String> {
    (0..device.zones.len())
        .map(|i| {
            config
                .color_for_zone(i as u32)
                .map(|s| s.to_ascii_uppercase())
                .filter(|s| openrgb::normalize_hex(s).is_ok())
                .unwrap_or_else(|| DEFAULT_ZONE_COLOR.to_string())
        })
        .collect()
}
