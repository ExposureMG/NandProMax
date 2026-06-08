/// All credits to [cOz] for demontool
use anyhow::{Context, Result};

use crate::demon::usb::UsbClient;

/// DemoN command codes
#[allow(dead_code)]
#[repr(u8)]
pub enum Command {
    GetMode = 0x00,
    GetProtocolVersion = 0x01,
    GetDeviceId = 0x02,
    GetFirmwareVersion = 0x03,
    RunBootloader = 0x04,
    GetExtFlash = 0x05,
    SetExtFlash = 0x06,
    AcquireExtFlash = 0x07,
    ReleaseExtFlash = 0x08,
    GetExtFlashId = 0x09,
    GetInvalidBlocks = 0x0A,
    EraseExtFlashBlock = 0x0B,
    EraseAllExtFlashBlocks = 0x0C,
    ReadExtFlashBlock = 0x0D,
    ProgramExtFlashBlock = 0x0E,
    AssertSbReset = 0x0F,
    DeassertSbReset = 0x10,
    ReadSerialPort = 0x11,
    WriteSerialPort = 0x12,
    ExecXsvf = 0x13,
    PowerOn = 0x14,
    PowerOff = 0x15,
    // Bootloader commands
    BtlLeave = 0x81,
    Btl82 = 0x82,
    BtlLock = 0x83,
    BtlUnlock = 0x84,
    BtlReadPage = 0x85,
    BtlErasePage = 0x86,
    BtlWritePage = 0x87,
}

/// Device types
#[derive(Debug, Clone, Copy)]
pub enum DeviceType {
    Fat16 = 0,
    Slim16 = 1,
}

impl From<u16> for DeviceType {
    fn from(value: u16) -> Self {
        match value {
            0 => DeviceType::Fat16,
            1 => DeviceType::Slim16,
            _ => DeviceType::Fat16,
        }
    }
}

/// Device mode
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DeviceMode {
    Bootloader = 0,
    Firmware = 1,
}

impl From<u8> for DeviceMode {
    fn from(value: u8) -> Self {
        match value {
            0 => DeviceMode::Bootloader,
            1 => DeviceMode::Firmware,
            _ => DeviceMode::Bootloader,
        }
    }
}

/// NAND location
#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum FlashSelection {
    Internal = 0,
    External = 1,
}

impl From<u8> for FlashSelection {
    fn from(value: u8) -> Self {
        match value {
            0 => FlashSelection::Internal,
            1 => FlashSelection::External,
            _ => panic!("Invalid flash selection"),
        }
    }
}

/// NAND Flash Manufacturer info
#[derive(Debug, Clone)]
pub struct NandManufacturer {
    pub id: u8,
    pub name: &'static str,
}

/// NAND Flash Device info
#[derive(Debug, Clone)]
pub struct NandDevice {
    pub id: u8,
    pub name: &'static str,
    pub page_size: u32,
    pub spare_size: u32,
    pub chip_size: u32, // in MiB
    pub pages_per_block: u32,
    #[allow(dead_code)]
    pub big_block: bool,
}

impl NandDevice {
    /// Calculate user block size (page_size * pages_per_block)
    pub fn user_block_size(&self) -> u64 {
        (self.page_size as u64) * (self.pages_per_block as u64)
    }

    /// Calculate total block size (page_size + spare_size) * pages_per_block
    pub fn total_block_size(&self) -> u64 {
        (self.page_size + self.spare_size) as u64 * (self.pages_per_block as u64)
    }

    /// Calculate total number of blocks
    pub fn num_blocks(&self) -> u64 {
        let total_bytes = (self.chip_size as u64) * 1024 * 1024;
        total_bytes / self.user_block_size()
    }

    /// Calculate total file size for a dump
    pub fn file_size(&self) -> u64 {
        self.num_blocks() * self.total_block_size()
    }
}

/// DemoN device information
#[derive(Debug)]
pub struct DeviceInfo {
    pub device_id: DeviceType,
    pub protocol_version: u16,
    pub firmware_version: u16,
    pub nand_id: u16,
    pub mode: DeviceMode,
}

/// DemoN NAND client
pub struct DemonClient {
    usb: UsbClient,
    info: Option<DeviceInfo>,
}

