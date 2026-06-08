use std::time::Duration;

use anyhow::{bail, Context, Result};
use rusb::{Device, DeviceHandle, UsbContext};

/// DemoN device VID and PID
const DEMON_VID: u16 = 0x11d4;
const DEMON_PID: u16 = 0x444e;

/// DemoN USB client
pub struct UsbClient {
    handle: DeviceHandle<rusb::Context>,
    endpoint_in: u8,
    endpoint_out: u8,
    interface_detached: bool,
}

#[allow(dead_code)]
impl UsbClient {
    /// Open a connection to the DemoN USB device
    pub fn open() -> Result<Self> {
        let context = rusb::Context::new().context("create USB context")?;

        // Find the DemoN device
        let device = Self::find_device(&context)?;
        let mut handle = device.open().context("open device handle")?;

        // Get endpoints
        let (endpoint_in, endpoint_out) = Self::get_endpoints(&device)?;

        // Claim interface
        let interface_detached = Self::claim_interface(&mut handle)?;

        Ok(Self {
            handle,
            endpoint_in,
            endpoint_out,
            interface_detached,
        })
    }

    /// Find DemoN device in the list of connected USB devices
    fn find_device(context: &rusb::Context) -> Result<Device<rusb::Context>> {
        let devices = match context.devices() {
            Ok(d) => d,
            Err(e) => bail!("Failed to list USB devices: {e}"),
        };

        for device in devices.iter() {
            let device_desc = match device.device_descriptor() {
                Ok(d) => d,
                Err(_) => continue,
            };

            if device_desc.vendor_id() == DEMON_VID && device_desc.product_id() == DEMON_PID {
                return Ok(device);
            }
        }

        bail!(
            "DemoN device not found (VID=0x{:04x}, PID=0x{:04x})",
            DEMON_VID,
            DEMON_PID
        );
    }

    /// Get bulk endpoints for the device
    fn get_endpoints(device: &Device<rusb::Context>) -> Result<(u8, u8)> {
        let config_desc = device
            .config_descriptor(0)
            .context("get config descriptor")?;

        let mut ep_in = None;
        let mut ep_out = None;

        for interface in config_desc.interfaces() {
            //for altsetting in interface.alt_settings() {
            for if_descriptors in interface.descriptors() {
                for endpoint in if_descriptors.endpoint_descriptors() {
                    if endpoint.transfer_type() == rusb::TransferType::Bulk {
                        if endpoint.direction() == rusb::Direction::In {
                            ep_in = Some(endpoint.address());
                        } else if endpoint.direction() == rusb::Direction::Out {
                            ep_out = Some(endpoint.address());
                        }
                    }
                }
            }
            //}
        }

        match (ep_in, ep_out) {
            (Some(in_ep), Some(out_ep)) => Ok((in_ep, out_ep)),
            _ => bail!("No bulk endpoints found for DemoN device"),
        }
    }

    /// Claim the device interface
    fn claim_interface(handle: &mut DeviceHandle<rusb::Context>) -> Result<bool> {
        let mut interface_detached = false;

        // On Linux, we may need to detach the kernel driver
        #[cfg(target_os = "linux")]
        {
            let result = handle.claim_interface(0);
            if result.is_err() {
                handle
                    .detach_kernel_driver(0)
                    .context("detach kernel driver")?;
                interface_detached = true;
                handle
                    .claim_interface(0)
                    .context("claim interface after detaching kernel driver")?;
            }
        }

        #[cfg(not(target_os = "linux"))]
        {
            handle.claim_interface(0).context("claim interface")?;
        }

        Ok(interface_detached)
    }

    /// Write a single byte to the device
    pub fn write_byte(&mut self, byte: u8) -> Result<()> {
        let buf = [byte];
        self.write_bulk(&buf).context("write byte")?;
        Ok(())
    }

    /// Write two bytes to the device (little-endian)
    pub fn write_u16(&mut self, value: u16) -> Result<()> {
        let buf = value.to_le_bytes();
        self.write_bulk(&buf).context("write u16")?;
        Ok(())
    }

    /// Write data to the bulk OUT endpoint
    pub fn write_bulk(&mut self, data: &[u8]) -> Result<usize> {
        let timeout = Duration::from_millis(1000);
        match self.handle.write_bulk(self.endpoint_out, data, timeout) {
            Ok(len) => Ok(len),
            Err(e) => bail!("Bulk write failed: {e}"),
        }
    }

    /// Read a single byte from the device
    pub fn read_byte(&mut self) -> Result<u8> {
        let mut buf = [0u8; 1];
        self.read_bulk(&mut buf, 1, Duration::from_millis(1000))?;
        Ok(buf[0])
    }

    /// Read two bytes from the device (little-endian)
    pub fn read_u16(&mut self) -> Result<u16> {
        let mut buf = [0u8; 2];
        self.read_bulk(&mut buf, 2, Duration::from_millis(1000))?;
        Ok(u16::from_le_bytes(buf))
    }

    /// Read data from the bulk IN endpoint
    pub fn read_bulk(
        &mut self,
        buf: &mut [u8],
        expected_len: usize,
        timeout: Duration,
    ) -> Result<usize> {
        match self.handle.read_bulk(self.endpoint_in, buf, timeout) {
            Ok(len) => {
                if len < expected_len {
                    bail!("Expected to read {} bytes but got {}", expected_len, len);
                }
                Ok(len)
            }
            Err(e) => bail!("Bulk read failed: {e}"),
        }
    }

    /// Read variable-length data with timeout
    pub fn read_variable(&mut self, buf: &mut [u8], timeout_ms: u64) -> Result<usize> {
        let timeout = Duration::from_millis(timeout_ms);
        match self.handle.read_bulk(self.endpoint_in, buf, timeout) {
            Ok(len) => Ok(len),
            Err(e) if e == rusb::Error::Timeout => Ok(0),
            Err(e) => bail!("Variable read failed: {e}"),
        }
    }
}

impl Drop for UsbClient {
    fn drop(&mut self) {
        let _ = self.handle.release_interface(0);

        #[cfg(target_os = "linux")]
        {
            if self.interface_detached {
                let _ = self.handle.attach_kernel_driver(0);
            }
        }
    }
}
