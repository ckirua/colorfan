//! Minimal synchronous OpenRGB SDK client (protocol ≤ 3).
//!
//! Used against a long-lived `openrgb --server` so applies avoid cold-start
//! device probing. Zone updates use `UpdateZoneLEDs` (true per-zone, no sibling blanking).

use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};

const MAGIC: &[u8; 4] = b"ORGB";
const DEFAULT_ADDR: &str = "127.0.0.1:6742";
/// Cap at 3 so controller payloads omit v4+ segments / v5 flags / v6 fields.
const CLIENT_PROTOCOL: u32 = 3;

const ID_REQUEST_CONTROLLER_COUNT: u32 = 0;
const ID_REQUEST_CONTROLLER_DATA: u32 = 1;
const ID_REQUEST_PROTOCOL_VERSION: u32 = 40;
const ID_SET_CLIENT_NAME: u32 = 50;
const ID_UPDATE_ZONE_LEDS: u32 = 1051;
const ID_SET_CUSTOM_MODE: u32 = 1100;

#[derive(Debug, Clone)]
pub struct SdkZone {
    pub name: String,
    pub leds_count: u32,
}

#[derive(Debug, Clone)]
pub struct SdkController {
    pub id: u32,
    pub name: String,
    pub zones: Vec<SdkZone>,
}

pub struct SdkClient {
    stream: TcpStream,
    protocol: u32,
}

impl SdkClient {
    pub fn connect() -> Result<Self> {
        Self::connect_to(DEFAULT_ADDR)
    }

    pub fn connect_to(addr: &str) -> Result<Self> {
        let stream = TcpStream::connect_timeout(
            &addr
                .parse()
                .with_context(|| format!("parsing SDK address `{addr}`"))?,
            Duration::from_secs(2),
        )
        .with_context(|| format!("connecting to OpenRGB SDK at {addr}"))?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        stream.set_write_timeout(Some(Duration::from_secs(5)))?;

        let mut client = Self {
            stream,
            protocol: CLIENT_PROTOCOL,
        };

        // Negotiate protocol (lowest of client/server). Older servers may not reply.
        match client.request_u32(0, ID_REQUEST_PROTOCOL_VERSION, &CLIENT_PROTOCOL.to_le_bytes()) {
            Ok(server_ver) => {
                client.protocol = CLIENT_PROTOCOL.min(server_ver);
            }
            Err(_) => {
                client.protocol = 0;
            }
        }

        client.send_packet(0, ID_SET_CLIENT_NAME, b"colorfan\0")?;
        Ok(client)
    }

    pub fn list_controllers(&mut self) -> Result<Vec<SdkController>> {
        let count = self.request_u32(0, ID_REQUEST_CONTROLLER_COUNT, &[])?;
        let mut out = Vec::with_capacity(count as usize);
        for id in 0..count {
            let payload = if self.protocol == 0 {
                Vec::new()
            } else {
                self.protocol.to_le_bytes().to_vec()
            };
            let data = self.request_bytes(id, ID_REQUEST_CONTROLLER_DATA, &payload)?;
            let (name, zones) = parse_controller(&data, self.protocol)?;
            out.push(SdkController { id, name, zones });
        }
        Ok(out)
    }

    /// Set Direct/custom mode, then paint each `(zone_idx, RRGGBB)` via UpdateZoneLEDs.
    pub fn apply_zones(&mut self, device: u32, zones: &[(u32, &str)]) -> Result<()> {
        let controllers = self.list_controllers()?;
        let ctrl = controllers
            .iter()
            .find(|c| c.id == device)
            .ok_or_else(|| anyhow!("SDK: no controller with id {device}"))?;

        self.send_packet(device, ID_SET_CUSTOM_MODE, &[])?;

        for (zone_idx, hex) in zones {
            let zone = ctrl
                .zones
                .get(*zone_idx as usize)
                .ok_or_else(|| anyhow!("SDK: zone {zone_idx} out of range"))?;
            let (r, g, b) = parse_rgb(hex)?;
            let n = zone.leds_count as usize;
            // OpenRGB requires data_size == pkt_size (includes the data_size field itself).
            // Wrong value → server logs "UpdateZoneLEDs packet has invalid size" and drops the apply.
            let data_size = update_zone_leds_data_size(n);
            let mut payload = Vec::with_capacity(data_size);
            payload.extend_from_slice(&(data_size as u32).to_le_bytes());
            payload.extend_from_slice(&zone_idx.to_le_bytes());
            payload.extend_from_slice(&(n as u16).to_le_bytes());
            for _ in 0..n {
                payload.push(r);
                payload.push(g);
                payload.push(b);
                payload.push(0);
            }
            debug_assert_eq!(payload.len(), data_size);
            self.send_packet(device, ID_UPDATE_ZONE_LEDS, &payload)?;
        }
        Ok(())
    }

