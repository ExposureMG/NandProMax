use std::ffi::CStr;
use std::io::{Read, Write};
use std::os::raw::c_char;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

mod demon;
mod flasher;
mod ftdi;
mod interface;
mod lpc;
mod picoflasher;
mod tcp;
mod types;

use crate::demon::DemonClient;
use crate::flasher::{run_read_nand, run_write_nand, NandFlasher};
use crate::types::{AdapterType, DeviceType, FtdiPageFormat, MediaType};
use crate::lpc::LpcClient;
use crate::picoflasher::pfc::{
    Client, CMD_EMMC_DETECT, CMD_EMMC_GET_EXT_CSD, CMD_EMMC_INIT, CMD_EMMC_READ,
    CMD_EMMC_WRITE, CMD_GET_FLASH_CONFIG, CMD_GET_VERSION, CMD_READ_FLASH, CMD_WRITE_FLASH,
    CMD_SET_SMC_WORKAROUND, CMD_START_SMC, CMD_STOP_SMC, EMMC_BLOCK_BYTES, NAND_BLOCK_BYTES,
};

#[repr(C)]
pub enum FtdiPageFormatC {
    Auto = 0,
    Small = 1,
    Big = 2,
}

#[repr(C)]
pub enum NandProDeviceC {
    Auto = 0,
    Picoflasher = 1,
    Ftdi = 2,
    Lpc = 3,
    Demon = 4,
}

#[repr(C)]
pub enum NandProAdapterC {
    Auto = 0,
    Usb = 1,
    Tcp = 2,
}

#[repr(C)]
pub enum NandProMediaC {
    Auto = 0,
    Spi = 1,
    Emmc = 2,
}

/// Unified C API entry point for reading NAND / eMMC flash from any hardware device
#[no_mangle]
pub unsafe extern "C" fn nandpromax_read_nand_c(
    out_path: *const c_char,
    start: u32,
    count: u32,
    count_has_val: bool,
    device: NandProDeviceC,
    adapter: NandProAdapterC,
    media: NandProMediaC,
    serial_or_addr: *const c_char,
    elapsed_secs_out: *mut f64,
) -> i32 {
    if out_path.is_null() {
        return -1;
    }

    let path_str = match CStr::from_ptr(out_path).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let out = PathBuf::from(path_str);

    let ep_str = if serial_or_addr.is_null() {
        None
    } else {
        CStr::from_ptr(serial_or_addr).to_str().ok()
    };

    let count_opt = if count_has_val { Some(count) } else { None };

    let dev_opt = match device {
        NandProDeviceC::Picoflasher => Some(DeviceType::Pico),
        NandProDeviceC::Ftdi => Some(DeviceType::Ftdi),
        NandProDeviceC::Lpc => Some(DeviceType::Lpc),
        NandProDeviceC::Demon => Some(DeviceType::Demon),
        NandProDeviceC::Auto => None,
    };

    let adapt_opt = match adapter {
        NandProAdapterC::Usb => Some(AdapterType::Usb),
        NandProAdapterC::Tcp => Some(AdapterType::Tcp),
        NandProAdapterC::Auto => None,
    };

    let media_opt = match media {
        NandProMediaC::Spi => Some(MediaType::Spi),
        NandProMediaC::Emmc => Some(MediaType::Emmc),
        NandProMediaC::Auto => None,
    };

    match unified_read_nand_impl(out, start, count_opt, dev_opt, adapt_opt, media_opt, ep_str) {
        Ok(duration) => {
            if !elapsed_secs_out.is_null() {
                *elapsed_secs_out = duration.as_secs_f64();
            }
            0
        }
        Err(_) => -2,
    }
}

