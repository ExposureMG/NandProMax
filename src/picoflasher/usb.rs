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
		for p in &ports {
			if let serialport::SerialPortType::UsbPort(info) = &p.port_type {
				if info.vid == 0x2e8a
					|| info.vid == 0x600d
					|| info
						.product
						.as_deref()
						.unwrap_or("")
						.to_lowercase()
						.contains("pico")
				{
					return Ok(p.port_name.clone());
				}
			}
		}

		let acm_ports: Vec<_> = ports
			.iter()
			.filter(|p| {
				matches!(p.port_type, serialport::SerialPortType::UsbPort(_))
					|| p.port_name.contains("ttyACM")
					|| p.port_name.contains("ttyUSB")
					|| p.port_name.contains("usbmodem")
					|| p.port_name.contains("cu.")
			})
			.collect();

		if acm_ports.len() == 1 {
			Ok(acm_ports[0].port_name.clone())
		} else if acm_ports.is_empty() {
			anyhow::bail!("No USB serial ports (/dev/ttyACM* or /dev/ttyUSB*) found. Please connect your PicoFlasher.")
		} else {
			let names: Vec<String> = acm_ports.iter().map(|p| p.port_name.clone()).collect();
			anyhow::bail!(
				"Multiple USB serial ports found ({}); please specify one using --serial <PORT>",
				names.join(", ")
			)
		}
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

	pub fn read_with_ret(&mut self, cmd: u8, lba: u32, data_len: usize) -> Result<(u32, Option<Vec<u8>>)> {
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
