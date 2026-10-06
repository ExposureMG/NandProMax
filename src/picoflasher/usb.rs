use std::io::{Read, Write};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::picoflasher::pfc::cmd_payload;

pub struct Client {
    port: Box<dyn serialport::SerialPort>,
}

impl Client {
    pub fn detect_port() -> Result<String> {
        let ports = serialport::available_ports().context("failed to list serial ports")?;

        let known: Vec<String> = ports
            .iter()
            .filter(|p| match &p.port_type {
                serialport::SerialPortType::UsbPort(info) => {
                    info.vid == 0x2e8a
                        || info.vid == 0x600d
                        || info
                            .product
                            .as_deref()
                            .unwrap_or("")
                            .to_lowercase()
                            .contains("pico")
                }
                _ => false,
            })
            .map(|p| p.port_name.clone())
            .collect();
        if let Some(name) = drop_tty_duplicates(known).into_iter().next() {
            return Ok(name);
        }

        let listed: Vec<(String, bool)> = ports
            .iter()
            .map(|p| {
                (
                    p.port_name.clone(),
                    matches!(p.port_type, serialport::SerialPortType::UsbPort(_)),
                )
            })
            .collect();
        select_fallback_port(&listed)
    }

    pub fn open(path: &str, timeout: Duration) -> Result<Self> {
        let resolved_path = if path.is_empty() {
            Self::detect_port()?
        } else {
            path.to_string()
        };
        let port = serialport::new(&resolved_path, 115_200)
            .timeout(timeout)
            .open()
            .with_context(|| format!("open serial port {resolved_path}"))?;
        Ok(Self { port })
    }

    /// Open the serial port (auto-detected when `port` is empty) and return the
    /// client together with the port name that was used.
    pub fn connect(port: &str, timeout: Duration) -> Result<(Self, String)> {
        let resolved = if port.is_empty() {
            Self::detect_port()?
        } else {
            port.to_string()
        };
        let client = Self::open(&resolved, timeout)?;
        Ok((client, resolved))
    }

    pub fn send_cmd(&mut self, cmd: u8, lba: u32, extra: &[u8]) -> Result<()> {
        let mut payload = Vec::with_capacity(5 + extra.len());
        payload.extend_from_slice(&cmd_payload(cmd, lba));
        payload.extend_from_slice(extra);
        self.port.write_all(&payload).context("serial write")?;
        Ok(())
    }

    pub fn cmd_void(&mut self, cmd: u8, lba: u32) -> Result<()> {
        self.send_cmd(cmd, lba, &[])
    }

    pub fn cmd_u32(&mut self, cmd: u8, lba: u32) -> Result<u32> {
        self.send_cmd(cmd, lba, &[])?;
        self.read_u32()
    }

    pub fn cmd_u8(&mut self, cmd: u8, lba: u32) -> Result<u8> {
        self.send_cmd(cmd, lba, &[])?;
        let mut b = [0u8; 1];
        self.port.read_exact(&mut b).context("serial read u8")?;
        Ok(b[0])
    }

    pub fn cmd_exact_bytes(&mut self, cmd: u8, lba: u32, len: usize) -> Result<Vec<u8>> {
        self.send_cmd(cmd, lba, &[])?;
        let mut buf = vec![0u8; len];
        self.port
            .read_exact(&mut buf)
            .with_context(|| format!("serial read {len} bytes"))?;
        Ok(buf)
    }

    pub fn read_with_ret(
        &mut self,
        cmd: u8,
        lba: u32,
        data_len: usize,
    ) -> Result<(u32, Option<Vec<u8>>)> {
        self.send_cmd(cmd, lba, &[])?;
        let ret = self.read_u32()?;
        if ret != 0 {
            return Ok((ret, None));
        }
        let mut data = vec![0u8; data_len];
        self.port
            .read_exact(&mut data)
            .with_context(|| format!("serial read {data_len} bytes"))?;
        Ok((ret, Some(data)))
    }

