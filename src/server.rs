//! Supervise a long-lived `openrgb --server` for SDK applies.

use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::config::Config;
use crate::sdk;

const SDK_ADDR: &str = "127.0.0.1:6742";

pub struct OpenRgbServer {
    child: Option<Child>,
    /// True when this process started the server (so we should stop it on drop).
    owned: bool,
}

impl OpenRgbServer {
    /// Connect to an existing SDK server, or start `openrgb --server` if needed.
    pub fn ensure(cfg: &Config) -> Result<Self> {
        if sdk::sdk_port_open() {
            return Ok(Self {
                child: None,
                owned: false,
            });
        }

        if !cfg.prefer_sdk {
            bail!("OpenRGB SDK server not running and prefer_sdk is false");
        }

        let mut argv: Vec<String> = Vec::new();
        if cfg.wants_sudo() {
            argv.push("sudo".into());
            argv.push("-n".into());
        }
        argv.push(cfg.openrgb_path.clone());
        argv.push("--server".into());
        if cfg.no_autoconnect {
            argv.push("--noautoconnect".into());
        }

        let (program, args) = argv
            .split_first()
            .ok_or_else(|| anyhow::anyhow!("empty openrgb server command"))?;

        eprintln!(
            "colorfan: starting OpenRGB SDK server (`{}`) — one-time probe, ~5–10s…",
            argv.join(" ")
        );

        let child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("spawning OpenRGB server via `{program}`"))?;

        let deadline = Instant::now() + Duration::from_secs(30);
        let mut port_ready_at: Option<Instant> = None;
        while Instant::now() < deadline {
            if TcpStream::connect_timeout(
                &SDK_ADDR.parse().unwrap(),
                Duration::from_millis(200),
            )
            .is_ok()
            {
                if port_ready_at.is_none() {
                    port_ready_at = Some(Instant::now());
                }
                // Port opens before device detection finishes; wait until SDK sees controllers
                // (or a few seconds after listen, for hosts with zero devices).
                if sdk_has_controllers()
                    || port_ready_at
                        .map(|t| t.elapsed() > Duration::from_secs(12))
                        .unwrap_or(false)
                {
                    return Ok(Self {
                        child: Some(child),
                        owned: true,
                    });
                }
            }
            thread::sleep(Duration::from_millis(250));
        }

        let mut child = child;
        let _ = child.kill();
        bail!("timed out waiting for OpenRGB SDK server on {SDK_ADDR}");
    }

    pub fn owned(&self) -> bool {
        self.owned
    }
}

impl Drop for OpenRgbServer {
    fn drop(&mut self) {
        if !self.owned {
            return;
        }
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn sdk_has_controllers() -> bool {
    match crate::sdk::SdkClient::connect() {
        Ok(mut c) => c
            .list_controllers()
            .map(|v| !v.is_empty())
            .unwrap_or(false),
        Err(_) => false,
    }
}
