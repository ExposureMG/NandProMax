pub mod gpio;
pub mod spi;

use anyhow::{Context, Result};

pub struct FtdiDeviceSummary {
	pub index: usize,
	pub device_type: String,
	pub serial_number: String,
	pub description: String,
}

#[cfg(feature = "libftd2xx")]
pub fn list_devices() -> Result<Vec<FtdiDeviceSummary>> {
	use ftdi_embedded_hal::libftd2xx;
	let devs = libftd2xx::list_devices().context("FT_GetDeviceInfoList")?;
	Ok(devs
		.into_iter()
		.enumerate()
		.map(|(i, d)| FtdiDeviceSummary {
			index: i,
			device_type: format!("{:?}", d.device_type),
			serial_number: d.serial_number,
			description: d.description,
		})
		.collect())
}

#[cfg(all(feature = "libftdi", not(feature = "libftd2xx")))]
pub fn list_devices() -> Result<Vec<FtdiDeviceSummary>> {
	// Simple summary fallback for libftdi backend
	Ok(vec![FtdiDeviceSummary {
		index: 0,
		device_type: "libftdi".to_string(),
		serial_number: "unknown".to_string(),
		description: "FTDI Device via libftdi".to_string(),
	}])
}

