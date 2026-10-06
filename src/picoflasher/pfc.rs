//! PicoFlasher command set. The transport is the USB CDC serial port; see
//! [`crate::picoflasher::usb::Client`].

pub use crate::picoflasher::usb::Client;

pub const CMD_GET_VERSION: u8 = 0x00;
pub const CMD_GET_FLASH_CONFIG: u8 = 0x01;
pub const CMD_READ_FLASH: u8 = 0x02;
pub const CMD_WRITE_FLASH: u8 = 0x03;
pub const CMD_READ_FLASH_STREAM: u8 = 0x04;
#[allow(dead_code)]
pub const CMD_ERASE_FLASH: u8 = 0x05;

pub const CMD_SET_SMC_WORKAROUND: u8 = 0x20;
pub const CMD_STOP_SMC: u8 = 0x21;
pub const CMD_START_SMC: u8 = 0x22;

pub const CMD_EMMC_DETECT: u8 = 0x50;
pub const CMD_EMMC_INIT: u8 = 0x51;
#[allow(dead_code)]
pub const CMD_EMMC_GET_CID: u8 = 0x52;
#[allow(dead_code)]
pub const CMD_EMMC_GET_CSD: u8 = 0x53;
pub const CMD_EMMC_GET_EXT_CSD: u8 = 0x54;
pub const CMD_EMMC_READ: u8 = 0x55;
pub const CMD_EMMC_READ_STREAM: u8 = 0x56;
pub const CMD_EMMC_WRITE: u8 = 0x57;

pub const NAND_BLOCK_BYTES: usize = 0x210;
pub const EMMC_BLOCK_BYTES: usize = 0x200;

pub fn cmd_payload(cmd: u8, lba: u32) -> [u8; 5] {
    let mut buf = [0u8; 5];
    buf[0] = cmd;
    buf[1..5].copy_from_slice(&lba.to_le_bytes());
    buf
}