#[allow(dead_code)]
impl DemonClient {
    /// Open a new connection to the DemoN device
    pub fn open() -> Result<Self> {
        let usb = UsbClient::open().context("Failed to open USB connection")?;
        Ok(Self { usb, info: None })
    }

    /// Initialize the device and get device info
    pub fn init(&mut self) -> Result<&DeviceInfo> {
        if self.info.is_some() {
            return Ok(self.info.as_ref().unwrap());
        }

        let mode = self.get_mode()?;
        if mode == DeviceMode::Bootloader {
            eprintln!("DemoN is in bootloader mode. Attempting to exit...");
            self.leave_bootloader_mode()?;
            // Wait for re-enumeration
            std::thread::sleep(std::time::Duration::from_secs(1));
            // Reconnect
            self.usb = UsbClient::open().context("Failed to reconnect after bootloader exit")?;
        }

        let device_id = self.get_device_id()?.into();
        let protocol_version = self.get_protocol_version()?;
        let firmware_version = self.get_firmware_version()?;
        let nand_id = self.read_flash_id()?;

        self.info = Some(DeviceInfo {
            device_id,
            protocol_version,
            firmware_version,
            nand_id,
            mode: DeviceMode::Firmware,
        });

        Ok(self.info.as_ref().unwrap())
    }

    /// Get current mode
    pub fn get_mode(&mut self) -> Result<DeviceMode> {
        self.usb.write_byte(Command::GetMode as u8)?;
        let mode = self.usb.read_byte()?;
        Ok(mode.into())
    }

    /// Get protocol version
    pub fn get_protocol_version(&mut self) -> Result<u16> {
        self.usb.write_byte(Command::GetProtocolVersion as u8)?;
        self.usb.read_u16()
    }

    /// Get device ID
    pub fn get_device_id(&mut self) -> Result<u16> {
        self.usb.write_byte(Command::GetDeviceId as u8)?;
        self.usb.read_u16()
    }

    /// Get firmware version
    pub fn get_firmware_version(&mut self) -> Result<u16> {
        self.usb.write_byte(Command::GetFirmwareVersion as u8)?;
        self.usb.read_u16()
    }

    /// Get current flash selection
    pub fn get_ext_flash(&mut self) -> Result<FlashSelection> {
        self.usb.write_byte(Command::GetExtFlash as u8)?;
        let loc = self.usb.read_byte()?;
        Ok(FlashSelection::from(loc))
    }

    /// Set flash selection
    pub fn set_ext_flash(&mut self, selection: FlashSelection) -> Result<()> {
        self.usb.write_byte(Command::SetExtFlash as u8)?;
        self.usb.write_byte(selection as u8)?;
        Ok(())
    }

    /// Get flash ID
    pub fn read_flash_id(&mut self) -> Result<u16> {
        self.acquire_flash()?;
        self.usb.write_byte(Command::GetExtFlashId as u8)?;
        let id = self.usb.read_u16()?;
        self.release_flash()?;
        Ok(id)
    }

    /// Acquire flash access
    pub fn acquire_flash(&mut self) -> Result<()> {
        self.usb.write_byte(Command::AssertSbReset as u8)?;
        self.usb.write_byte(Command::AcquireExtFlash as u8)?;
        Ok(())
    }

    /// Release flash access
    pub fn release_flash(&mut self) -> Result<()> {
        self.usb.write_byte(Command::ReleaseExtFlash as u8)?;
        self.usb.write_byte(Command::DeassertSbReset as u8)?;
        Ok(())
    }

    /// Read a block from NAND
    pub fn read_block(&mut self, page_number: u16, length: usize, buf: &mut [u8]) -> Result<usize> {
        self.usb.write_byte(Command::ReadExtFlashBlock as u8)?;
        self.usb.write_u16(page_number)?;
        let len = self
            .usb
            .read_bulk(buf, length, std::time::Duration::from_millis(5000))?;
        Ok(len)
    }

    /// Write a block to NAND
    pub fn write_block(&mut self, block: u16, data: &[u8]) -> Result<()> {
        self.usb.write_byte(Command::ProgramExtFlashBlock as u8)?;
        self.usb.write_u16(block)?;
        self.usb.write_bulk(data)?;
        Ok(())
    }