    pub fn recv_stream_block(&mut self, data_len: usize) -> Result<(u32, Option<Vec<u8>>)> {
        let ret = self.read_u32()?;
        if ret != 0 {
            return Ok((ret, None));
        }
        let mut data = vec![0u8; data_len];
        self.port
            .read_exact(&mut data)
            .with_context(|| format!("serial read {data_len} bytes"))?;
        Ok((ret, Some(data)))
    }

    pub fn write_single(&mut self, cmd: u8, lba: u32, data: &[u8]) -> Result<u32> {
        self.send_cmd(cmd, lba, data)?;
        self.read_u32()
    }

    fn read_u32(&mut self) -> Result<u32> {
        let mut buf = [0u8; 4];
        self.port.read_exact(&mut buf).context("serial read u32")?;
        Ok(u32::from_le_bytes(buf))
    }
}

const NO_PORT_MSG: &str = "No USB serial port found (a COM port on Windows, /dev/cu.usbmodem* on macOS, /dev/ttyACM* on Linux). Please connect your PicoFlasher.";

/// macOS lists every USB serial device twice (/dev/cu.X and /dev/tty.X); keep only the cu.* one.
fn drop_tty_duplicates(names: Vec<String>) -> Vec<String> {
    let has = |n: &str| names.iter().any(|x| x == n);
    names
        .iter()
        .filter(|n| match n.strip_prefix("/dev/tty.") {
            Some(rest) => !has(&format!("/dev/cu.{rest}")),
            None => true,
        })
        .cloned()
        .collect()
}

/// Pick the only plausible USB serial port from `(name, is_usb)` pairs.
fn select_fallback_port(ports: &[(String, bool)]) -> Result<String> {
    let candidates: Vec<String> = ports
        .iter()
        .filter(|(name, is_usb)| {
            *is_usb
                || name.contains("ttyACM")
                || name.contains("ttyUSB")
                || name.contains("usbmodem")
        })
        .map(|(name, _)| name.clone())
        .collect();
    let mut candidates = drop_tty_duplicates(candidates);

    match candidates.len() {
        0 => anyhow::bail!("{NO_PORT_MSG}"),
        1 => Ok(candidates.remove(0)),
        _ => anyhow::bail!(
            "Multiple USB serial ports found ({}); please specify one using --serial <PORT>",
            candidates.join(", ")
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ports(list: &[(&str, bool)]) -> Vec<(String, bool)> {
        list.iter().map(|(n, u)| (n.to_string(), *u)).collect()
    }

    #[test]
    fn ignores_non_usb_cu_ports() {
        let p = ports(&[
            ("/dev/cu.Bluetooth-Incoming-Port", false),
            ("/dev/tty.Bluetooth-Incoming-Port", false),
        ]);
        assert!(select_fallback_port(&p).is_err());
    }

    #[test]
    fn prefers_cu_over_duplicate_tty() {
        let p = ports(&[
            ("/dev/tty.usbmodem1101", true),
            ("/dev/cu.usbmodem1101", true),
            ("/dev/cu.Bluetooth-Incoming-Port", false),
        ]);
        assert_eq!(select_fallback_port(&p).unwrap(), "/dev/cu.usbmodem1101");
    }

    #[test]
    fn linux_and_windows_names() {
        assert_eq!(
            select_fallback_port(&ports(&[("/dev/ttyACM0", false)])).unwrap(),
            "/dev/ttyACM0"
        );
        assert_eq!(
            select_fallback_port(&ports(&[("COM7", true), ("COM1", false)])).unwrap(),
            "COM7"
        );
    }

    #[test]
    fn multiple_distinct_ports_is_an_error() {
        let p = ports(&[("/dev/ttyACM0", true), ("/dev/ttyUSB0", true)]);
        let msg = select_fallback_port(&p).unwrap_err().to_string();
        assert!(msg.contains("Multiple") && msg.contains("ttyACM0") && msg.contains("ttyUSB0"));
    }

    #[test]
    fn empty_error_is_platform_neutral() {
        let msg = select_fallback_port(&[]).unwrap_err().to_string();
        assert!(msg.contains("COM") && msg.contains("usbmodem") && msg.contains("ttyACM"));
    }
}