    fn send_packet(&mut self, device_id: u32, packet_id: u32, data: &[u8]) -> Result<()> {
        let mut hdr = Vec::with_capacity(16 + data.len());
        hdr.extend_from_slice(MAGIC);
        hdr.extend_from_slice(&device_id.to_le_bytes());
        hdr.extend_from_slice(&packet_id.to_le_bytes());
        hdr.extend_from_slice(&(data.len() as u32).to_le_bytes());
        hdr.extend_from_slice(data);
        self.stream
            .write_all(&hdr)
            .context("writing OpenRGB SDK packet")?;
        Ok(())
    }

    fn read_packet(&mut self, expect_device: u32, expect_id: u32) -> Result<Vec<u8>> {
        // Skip unsolicited packets (e.g. DEVICE_LIST_UPDATED) a few times.
        for _ in 0..8 {
            let mut magic = [0u8; 4];
            self.stream
                .read_exact(&mut magic)
                .context("reading OpenRGB magic")?;
            if &magic != MAGIC {
                bail!("OpenRGB SDK: bad magic");
            }
            let device_id = read_u32(&mut self.stream)?;
            let packet_id = read_u32(&mut self.stream)?;
            let size = read_u32(&mut self.stream)? as usize;
            let mut data = vec![0u8; size];
            if size > 0 {
                self.stream
                    .read_exact(&mut data)
                    .context("reading OpenRGB payload")?;
            }
            if device_id == expect_device && packet_id == expect_id {
                return Ok(data);
            }
            // ignore and continue
        }
        bail!("OpenRGB SDK: expected packet {expect_id} for device {expect_device}")
    }

    fn request_bytes(&mut self, device_id: u32, packet_id: u32, data: &[u8]) -> Result<Vec<u8>> {
        self.send_packet(device_id, packet_id, data)?;
        self.read_packet(device_id, packet_id)
    }

    fn request_u32(&mut self, device_id: u32, packet_id: u32, data: &[u8]) -> Result<u32> {
        let resp = self.request_bytes(device_id, packet_id, data)?;
        if resp.len() < 4 {
            bail!("OpenRGB SDK: short u32 response ({})", resp.len());
        }
        Ok(u32::from_le_bytes([resp[0], resp[1], resp[2], resp[3]]))
    }
}

fn read_u32(r: &mut impl Read) -> Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn parse_rgb(hex: &str) -> Result<(u8, u8, u8)> {
    let s = hex.trim().trim_start_matches('#');
    if s.len() != 6 {
        bail!("invalid hex `{hex}`");
    }
    let r = u8::from_str_radix(&s[0..2], 16)?;
    let g = u8::from_str_radix(&s[2..4], 16)?;
    let b = u8::from_str_radix(&s[4..6], 16)?;
    Ok((r, g, b))
}

/// OpenRGB `UpdateZoneLEDs` payload `data_size` (must equal full packet body length).
fn update_zone_leds_data_size(num_leds: usize) -> usize {
    4 + 4 + 2 + 4 * num_leds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_zone_leds_data_size_includes_size_field() {
        // 2 LEDs: data_size(4) + zone(4) + num(2) + 2*RGBColor(8) = 18
        assert_eq!(update_zone_leds_data_size(2), 18);
        // 60 LEDs: 4+4+2+240 = 250
        assert_eq!(update_zone_leds_data_size(60), 250);
        // Regression: old bug used 4+2+4*n (excluded data_size) → server rejected applies.
        assert_ne!(update_zone_leds_data_size(2), 4 + 2 + 8);
    }
}