    /// Erase a block
    pub fn erase_block(&mut self, block: u16) -> Result<()> {
        self.usb.write_byte(Command::EraseExtFlashBlock as u8)?;
        self.usb.write_u16(block)?;
        Ok(())
    }

    /// Erase all blocks
    pub fn erase_all_blocks(&mut self) -> Result<()> {
        self.usb.write_byte(Command::EraseAllExtFlashBlocks as u8)?;
        Ok(())
    }

    /// Get invalid/block blocks list
    pub fn get_invalid_blocks(&mut self) -> Result<Vec<u16>> {
        self.acquire_flash()?;
        self.usb.write_byte(Command::GetInvalidBlocks as u8)?;

        let mut buf = [0u8; 2];
        let len = self.usb.read_variable(&mut buf, 500)?;

        if len == 0 {
            return Ok(Vec::new());
        }

        // The first byte contains the count
        let count = buf[0] as usize;
        let mut blocks = Vec::with_capacity(count);

        // Read the block list
        let mut block_buf = vec![0u8; count * 2];
        let _ = self.usb.read_variable(&mut block_buf, 500)?;

        for i in 0..count {
            let block = u16::from_le_bytes([block_buf[i * 2], block_buf[i * 2 + 1]]);
            blocks.push(block);
        }

        self.release_flash()?;
        Ok(blocks)
    }

    /// Power on the console
    pub fn power_on(&mut self) -> Result<()> {
        self.usb.write_byte(Command::PowerOn as u8)?;
        Ok(())
    }

    /// Power off the console
    pub fn power_off(&mut self) -> Result<()> {
        self.usb.write_byte(Command::PowerOff as u8)?;
        Ok(())
    }

    /// Enter bootloader mode
    pub fn enter_bootloader_mode(&mut self) -> Result<()> {
        self.usb.write_byte(Command::RunBootloader as u8)?;
        Ok(())
    }

    /// Leave bootloader mode
    pub fn leave_bootloader_mode(&mut self) -> Result<()> {
        self.usb.write_byte(Command::BtlLeave as u8)?;
        Ok(())
    }

    /// Lock for firmware update
    pub fn lock_for_update(&mut self) -> Result<()> {
        self.usb.write_byte(Command::BtlLock as u8)?;
        Ok(())
    }

    /// Unlock for firmware update
    pub fn unlock_for_update(&mut self) -> Result<()> {
        self.usb.write_byte(Command::BtlUnlock as u8)?;
        Ok(())
    }

    /// Read firmware data page (256 bytes)
    pub fn read_fw_page(&mut self, page: u8, buf: &mut [u8]) -> Result<usize> {
        self.usb.write_byte(Command::BtlReadPage as u8)?;
        self.usb.write_byte(page)?;
        let len = self
            .usb
            .read_bulk(buf, 256, std::time::Duration::from_millis(1000))?;
        Ok(len)
    }

    /// Write firmware data page
    pub fn write_fw_page(&mut self, page: u8, data: &[u8]) -> Result<()> {
        self.usb.write_byte(Command::BtlErasePage as u8)?;
        self.usb.write_byte(page)?;
        self.usb.write_byte(Command::BtlWritePage as u8)?;
        self.usb.write_byte(page)?;
        self.usb.write_bulk(data)?;
        Ok(())
    }

    /// Get NAND device info based on flash ID
    pub fn get_nand_info(&self) -> Option<&'static NandDevice> {
        if let Some(info) = &self.info {
            get_nand_device_by_id(info.nand_id)
        } else {
            None
        }
    }

    /// Get manufacturer name based on flash ID
    pub fn get_manufacturer_name(&self) -> Option<&'static str> {
        if let Some(info) = &self.info {
            let manu_id = ((info.nand_id >> 8) & 0xFF) as u8;
            get_manufacturer_by_id(manu_id)
        } else {
            None
        }
    }

    /// Get the raw page size for the current NAND device
    pub fn get_page_size(&self) -> Result<u32> {
        let nand_info = self
            .get_nand_info()
            .ok_or_else(|| anyhow::anyhow!("NAND device not recognized"))?;
        Ok(nand_info.page_size)
    }

    /// Get the spare size for the current NAND device
    pub fn get_spare_size(&self) -> Result<u32> {
        let nand_info = self
            .get_nand_info()
            .ok_or_else(|| anyhow::anyhow!("NAND device not recognized"))?;
        Ok(nand_info.spare_size)
    }

    /// Get the total page size (data + spare) for the current NAND device
    pub fn get_total_page_size(&self) -> Result<u32> {
        let nand_info = self
            .get_nand_info()
            .ok_or_else(|| anyhow::anyhow!("NAND device not recognized"))?;
        Ok(nand_info.page_size + nand_info.spare_size)
    }
}