/// Unified C API entry point for writing NAND / eMMC flash to any hardware device
#[no_mangle]
pub unsafe extern "C" fn nandpromax_write_nand_c(
    input_path: *const c_char,
    start: u32,
    count: u32,
    count_has_val: bool,
    device: NandProDeviceC,
    adapter: NandProAdapterC,
    media: NandProMediaC,
    serial_or_addr: *const c_char,
    erase: bool,
    verify: bool,
    elapsed_secs_out: *mut f64,
) -> i32 {
    if input_path.is_null() {
        return -1;
    }

    let path_str = match CStr::from_ptr(input_path).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let input = PathBuf::from(path_str);

    let ep_str = if serial_or_addr.is_null() {
        None
    } else {
        CStr::from_ptr(serial_or_addr).to_str().ok()
    };

    let count_opt = if count_has_val { Some(count) } else { None };

    let dev_opt = match device {
        NandProDeviceC::Picoflasher => Some(DeviceType::Pico),
        NandProDeviceC::Ftdi => Some(DeviceType::Ftdi),
        NandProDeviceC::Lpc => Some(DeviceType::Lpc),
        NandProDeviceC::Demon => Some(DeviceType::Demon),
        NandProDeviceC::Auto => None,
    };

    let adapt_opt = match adapter {
        NandProAdapterC::Usb => Some(AdapterType::Usb),
        NandProAdapterC::Tcp => Some(AdapterType::Tcp),
        NandProAdapterC::Auto => None,
    };

    let media_opt = match media {
        NandProMediaC::Spi => Some(MediaType::Spi),
        NandProMediaC::Emmc => Some(MediaType::Emmc),
        NandProMediaC::Auto => None,
    };

    match unified_write_nand_impl(
        input, start, count_opt, dev_opt, adapt_opt, media_opt, ep_str, erase, verify,
    ) {
        Ok(duration) => {
            if !elapsed_secs_out.is_null() {
                *elapsed_secs_out = duration.as_secs_f64();
            }
            0
        }
        Err(_) => -2,
    }
}

// Backwards-compatible FTDI-specific entry points
#[no_mangle]
pub unsafe extern "C" fn ftdi_read_nand_c(
    out_path: *const c_char,
    start: u32,
    count: u32,
    count_has_val: bool,
    page_format: FtdiPageFormatC,
    ftdi_desc: *const c_char,
    ftdi_index: i32,
    ftdi_index_has_val: bool,
    freq_hz: u32,
    elapsed_secs_out: *mut f64,
) -> i32 {
    nandpromax_read_nand_c(
        out_path,
        start,
        count,
        count_has_val,
        NandProDeviceC::Ftdi,
        NandProAdapterC::Usb,
        NandProMediaC::Spi,
        ftdi_desc,
        elapsed_secs_out,
    )
}

#[no_mangle]
pub unsafe extern "C" fn ftdi_write_nand_c(
    input_path: *const c_char,
    start: u32,
    count: u32,
    count_has_val: bool,
    page_format: FtdiPageFormatC,
    ftdi_desc: *const c_char,
    ftdi_index: i32,
    ftdi_index_has_val: bool,
    freq_hz: u32,
    erase: bool,
    verify: bool,
    elapsed_secs_out: *mut f64,
) -> i32 {
    nandpromax_write_nand_c(
        input_path,
        start,
        count,
        count_has_val,
        NandProDeviceC::Ftdi,
        NandProAdapterC::Usb,
        NandProMediaC::Spi,
        ftdi_desc,
        erase,
        verify,
        elapsed_secs_out,
    )
}

