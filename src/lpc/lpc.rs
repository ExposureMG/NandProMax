use anyhow::{bail, Context, Result};
use std::time::Duration;

use crate::lpc::usb::UsbClient;

/// LPC/XFlash command codes (from XFlash.py)
#[allow(dead_code)]
#[repr(u8)]
pub enum Command {
    DataRead = 0x01,
    DataWrite = 0x02,
    DataInit = 0x03,
    DataDeinit = 0x04,
    DataStatus = 0x05,
    DataErase = 0x06,
    DataExec = 0x07,
    DevVersion = 0x08,
    XsvfExec = 0x09,
    XboxPowerOn = 0x10,
    XboxPowerOff = 0x11,
    DevUpdate = 0xF0,
}

/// Flash configuration (from XConfig.py)
#[derive(Debug, Clone)]
pub struct FlashConfig {
    pub raw: u32,
    pub controller_type: u8,
    pub block_type: u8,
    pub page_size: u32,
    pub meta_size: u32,
    pub meta_type: u8,
    pub block_size: u32,
    pub size_blocks: u32,
    pub size_small_blocks: u32,
    pub file_blocks: u32,
    pub blocks_per_little: u32,
}

impl FlashConfig {
    /// Parse flash config from raw u32 value
    pub fn parse(config: u32) -> Result<Self> {
        let controller_type = ((config >> 17) & 3) as u8;
        let block_type = ((config >> 4) & 3) as u8;

        let page_size = 0x200;
        let meta_size = 0x10;
        let mut meta_type = 0u8;
        let mut block_size = 0u32;
        let mut size_blocks = 0u32;
        let mut file_blocks = 0u32;

        match controller_type {
            0 => {
                meta_type = 0;
                block_size = 0x20;
                match block_type {
                    0 => bail!("nand type 0:0 is invalid"),
                    1 => {
                        size_blocks = 0x400;
                        file_blocks = 0x3E0;
                    }
                    2 => {
                        size_blocks = 0x800;
                        file_blocks = 0x7C0;
                    }
                    3 => {
                        size_blocks = 0x1000;
                        file_blocks = 0xF80;
                    }
                    _ => bail!("unknown block type {} for controller type 0", block_type),
                }
            }
            1 => {
                if block_type == 0 {
                    bail!("nand type 1:0 is invalid")
                }
                meta_type = 1;
                block_size = 0x20;
                if block_type == 1 {
                    size_blocks = 0x400;
                    file_blocks = 0x3E0;
                }
            }
            2 => {
                meta_type = 1;
                block_size = 0x20;
                if block_type == 1 {
                    size_blocks = 0x1000;
                    file_blocks = 0xF80;
                } else if block_type == 2 || block_type == 3 {
                    meta_type = 2;
                    if block_type == 2 {
                        block_size = 0x100;
                        size_blocks =
                            1 << (((config >> 19) & 3) + ((config >> 21) & 15) + 23) >> 17;
                        file_blocks = 0x1E0;
                    } else if block_type == 3 {
                        block_size = 0x200;
                        size_blocks =
                            1 << (((config >> 19) & 3) + ((config >> 21) & 15) + 23) >> 18;
                        file_blocks = 0xF0;
                    }
                }
            }
            _ => bail!("controller type {} is invalid", controller_type),
        }

        let sizesmallblocks = size_blocks * (block_size / 0x20);
        let blocksperlittle = block_size / 0x20;

        Ok(Self {
            raw: config,
            controller_type,
            block_type,
            page_size,
            meta_size,
            meta_type,
            block_size,
            size_blocks,
            size_small_blocks: sizesmallblocks,
            file_blocks,
            blocks_per_little: blocksperlittle,
        })
    }

    /// Get total file size in bytes
    pub fn file_size(&self) -> u64 {
        (self.size_small_blocks as u64) * (self.block_size as u64)
    }

    /// Get block size in bytes (including meta data)
    pub fn full_block_size(&self) -> u32 {
        self.page_size + self.meta_size
    }
}

/// Status codes (from XStatus.py)
#[allow(dead_code)]
pub mod status {
    pub const ILL_LOG: u32 = 0x800;
    pub const PIN_WP_N: u32 = 0x400;
    pub const PIN_BY_N: u32 = 0x200;
    pub const INT_CP: u32 = 0x100;
    pub const ADDR_ER: u32 = 0x080;
    pub const BB_ER: u32 = 0x040;
    pub const RNP_ER: u32 = 0x020;
    pub const ECC_ER: u32 = 0x01c;
    pub const WR_ER: u32 = 0x002;
    pub const BUSY: u32 = 0x001;

    pub const OK: u32 = PIN_BY_N;
    pub const ERROR: u32 = ILL_LOG | ADDR_ER | BB_ER | RNP_ER | ECC_ER | WR_ER;

    /// Check if status indicates an error
    pub fn is_error(status: u32) -> bool {
        (status & ERROR != 0) || (status & OK == 0)
    }

    /// Check if status has specific bit set
    pub fn has_bit(status: u32, bit: u32) -> bool {
        (status & bit) != 0
    }
}

