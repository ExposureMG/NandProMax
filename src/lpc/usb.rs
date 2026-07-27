use std::time::Duration;

use anyhow::{bail, Context, Result};
use rusb::{Device, DeviceHandle, UsbContext};

/// LPC/XFlash device VID and PID
const LPC_VID: u16 = 0xFFFF;
const LPC_PID: u16 = 0x0004;

/// Fixed endpoints for LPC device (from XFlash.py)
const LPC_ENDPOINT_OUT: u8 = 0x05;
const LPC_ENDPOINT_IN: u8 = 0x82;

/// LPC USB client
pub struct UsbClient {
    handle: DeviceHandle<rusb::Context>,
    endpoint_in: u8,
    endpoint_out: u8,
    interface_detached: bool,
}

impl UsbClient {
    /// Open a connection to the LPC/XFlash USB device
    pub fn open() -> Result<Self> {
        let context = rusb::Context::new().context("create USB context")?;

        // Find the LPC device
        let device = Self::find_device(&context)?;
        let mut handle = device.open().context("open device handle")?;

        // Use fixed endpoints for LPC device
        let endpoint_in = LPC_ENDPOINT_IN;
        let endpoint_out = LPC_ENDPOINT_OUT;

        // Claim interface
        let interface_detached = Self::claim_interface(&mut handle)?;

        Ok(Self {
            handle,
            endpoint_in,
            endpoint_out,
            interface_detached,
        })
    }

    /// Find LPC device in the list of connected USB devices
    fn find_device(context: &rusb::Context) -> Result<Device<rusb::Context>> {
        let devices = context.devices().context("list USB devices")?;

        for device in devices.iter() {
            let device_desc = match device.device_descriptor() {
                Ok(d) => d,
                Err(_) => continue,
            };

            if device_desc.vendor_id() == LPC_VID && device_desc.product_id() == LPC_PID {
                return Ok(device);
            }
        }

        bail!(
            "LPC/XFlash device not found (VID=0x{:04x}, PID=0x{:04x})",
            LPC_VID,
            LPC_PID
        );
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

    /// Send a control command to the device (from XFlash.py deviceCmd)
    /// CMD_DATA_READ = 0x01, CMD_DATA_WRITE = 0x02, etc.
    pub fn control_transfer(
        &mut self,
        cmd: u8,
        arg_a: u32,
        arg_b: u32,
        timeout_ms: Option<u64>,
    ) -> Result<()> {
        let mut buf = [0u8; 8];
        buf[..4].copy_from_slice(&arg_a.to_le_bytes());
        buf[4..].copy_from_slice(&arg_b.to_le_bytes());

        let timeout = timeout_ms
            .map(Duration::from_millis)
            .unwrap_or(Duration::from_millis(1000));

        let result =
            self.handle
                .write_control(rusb::RequestType::Vendor as u8, cmd, 0, 0, &buf, timeout);

        result.map_err(|e| anyhow::anyhow!("Control transfer failed: {e}"))?;
        Ok(())
    }

    /// Read data from the bulk IN endpoint
    pub fn read_bulk(&mut self, buf: &mut [u8], timeout: Duration) -> Result<usize> {
        match self.handle.read_bulk(self.endpoint_in, buf, timeout) {
            Ok(len) => Ok(len),
            Err(e) => bail!("Bulk read failed: {e}"),
        }
    }

    /// Write data to the bulk OUT endpoint
    pub fn write_bulk(&mut self, data: &[u8], timeout: Duration) -> Result<usize> {
        match self.handle.write_bulk(self.endpoint_out, data, timeout) {
            Ok(len) => Ok(len),
            Err(e) => bail!("Bulk write failed: {e}"),
        }
    }

    /// Read a u32 value from the device (used for status and version)
    pub fn read_u32(&mut self) -> Result<u32> {
        let mut buf = [0u8; 4];
        let len = self.read_bulk(&mut buf, Duration::from_millis(1000))?;
        if len != 4 {
            bail!("Expected to read 4 bytes but got {}", len);
        }
        Ok(u32::from_le_bytes(buf))
    }

    /// Device reset (from XFlash.py deviceReset)
    pub fn device_reset(&mut self) -> Result<()> {
        if let Err(e) = self.handle.reset() {
            // Ignore reset errors, device might not support it
            eprintln!("Device reset not supported: {e}");
        }
        self.handle
            .set_active_configuration(1)
            .context("set configuration after reset")?;
        Ok(())
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