/// List of NAND manufacturers
const MANUFACTURERS: &[NandManufacturer] = &[
    NandManufacturer {
        id: 0x98,
        name: "Toshiba",
    },
    NandManufacturer {
        id: 0xec,
        name: "Samsung",
    },
    NandManufacturer {
        id: 0x04,
        name: "Fujitsu",
    },
    NandManufacturer {
        id: 0x8f,
        name: "National",
    },
    NandManufacturer {
        id: 0x07,
        name: "Renesas",
    },
    NandManufacturer {
        id: 0x20,
        name: "ST Micro",
    },
    NandManufacturer {
        id: 0xad,
        name: "Hynix",
    },
    NandManufacturer {
        id: 0x2c,
        name: "Micron",
    },
    NandManufacturer {
        id: 0x00,
        name: "Unknown",
    },
];

/// List of NAND devices
const NAND_DEVICES: &[NandDevice] = &[
    // Small block chips
    NandDevice {
        id: 0xd6,
        name: "8MiB",
        page_size: 512,
        spare_size: 16,
        chip_size: 8,
        pages_per_block: 16,
        big_block: false,
    },
    NandDevice {
        id: 0xe6,
        name: "8MiB",
        page_size: 512,
        spare_size: 16,
        chip_size: 8,
        pages_per_block: 16,
        big_block: false,
    },
    NandDevice {
        id: 0x73,
        name: "16MiB",
        page_size: 512,
        spare_size: 16,
        chip_size: 16,
        pages_per_block: 32,
        big_block: false,
    },
    NandDevice {
        id: 0x75,
        name: "32MiB",
        page_size: 512,
        spare_size: 16,
        chip_size: 32,
        pages_per_block: 32,
        big_block: false,
    },
    NandDevice {
        id: 0x76,
        name: "64MiB",
        page_size: 512,
        spare_size: 16,
        chip_size: 64,
        pages_per_block: 32,
        big_block: false,
    },
    NandDevice {
        id: 0x79,
        name: "128MiB",
        page_size: 512,
        spare_size: 16,
        chip_size: 128,
        pages_per_block: 32,
        big_block: false,
    },
    NandDevice {
        id: 0x71,
        name: "256MiB",
        page_size: 512,
        spare_size: 16,
        chip_size: 256,
        pages_per_block: 32,
        big_block: false,
    },
    NandDevice {
        id: 0x73,
        name: "256MiB",
        page_size: 512,
        spare_size: 16,
        chip_size: 256,
        pages_per_block: 32,
        big_block: false,
    },
    // Big block chips
    NandDevice {
        id: 0xF2,
        name: "64MiB",
        page_size: 512,
        spare_size: 16,
        chip_size: 64,
        pages_per_block: 32,
        big_block: true,
    },
    NandDevice {
        id: 0xDA,
        name: "256MiB",
        page_size: 2048,
        spare_size: 64,
        chip_size: 256,
        pages_per_block: 64,
        big_block: true,
    },
    NandDevice {
        id: 0xDC,
        name: "512MiB",
        page_size: 2048,
        spare_size: 64,
        chip_size: 512,
        pages_per_block: 64,
        big_block: true,
    },
    NandDevice {
        id: 0xD7,
        name: "4096MiB",
        page_size: 8192,
        spare_size: 448,
        chip_size: 4096,
        pages_per_block: 256,
        big_block: false,
    },
];

/// Get manufacturer by ID
pub fn get_manufacturer_by_id(id: u8) -> Option<&'static str> {
    MANUFACTURERS.iter().find(|m| m.id == id).map(|m| m.name)
}

/// Get NAND device by flash ID (device byte only)
pub fn get_nand_device_by_id(flash_id: u16) -> Option<&'static NandDevice> {
    let device_id = (flash_id & 0xFF) as u8;
    NAND_DEVICES.iter().find(|d| d.id == device_id)
}
