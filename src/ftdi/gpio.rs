use anyhow::{Context, Result};
use ftdi_embedded_hal as hal;
use hal::eh1::digital::OutputPin as _;
#[cfg(feature = "libftd2xx")]
mod backend {
	use super::*;
	use hal::libftd2xx;
	use hal::libftd2xx::{FtdiCommon as _, FtdiMpsse as _};
	use hal::ftdi_mpsse::{MpsseCmdExecutor, MpsseSettings};

	pub struct Device {
		pub inner: libftd2xx::Ftdi,
	}

	impl Device {
		pub fn with_description(desc: &str) -> Result<Self> {
			let mut inner = libftd2xx::Ftdi::with_description(desc)
				.with_context(|| format!("open device by description: {desc:?}"))?;
			let _ = inner.set_usb_parameters(65536);
			Ok(Self { inner })
		}

		pub fn with_index(index: i32) -> Result<Self> {
			let mut inner = libftd2xx::Ftdi::with_index(index).with_context(|| format!("open device index {index}"))?;
			let _ = inner.set_usb_parameters(65536);
			Ok(Self { inner })
		}
	}

	impl libftd2xx::FtdiCommon for Device {
		const DEVICE_TYPE: libftd2xx::DeviceType = libftd2xx::DeviceType::FT2232H;

		fn handle(&mut self) -> *mut std::ffi::c_void {
			self.inner.handle()
		}
	}

	impl libftd2xx::FtdiMpsse for Device {}

	impl MpsseCmdExecutor for Device {
		type Error = libftd2xx::TimeoutError;

		fn init(&mut self, settings: &MpsseSettings) -> Result<(), Self::Error> {
			self.initialize_mpsse(settings)
		}

		fn send(&mut self, data: &[u8]) -> Result<(), Self::Error> {
			self.write_all(data)
		}

		fn recv(&mut self, data: &mut [u8]) -> Result<(), Self::Error> {
			self.read_all(data)
		}
	}
}

#[cfg(all(feature = "libftdi", not(feature = "libftd2xx")))]
mod backend {
	use super::*;
	use hal::ftdi_mpsse::{MpsseCmdExecutor, MpsseSettings};

	pub struct Device {
		pub inner: hal::ftdi::Device,
	}

	impl Device {
		pub fn with_description(_desc: &str) -> Result<Self> {
			let inner = hal::ftdi::find_by_vid_pid(0x0403, 0x6010)
				.interface(hal::ftdi::Interface::A)
				.open()
				.map_err(|e| anyhow::anyhow!("open device by libftdi: {e:?}"))?;
			Ok(Self { inner })
		}

		pub fn with_index(_index: i32) -> Result<Self> {
			let inner = hal::ftdi::find_by_vid_pid(0x0403, 0x6010)
				.interface(hal::ftdi::Interface::A)
				.open()
				.map_err(|e| anyhow::anyhow!("open device by libftdi: {e:?}"))?;
			Ok(Self { inner })
		}
	}

	impl MpsseCmdExecutor for Device {
		type Error = std::io::Error;

		fn init(&mut self, settings: &MpsseSettings) -> Result<(), Self::Error> {
			self.inner.init(settings)
		}

		fn send(&mut self, data: &[u8]) -> Result<(), Self::Error> {
			self.inner.send(data)
		}

		fn recv(&mut self, data: &mut [u8]) -> Result<(), Self::Error> {
			self.inner.recv(data)
		}
	}

	impl std::ops::Deref for Device {
		type Target = hal::ftdi::Device;
		fn deref(&self) -> &Self::Target {
			&self.inner
		}
	}

	impl std::ops::DerefMut for Device {
		fn deref_mut(&mut self) -> &mut Self::Target {
			&mut self.inner
		}
	}
}

pub use backend::Device;

pub struct XboxPins {
	pub cs: hal::OutputPin<Device>,
	pub xx: hal::OutputPin<Device>,
	pub ej: hal::OutputPin<Device>,
}

impl XboxPins {
	pub fn new(hal: &hal::FtHal<Device>) -> Result<Self> {
		let mut cs = hal.ad3()?;
		let mut xx = hal.ad4()?;
		let mut ej = hal.ad5()?;

		cs.set_high()?;
		xx.set_low()?;
		ej.set_low()?;

		Ok(Self { cs, xx, ej })
	}

	pub fn set_cs(&mut self, high: bool) -> Result<()> {
		if high {
			self.cs.set_high()?;
		} else {
			self.cs.set_low()?;
		}
		Ok(())
	}

	pub fn set_gpio(&mut self, xx: bool, ej: bool) -> Result<()> {
		if xx {
			self.xx.set_high()?;
		} else {
			self.xx.set_low()?;
		}

		if ej {
			self.ej.set_high()?;
		} else {
			self.ej.set_low()?;
		}

		Ok(())
	}
}