fn unified_read_nand_impl(
    out: PathBuf,
    start: u32,
    count: Option<u32>,
    device: Option<DeviceType>,
    adapter: Option<AdapterType>,
    media: Option<MediaType>,
    ep: Option<&str>,
) -> Result<Duration> {
    let dev = device.unwrap_or(DeviceType::Pico);
    let ad = adapter.unwrap_or(AdapterType::Usb);
    let med = media.unwrap_or(MediaType::Spi);

    let t0 = Instant::now();
    match (dev, med) {
        (DeviceType::Pico, MediaType::Spi) => {
            let timeout = Duration::from_secs(3);
            let (mut client, _) = if ad == AdapterType::Tcp {
                Client::connect_tcp(ep.unwrap_or("192.168.4.1:3232"), timeout)?
            } else {
                Client::connect_usb(ep.unwrap_or(""), timeout)?
            };
            let (flash_config, blocks_total) = prepare_nand_pfc(&mut client)?;
            let blocks = count.unwrap_or(blocks_total.saturating_sub(start));
            read_nand_pfc(&mut client, out, start, blocks)?;
        }
        (DeviceType::Pico, MediaType::Emmc) => {
            let timeout = Duration::from_secs(3);
            let (mut client, _) = if ad == AdapterType::Tcp {
                Client::connect_tcp(ep.unwrap_or("192.168.4.1:3232"), timeout)?
            } else {
                Client::connect_usb(ep.unwrap_or(""), timeout)?
            };
            let blocks_total = prepare_emmc_pfc(&mut client)?;
            let blocks = count.unwrap_or(blocks_total.saturating_sub(start));
            read_emmc_pfc(&mut client, out, start, blocks)?;
        }
        (DeviceType::Ftdi, _) => {
            let mut xspi = crate::ftdi::spi::XSpi::open(ep.unwrap_or("auto"), None, 6_000_000)?;
            xspi.enter_flash_mode()?;
            let flash_config = xspi.read_u32(0x00)?;
            let geom = crate::ftdi::spi::sfc_init(flash_config)?;
            let pages = count.unwrap_or(geom.pages_count_in_nand.saturating_sub(start));
            let f = std::fs::File::create(out)?;
            let mut w = std::io::BufWriter::new(f);
            let mut page_buf = [0u8; 0x210];
            for i in 0..pages {
                crate::ftdi::spi::xnand_read_page_raw(&mut xspi, start + i, &mut page_buf)?;
                w.write_all(&page_buf)?;
            }
        }
        (DeviceType::Lpc, _) => {
            let mut client = LpcClient::open().context("Failed to open LPC device")?;
            run_read_nand(&mut client, out, start, count)?;
        }
        (DeviceType::Demon, _) => {
            let mut client = DemonClient::open().context("Failed to open DemoN device")?;
            run_read_nand(&mut client, out, start, count)?;
        }
        (DeviceType::Jrp, _) => bail!("JR-Programmer not yet supported in FFI"),
        (DeviceType::Esp, _) => bail!("Use Pico+Tcp for ESPFlasher in FFI"),
    }
    Ok(t0.elapsed())
}

fn unified_write_nand_impl(
    input: PathBuf,
    start: u32,
    _count: Option<u32>,
    device: Option<DeviceType>,
    adapter: Option<AdapterType>,
    media: Option<MediaType>,
    ep: Option<&str>,
    _erase: bool,
    _verify: bool,
) -> Result<Duration> {
    let dev = device.unwrap_or(DeviceType::Pico);
    let ad = adapter.unwrap_or(AdapterType::Usb);
    let med = media.unwrap_or(MediaType::Spi);

    let t0 = Instant::now();
    match (dev, med) {
        (DeviceType::Pico, MediaType::Spi) => {
            let timeout = Duration::from_secs(3);
            let (mut client, _) = if ad == AdapterType::Tcp {
                Client::connect_tcp(ep.unwrap_or("192.168.4.1:3232"), timeout)?
            } else {
                Client::connect_usb(ep.unwrap_or(""), timeout)?
            };
            let _ = prepare_nand_pfc(&mut client)?;
            write_nand_pfc(&mut client, input, start)?;
        }
        (DeviceType::Pico, MediaType::Emmc) => {
            let timeout = Duration::from_secs(3);
            let (mut client, _) = if ad == AdapterType::Tcp {
                Client::connect_tcp(ep.unwrap_or("192.168.4.1:3232"), timeout)?
            } else {
                Client::connect_usb(ep.unwrap_or(""), timeout)?
            };
            let _ = prepare_emmc_pfc(&mut client)?;
            write_emmc_pfc(&mut client, input, start)?;
        }
        (DeviceType::Ftdi, _) => {
            bail!("FTDI write requires CLI interface");
        }
        (DeviceType::Lpc, _) => {
            let mut client = LpcClient::open().context("Failed to open LPC device")?;
            run_write_nand(&mut client, input, start)?;
        }
        (DeviceType::Demon, _) => {
            let mut client = DemonClient::open().context("Failed to open DemoN device")?;
            run_write_nand(&mut client, input, start)?;
        }
        (DeviceType::Jrp, _) => bail!("JR-Programmer not yet supported in FFI"),
        (DeviceType::Esp, _) => bail!("Use Pico+Tcp for ESPFlasher in FFI"),
    }
    Ok(t0.elapsed())
}