fn parse_controller(data: &[u8], protocol: u32) -> Result<(String, Vec<SdkZone>)> {
    let mut c = Cursor::new(data);
    let _data_size = c.u32()?; // leading size field inside payload
    let _ty = c.u32()?;
    let name = c.string()?;
    if protocol >= 1 {
        let _vendor = c.string()?;
    }
    let _description = c.string()?;
    let _version = c.string()?;
    let _serial = c.string()?;
    let _location = c.string()?;

    let num_modes = c.u16()? as usize;
    let _active_mode = c.i32()?;
    for _ in 0..num_modes {
        skip_mode(&mut c, protocol)?;
    }

    let num_zones = c.u16()? as usize;
    let mut zones = Vec::with_capacity(num_zones);
    for _ in 0..num_zones {
        zones.push(read_zone(&mut c, protocol)?);
    }

    Ok((name, zones))
}

fn skip_mode(c: &mut Cursor<'_>, protocol: u32) -> Result<()> {
    let _name = c.string()?;
    let _value = c.i32()?;
    let _flags = c.u32()?;
    let _speed_min = c.u32()?;
    let _speed_max = c.u32()?;
    if protocol >= 3 {
        let _brightness_min = c.u32()?;
        let _brightness_max = c.u32()?;
    }
    let _colors_min = c.u32()?;
    let _colors_max = c.u32()?;
    let _speed = c.u32()?;
    if protocol >= 3 {
        let _brightness = c.u32()?;
    }
    let _direction = c.u32()?;
    let _color_mode = c.u32()?;
    let n = c.u16()? as usize;
    for _ in 0..n {
        let _ = c.u32()?; // RGBColor
    }
    Ok(())
}

fn read_zone(c: &mut Cursor<'_>, _protocol: u32) -> Result<SdkZone> {
    // Protocol ≤ 3: no segments / flags / per-zone modes.
    let name = c.string()?;
    let _ty = c.i32()?;
    let _leds_min = c.u32()?;
    let _leds_max = c.u32()?;
    let leds_count = c.u32()?;
    let matrix_len = c.u16()? as usize;
    if matrix_len > 0 {
        // height + width + height*width u32s; matrix_len is byte length of matrix blob
        c.skip(matrix_len)?;
    }
    Ok(SdkZone { name, leds_count })
}

struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn need(&self, n: usize) -> Result<()> {
        if self.pos + n > self.buf.len() {
            bail!(
                "OpenRGB SDK: truncated controller data at {} (need {n}, len {})",
                self.pos,
                self.buf.len()
            );
        }
        Ok(())
    }

    fn skip(&mut self, n: usize) -> Result<()> {
        self.need(n)?;
        self.pos += n;
        Ok(())
    }

    fn u16(&mut self) -> Result<u16> {
        self.need(2)?;
        let v = u16::from_le_bytes([self.buf[self.pos], self.buf[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    fn u32(&mut self) -> Result<u32> {
        self.need(4)?;
        let v = u32::from_le_bytes([
            self.buf[self.pos],
            self.buf[self.pos + 1],
            self.buf[self.pos + 2],
            self.buf[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    fn i32(&mut self) -> Result<i32> {
        Ok(self.u32()? as i32)
    }

    fn string(&mut self) -> Result<String> {
        let len = self.u16()? as usize;
        self.need(len)?;
        let raw = &self.buf[self.pos..self.pos + len];
        self.pos += len;
        let bytes = if raw.last() == Some(&0) {
            &raw[..raw.len().saturating_sub(1)]
        } else {
            raw
        };
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}

/// True if something accepts TCP on the OpenRGB SDK port.
pub fn sdk_port_open() -> bool {
    TcpStream::connect_timeout(
        &DEFAULT_ADDR.parse().unwrap(),
        Duration::from_millis(200),
    )
    .is_ok()
}