/// LPC/XFlash client
pub struct LpcClient {
    usb: UsbClient,
    flash_config: Option<FlashConfig>,
    pub version: Option<u32>,
}

impl LpcClient {
    /// Open a new connection to the LPC/XFlash device
    pub fn open() -> Result<Self> {
        let usb = UsbClient::open().context("Failed to open LPC USB connection")?;
        Ok(Self {
            usb,
            flash_config: None,
            version: None,
        })
    }

    /// Initialize the device and get version
    pub fn init(&mut self) -> Result<()> {
        self.usb.device_reset()?;
        self.version = Some(self.device_version()?);
        Ok(())
    }

    /// Get device version
    pub fn device_version(&mut self) -> Result<u32> {
        self.usb
            .control_transfer(Command::DevVersion as u8, 0, 4, None)?;
        self.usb.read_u32()
    }

    /// Initialize flash access
    pub fn flash_init(&mut self) -> Result<&FlashConfig> {
        if self.flash_config.is_some() {
            return Ok(self.flash_config.as_ref().unwrap());
        }

        self.usb
            .control_transfer(Command::DataInit as u8, 0, 0, None)?;
        let config_raw = self.usb.read_u32()?;

        let config = FlashConfig::parse(config_raw).context("Failed to parse flash config")?;

        self.flash_config = Some(config);
        Ok(self.flash_config.as_ref().unwrap())
    }

    /// Deinitialize flash access
    pub fn flash_deinit(&mut self) -> Result<()> {
        self.usb
            .control_transfer(Command::DataDeinit as u8, 0, 0, None)?;
        self.flash_config = None;
        Ok(())
    }

    /// Get flash status
    pub fn flash_status(&mut self) -> Result<u32> {
        self.usb
            .control_transfer(Command::DataStatus as u8, 0, 0, None)?;
        self.usb.read_u32()
    }

    /// Erase a block
    pub fn flash_erase(&mut self, block: u32) -> Result<u32> {
        self.usb
            .control_transfer(Command::DataErase as u8, block, 0, None)?;

        // For version >= 3, send exec command
        if self.version.unwrap_or(0) >= 3 {
            self.usb
                .control_transfer(Command::DataExec as u8, block, 0, None)?;
        }

        self.flash_status()
    }

    /// Read a block (0x4200 bytes)
    pub fn flash_read(&mut self, block: u32) -> Result<(u32, Vec<u8>)> {
        self.usb
            .control_transfer(Command::DataRead as u8, block, 0x4200, None)?;

        let mut buf = vec![0u8; 0x4200];
        let len = self.usb.read_bulk(&mut buf, Duration::from_millis(5000))?;

        if len < 0x4200 {
            buf.truncate(len);
        }

        let status = self.flash_status()?;
        Ok((status, buf))
    }

    /// Write a block (0x4200 bytes)
    pub fn flash_write(&mut self, block: u32, data: &[u8]) -> Result<u32> {
        if data.len() < 0x4200 {
            bail!(
                "Data must be at least 0x4200 bytes ({} bytes provided)",
                data.len()
            );
        }

        self.usb
            .control_transfer(Command::DataWrite as u8, block, 0x4200, None)?;
        self.usb.write_bulk(data, Duration::from_millis(5000))?;

        // For version >= 3, send exec command
        if self.version.unwrap_or(0) >= 3 {
            self.usb
                .control_transfer(Command::DataExec as u8, block, 0, None)?;
        }

        self.flash_status()
    }

    /// XSVF initialization
    pub fn xsvf_init(&mut self) -> Result<u32> {
        self.usb.device_reset()?;
        // Call version multiple times as in original code
        let _ = self.device_version()?;
        let _ = self.device_version()?;
        self.device_version()
    }

    /// XSVF write data
    pub fn xsvf_write(&mut self, data: &[u8]) -> Result<()> {
        self.usb
            .control_transfer(Command::DataWrite as u8, 0, data.len() as u32, None)?;
        self.usb.write_bulk(data, Duration::from_millis(5000))?;
        Ok(())
    }

    /// XSVF execute
    pub fn xsvf_execute(&mut self) -> Result<u32> {
        self.usb
            .control_transfer(Command::XsvfExec as u8, 0, 0, Some(10000))?;
        self.flash_status()
    }

    /// Power on Xbox console
    pub fn power_on(&mut self) -> Result<()> {
        self.usb
            .control_transfer(Command::XboxPowerOn as u8, 0, 0, None)?;
        Ok(())
    }

    /// Power off Xbox console
    pub fn power_off(&mut self) -> Result<()> {
        self.usb
            .control_transfer(Command::XboxPowerOff as u8, 0, 0, None)?;
        Ok(())
    }

    /// Enter bootloader/update mode
    pub fn device_update(&mut self) -> Result<()> {
        // Try to send update command, may fail on some devices
        let _ = self
            .usb
            .control_transfer(Command::DevUpdate as u8, 0, 0, None);
        Ok(())
    }
}