fn prepare_nand_pfc(client: &mut Client) -> Result<(u32, u32)> {
    let ver = client.cmd_u32(CMD_GET_VERSION, 0)?;
    let _ = client.cmd_u32(CMD_STOP_SMC, 0);
    let _ = client.cmd_u32(CMD_SET_SMC_WORKAROUND, 1);
    let flash_config = client.cmd_u32(CMD_GET_FLASH_CONFIG, 0)?;
    let blocks_total = match (flash_config >> 17) & 0x03 {
        0 => 1024,
        1 => 2048,
        2 => 4096,
        _ => 1024,
    };
    Ok((flash_config, blocks_total))
}

fn read_nand_pfc(client: &mut Client, out: PathBuf, start: u32, blocks: u32) -> Result<()> {
    let f = std::fs::File::create(out)?;
    let mut w = std::io::BufWriter::new(f);
    let end_block = start + blocks;
    let mut current_block = start;
    while current_block < end_block {
        let read_bytes = client.cmd_exact_bytes(CMD_READ_FLASH, current_block, NAND_BLOCK_BYTES)?;
        w.write_all(&read_bytes)?;
        current_block += 1;
    }
    let _ = client.cmd_u32(CMD_START_SMC, 0);
    Ok(())
}

fn write_nand_pfc(client: &mut Client, input: PathBuf, start: u32) -> Result<()> {
    let mut buf = vec![];
    std::fs::File::open(input)?.read_to_end(&mut buf)?;
    let blocks = (buf.len() / NAND_BLOCK_BYTES) as u32;
    for i in 0..blocks {
        let block = start + i;
        let off = (i as usize) * NAND_BLOCK_BYTES;
        let end = off + NAND_BLOCK_BYTES;
        client.write_single(CMD_WRITE_FLASH, block, &buf[off..end])?;
    }
    let _ = client.cmd_u32(CMD_START_SMC, 0);
    Ok(())
}

fn prepare_emmc_pfc(client: &mut Client) -> Result<u32> {
    let _ = client.cmd_u32(CMD_STOP_SMC, 0);
    let _ = client.cmd_u32(CMD_SET_SMC_WORKAROUND, 1);
    let _ = client.cmd_u32(CMD_EMMC_INIT, 0)?;
    let _ = client.cmd_u32(CMD_EMMC_DETECT, 0)?;
    let ext_csd = client.cmd_exact_bytes(CMD_EMMC_GET_EXT_CSD, 0, 512)?;
    let sec_count = u32::from_le_bytes(ext_csd[212..216].try_into().unwrap());
    Ok(sec_count)
}

fn read_emmc_pfc(client: &mut Client, out: PathBuf, start: u32, blocks: u32) -> Result<()> {
    let f = std::fs::File::create(out)?;
    let mut w = std::io::BufWriter::new(f);
    let end_lba = start + blocks;
    let mut current_lba = start;
    while current_lba < end_lba {
        let read_bytes = client.cmd_exact_bytes(CMD_EMMC_READ, current_lba, EMMC_BLOCK_BYTES)?;
        w.write_all(&read_bytes)?;
        current_lba += 1;
    }
    let _ = client.cmd_u32(CMD_START_SMC, 0);
    Ok(())
}

fn write_emmc_pfc(client: &mut Client, input: PathBuf, start: u32) -> Result<()> {
    let mut buf = vec![];
    std::fs::File::open(input)?.read_to_end(&mut buf)?;
    let blocks = (buf.len() / EMMC_BLOCK_BYTES) as u32;
    for i in 0..blocks {
        let lba = start + i;
        let off = (i as usize) * EMMC_BLOCK_BYTES;
        let end = off + EMMC_BLOCK_BYTES;
        client.write_single(CMD_EMMC_WRITE, lba, &buf[off..end])?;
    }
    let _ = client.cmd_u32(CMD_START_SMC, 0);
    Ok(())
}
