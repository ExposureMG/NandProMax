mod demon;
mod ftdi;
mod interface;
mod picoflasher;

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Parser;

use crate::demon::DemonClient;
use crate::interface::cli::{Cli, Command};
use crate::picoflasher::pfc::{
    Client, CMD_EMMC_DETECT, CMD_EMMC_GET_EXT_CSD, CMD_EMMC_INIT, CMD_EMMC_READ,
    CMD_EMMC_READ_STREAM, CMD_EMMC_WRITE, CMD_EMMC_WRITE_MULTI, CMD_GET_FLASH_CONFIG,
    CMD_GET_VERSION, CMD_READ_FLASH, CMD_READ_FLASH_STREAM, CMD_SET_SMC_WORKAROUND, CMD_START_SMC,
    CMD_STOP_SMC, CMD_WRITE_FLASH, CMD_WRITE_FLASH_MULTI, EMMC_BLOCK_BYTES, NAND_BLOCK_BYTES,
};

use crate::lpc::LpcClient;

fn main() -> Result<()> {
    let cli = Cli::parse();
    let timeout = Duration::from_millis(cli.timeout_ms);
    match cli.command {
        Command::FtdiList => {
            ftdi_list()?;
            println!("ok");
        }
        Command::DemonInfo => {
            demon_info()?;
            println!("ok");
        }
        Command::DemonList => {
            demon_list()?;
            println!("ok");
        }
        Command::DemonReadNand { out, start, count } => {
            let elapsed = demon_read_nand(out, start, count)?;
            println!("ok ({:.3}s)", elapsed.as_secs_f64());
        }
        Command::DemonWriteNand { input, start } => {
            demon_write_nand(input, start)?;
        // LPC/XFlash commands
        Command::LpcInfo => {
            lpc_info()?;
            println!("ok");
        }
        Command::LpcList => {
            lpc_list()?;
            println!("ok");
        }
        Command::LpcReadNand { out, start, count } => {
            let elapsed = lpc_read_nand(out, start, count)?;
            println!("ok ({:.3}s)", elapsed.as_secs_f64());
        }
        Command::LpcWriteNand { input, start } => {
            lpc_write_nand(input, start)?;
            println!("ok");
        }
        Command::FtdiReadNand {
            out,
            start,
            count,
            ftdi_desc,
            ftdi_index,
            freq_hz,
        } => {
            let elapsed = ftdi_read_nand(out, start, count, &ftdi_desc, ftdi_index, freq_hz)?;
            println!("ok ({:.3}s)", elapsed.as_secs_f64());
        }
        Command::FtdiWriteNand {
            input,
            start,
            count,
            ftdi_desc,
            ftdi_index,
            freq_hz,
        } => {
            let elapsed = ftdi_write_nand(input, start, count, &ftdi_desc, ftdi_index, freq_hz)?;
            println!("ok ({:.3}s)", elapsed.as_secs_f64());
        }
        Command::ReadNand { out, start, count } => {
            let (mut client, resolved) = if let Some(port) = &cli.serial {
                Client::connect_usb(port, timeout)
                    .with_context(|| format!("failed to open serial {port}"))?
            } else {
                Client::connect_tcp(&cli.addr, timeout)
                    .with_context(|| format!("failed to connect to {}", cli.addr))?
            };
            eprintln!("connected to {resolved}");

            let (flash_config, blocks_total) = prepare_nand(&mut client)?;
            let blocks = count.unwrap_or(blocks_total.saturating_sub(start));
            eprintln!("flash_config=0x{flash_config:08x} blocks={blocks} start={start}");
            read_nand(&mut client, out, start, blocks)?;
            println!("ok");
        }
        Command::WriteNand { input, start } => {
            let (mut client, resolved) = if let Some(port) = &cli.serial {
                Client::connect_usb(port, timeout)
                    .with_context(|| format!("failed to open serial {port}"))?
            } else {
                Client::connect_tcp(&cli.addr, timeout)
                    .with_context(|| format!("failed to connect to {}", cli.addr))?
            };
            eprintln!("connected to {resolved}");

            let (flash_config, blocks_total) = prepare_nand(&mut client)?;
            eprintln!("flash_config=0x{flash_config:08x} start={start} max_blocks={blocks_total}");
            write_nand(&mut client, input, start)?;
            println!("ok");
        }
        Command::ReadEmmc { out, start, count } => {
            let (mut client, resolved) = if let Some(port) = &cli.serial {
                Client::connect_usb(port, timeout)
                    .with_context(|| format!("failed to open serial {port}"))?
            } else {
                Client::connect_tcp(&cli.addr, timeout)
                    .with_context(|| format!("failed to connect to {}", cli.addr))?
            };
            eprintln!("connected to {resolved}");

            let blocks_total = prepare_emmc(&mut client)?;
            let blocks = count.unwrap_or(blocks_total.saturating_sub(start));
            eprintln!("emmc_blocks={blocks} start={start}");
            read_emmc(&mut client, out, start, blocks)?;
            println!("ok");
        }
        Command::WriteEmmc { input, start } => {
            let (mut client, resolved) = if let Some(port) = &cli.serial {
                Client::connect_usb(port, timeout)
                    .with_context(|| format!("failed to open serial {port}"))?
            } else {
                Client::connect_tcp(&cli.addr, timeout)
                    .with_context(|| format!("failed to connect to {}", cli.addr))?
            };
            eprintln!("connected to {resolved}");

            let blocks_total = prepare_emmc(&mut client)?;
            eprintln!("start={start} max_blocks={blocks_total}");
            write_emmc(&mut client, input, start)?;
            println!("ok");
        }
    }

    Ok(())
}

fn ftdi_read_nand(
    out: std::path::PathBuf,
    start: u32,
    count: Option<u32>,
    ftdi_desc: &str,
    ftdi_index: Option<i32>,
    freq_hz: u32,
) -> Result<Duration> {
    use crate::ftdi::spi::{sfc_init, xnand_clear_status, xnand_read_page_raw, XSpi};

    eprintln!("ftdi freq_hz={freq_hz}");
    let mut xspi = XSpi::open(ftdi_desc, ftdi_index, freq_hz)?;
    xspi.enter_flash_mode()?;

    let flash_config = xspi.read_u32(0x00)?;
    let geom = sfc_init(flash_config)?;
    let total_pages = geom.pages_count_in_nand;
    let pages = count.unwrap_or(total_pages.saturating_sub(start));

    eprintln!(
        "flash_config=0x{flash_config:08x} nand={}MB start={} pages={}",
        geom.nand_size_mb, start, pages
    );

    let f = File::create(out).context("open output")?;
    let mut f = BufWriter::with_capacity(1024 * 1024, f);

    let t0 = Instant::now();
    let mut page_buf = [0u8; 0x210];
    for i in 0..pages {
        if (i & 0xFF) == 0 {
            xnand_clear_status(&mut xspi).context("clear status")?;
        }
        let page = start + i;
        xnand_read_page_raw(&mut xspi, page, &mut page_buf)
            .with_context(|| format!("read page {page}"))?;
        f.write_all(&page_buf)?;

        if (i & 0xFF) == 0 {
            eprintln!("read {}/{} pages", i + 1, pages);
        }
    }

    f.flush().context("flush output")?;
    Ok(t0.elapsed())
}

fn ftdi_write_nand(
    input: std::path::PathBuf,
    start: u32,
    count: Option<u32>,
    ftdi_desc: &str,
    ftdi_index: Option<i32>,
    freq_hz: u32,
) -> Result<Duration> {
    use crate::ftdi::spi::{sfc_init, xnand_clear_status, xnand_write_page_raw, XSpi};

    let input_meta = std::fs::metadata(&input).context("stat input")?;
    let input_len = input_meta.len() as usize;
    if input_len % 0x210 != 0 {
        bail!("input size must be a multiple of 0x210 bytes (raw page size)");
    }

    let file_pages = (input_len / 0x210) as u32;
    let pages = count.unwrap_or(file_pages);
    if pages > file_pages {
        bail!("input has {file_pages} pages but --count={pages} requested");
    }

    eprintln!("ftdi freq_hz={freq_hz}");
    let mut xspi = XSpi::open(ftdi_desc, ftdi_index, freq_hz)?;
    xspi.enter_flash_mode()?;

    let flash_config = xspi.read_u32(0x00)?;
    let geom = sfc_init(flash_config)?;
    let total_pages = geom.pages_count_in_nand;
    if start >= total_pages {
        bail!("start page {start} out of range (total pages {total_pages})");
    }
    if start + pages > total_pages {
        bail!(
            "requested range {}..{} out of range (total pages {total_pages})",
            start,
            start + pages
        );
    }

    eprintln!(
        "flash_config=0x{flash_config:08x} nand={}MB start={} pages={} (input_pages={file_pages})",
        geom.nand_size_mb, start, pages
    );

    let f = File::open(input).context("open input")?;
    let mut f = BufReader::with_capacity(1024 * 1024, f);

    let t0 = Instant::now();
    let mut page_buf = [0u8; 0x210];
    for i in 0..pages {
        if (i & 0xFF) == 0 {
            xnand_clear_status(&mut xspi).context("clear status")?;
        }

        f.read_exact(&mut page_buf).context("read input page")?;
        let page = start + i;
        xnand_write_page_raw(&mut xspi, page, &page_buf)
            .with_context(|| format!("write page {page}"))?;

        if (i & 0xFF) == 0 {
            eprintln!("wrote {}/{} pages", i + 1, pages);
        }
    }

    Ok(t0.elapsed())
}

fn ftdi_list() -> Result<()> {
    use ftdi_embedded_hal::libftd2xx;

    let n = libftd2xx::num_devices().context("FT_ListDevices(NUMBER_ONLY)")?;
    eprintln!("libftd2xx num_devices={n}");
    let devs = libftd2xx::list_devices().context("FT_GetDeviceInfoList")?;
    eprintln!("libftd2xx list_devices len={}", devs.len());
    for (i, d) in devs.iter().enumerate() {
        eprintln!(
            "[{i}] vid=0x{:04x} pid=0x{:04x} type={:?} open={} serial={:?} desc={:?}",
            d.vendor_id, d.product_id, d.device_type, d.port_open, d.serial_number, d.description
        );
    }

    if let Ok(devs) = libftd2xx::list_devices_fs() {
        eprintln!("libftd2xx list_devices_fs len={}", devs.len());
        for (i, d) in devs.iter().enumerate() {
            eprintln!(
                "[fs {i}] vid=0x{:04x} pid=0x{:04x} type={:?} open={} serial={:?} desc={:?}",
                d.vendor_id,
                d.product_id,
                d.device_type,
                d.port_open,
                d.serial_number,
                d.description
            );
        }
    }
    Ok(())
}

fn prepare_nand(client: &mut Client) -> Result<(u32, u32)> {
    let _ver = client.cmd_u32(CMD_GET_VERSION, 0)?;
    let _ = client.cmd_u32(CMD_SET_SMC_WORKAROUND, 0)?;
    let _ = client.cmd_u32(CMD_STOP_SMC, 0)?;
    std::thread::sleep(Duration::from_millis(500));

    let flash_config = client.cmd_u32(CMD_GET_FLASH_CONFIG, 0)?;
    if flash_config == 0 || flash_config == 0xFFFF_FFFF {
        bail!("console not found (flash_config=0x{flash_config:08x})");
    }

    let flash_size_bytes = flash_size_from_config(flash_config).ok_or_else(|| {
        anyhow::anyhow!("unknown flash size for flash_config=0x{flash_config:08x}")
    })?;
    let blocks = (flash_size_bytes / 512) as u32;
    Ok((flash_config, blocks))
}

fn flash_size_from_config(flash_config: u32) -> Option<usize> {
    let major = (flash_config >> 17) & 3;
    let minor = (flash_config >> 4) & 3;

    let size_mb = if major >= 1 {
        match minor {
            0 => {
                if major != 1 {
                    16
                } else {
                    return None;
                }
            }
            1 => {
                if major != 1 {
                    64
                } else {
                    16
                }
            }
            2 | 3 => {
                let a = (flash_config >> 19) & 0x3;
                let b = (flash_config >> 21) & 0xF;
                8usize.checked_shl((a + b) as u32)?
            }
            _ => return None,
        }
    } else {
        8usize.checked_shl(minor as u32)?
    };

    Some(size_mb * 1024 * 1024)
}

fn read_nand(client: &mut Client, out: std::path::PathBuf, start: u32, count: u32) -> Result<()> {
    let f = File::create(out).context("open output")?;
    let mut f = BufWriter::with_capacity(1024 * 1024, f);

    if start == 0 {
        client.start_stream(CMD_READ_FLASH_STREAM, count)?;
        for i in 0..count {
            let (ret, data) = client.recv_stream_block(NAND_BLOCK_BYTES)?;
            if ret != 0 {
                bail!("read failed at block {i}: 0x{ret:08x}");
            }
            f.write_all(&data.unwrap()).context("write output")?;

            if (i & 0xFF) == 0 {
                eprintln!("read {}/{} blocks", i + 1, count);
            }
        }
    } else {
        for i in 0..count {
            let lba = start + i;
            let (ret, data) = client.read_with_ret(CMD_READ_FLASH, lba, NAND_BLOCK_BYTES)?;
            if ret != 0 {
                bail!("read failed at lba {lba}: 0x{ret:08x}");
            }
            f.write_all(&data.unwrap()).context("write output")?;

            if (i & 0xFF) == 0 {
                eprintln!("read {}/{} blocks", i + 1, count);
            }
        }
    }

    Ok(())
}

fn write_nand(client: &mut Client, input: std::path::PathBuf, start: u32) -> Result<()> {
    let mut buf = vec![];
    File::open(input)
        .context("open input")?
        .read_to_end(&mut buf)
        .context("read input")?;

    if buf.len() % NAND_BLOCK_BYTES != 0 {
        bail!(
            "input size must be a multiple of 0x210 (got 0x{:x})",
            buf.len()
        );
    }

    let blocks = (buf.len() / NAND_BLOCK_BYTES) as u32;
    let mut i = 0u32;
    if client.supports_multi_write() {
        while i < blocks {
            let remaining = blocks - i;
            let chunk_blocks = remaining.min(64);
            let lba = start + i;

            let off = (i as usize) * NAND_BLOCK_BYTES;
            let end = off + (chunk_blocks as usize) * NAND_BLOCK_BYTES;
            let (ret, idx) =
                client.write_multi(CMD_WRITE_FLASH_MULTI, lba, NAND_BLOCK_BYTES, &buf[off..end])?;
            if ret != 0 {
                bail!("write failed at lba {}: 0x{ret:08x}", lba + idx);
            }

            i += chunk_blocks;
            eprintln!("written {}/{} blocks", i, blocks);
        }
    } else {
        while i < blocks {
            let lba = start + i;
            let off = (i as usize) * NAND_BLOCK_BYTES;
            let end = off + NAND_BLOCK_BYTES;
            let ret = client.write_single(CMD_WRITE_FLASH, lba, &buf[off..end])?;
            if ret != 0 {
                bail!("write failed at lba {}: 0x{ret:08x}", lba);
            }
            i += 1;
            if (i & 0xFF) == 0 || i == blocks {
                eprintln!("written {}/{} blocks", i, blocks);
            }
        }
    }

    Ok(())
}

fn prepare_emmc(client: &mut Client) -> Result<u32> {
    let _ver = client.cmd_u32(CMD_GET_VERSION, 0)?;
    let _ = client.cmd_u32(CMD_SET_SMC_WORKAROUND, 0)?;
    let _ = client.cmd_u32(CMD_STOP_SMC, 0)?;
    std::thread::sleep(Duration::from_millis(500));

    let detect = client.cmd_u8(CMD_EMMC_DETECT, 0)?;
    if detect == 0 {
        bail!("eMMC not detected");
    }

    let ret = client.cmd_u32(CMD_EMMC_INIT, 0)?;
    if ret != 0 {
        bail!("EMMC_INIT failed: {ret}");
    }

    let ext = client.cmd_exact_bytes(CMD_EMMC_GET_EXT_CSD, 0, 512)?;

    let sec_count = u32::from_le_bytes(ext[212..216].try_into().unwrap());
    if sec_count == 0 {
        bail!("invalid EXT_CSD SEC_COUNT=0");
    }
    Ok(sec_count)
}

fn read_emmc(client: &mut Client, out: std::path::PathBuf, start: u32, count: u32) -> Result<()> {
    let f = File::create(out).context("open output")?;
    let mut f = BufWriter::with_capacity(1024 * 1024, f);

    if start == 0 {
        client.start_stream(CMD_EMMC_READ_STREAM, count)?;
        for i in 0..count {
            let (ret, data) = client.recv_stream_block(EMMC_BLOCK_BYTES)?;
            if ret != 0 {
                bail!("read failed at block {i}: {ret}");
            }
            f.write_all(&data.unwrap()).context("write output")?;

            if (i & 0xFF) == 0 {
                eprintln!("read {}/{} blocks", i + 1, count);
            }
        }
    } else {
        for i in 0..count {
            let lba = start + i;
            let (ret, data) = client.read_with_ret(CMD_EMMC_READ, lba, EMMC_BLOCK_BYTES)?;
            if ret != 0 {
                bail!("read failed at lba {lba}: {ret}");
            }
            f.write_all(&data.unwrap()).context("write output")?;

            if (i & 0xFF) == 0 {
                eprintln!("read {}/{} blocks", i + 1, count);
            }
        }
    }

    Ok(())
}

fn write_emmc(client: &mut Client, input: std::path::PathBuf, start: u32) -> Result<()> {
    let mut buf = vec![];
    File::open(input)
        .context("open input")?
        .read_to_end(&mut buf)
        .context("read input")?;

    if buf.len() % EMMC_BLOCK_BYTES != 0 {
        bail!(
            "input size must be a multiple of 0x200 (got 0x{:x})",
            buf.len()
        );
    }

    let blocks = (buf.len() / EMMC_BLOCK_BYTES) as u32;
    let mut i = 0u32;
    if client.supports_multi_write() {
        while i < blocks {
            let remaining = blocks - i;
            let chunk_blocks = remaining.min(64);
            let lba = start + i;

            let off = (i as usize) * EMMC_BLOCK_BYTES;
            let end = off + (chunk_blocks as usize) * EMMC_BLOCK_BYTES;
            let (ret, idx) =
                client.write_multi(CMD_EMMC_WRITE_MULTI, lba, EMMC_BLOCK_BYTES, &buf[off..end])?;
            if ret != 0 {
                bail!("write failed at lba {}: {ret}", lba + idx);
            }

            i += chunk_blocks;
            eprintln!("written {}/{} blocks", i, blocks);
        }
    } else {
        while i < blocks {
            let lba = start + i;
            let off = (i as usize) * EMMC_BLOCK_BYTES;
            let end = off + EMMC_BLOCK_BYTES;
            let ret = client.write_single(CMD_EMMC_WRITE, lba, &buf[off..end])?;
            if ret != 0 {
                bail!("write failed at lba {}: {ret}", lba);
            }
            i += 1;
            if (i & 0x3FF) == 0 || i == blocks {
                eprintln!("written {}/{} blocks", i, blocks);
            }
        }
    }

    let _ = client.cmd_u32(CMD_START_SMC, 0);

    Ok(())
}

// DemoN functions

fn demon_list() -> Result<()> {
    use crate::demon::usb::UsbClient;

    match UsbClient::open() {
        Ok(_) => {
            eprintln!("DemoN device found");
            Ok(())
        }
        Err(e) => {
            eprintln!("DemoN device not found: {e}");
// LPC/XFlash functions

fn lpc_list() -> Result<()> {
    use crate::lpc::usb::UsbClient;

    match UsbClient::open() {
        Ok(_) => {
            eprintln!("LPC/XFlash device found");
            Ok(())
        }
        Err(e) => {
            eprintln!("LPC/XFlash device not found: {e}");
            Ok(())
        }
    }
}

fn demon_info() -> Result<()> {
    let mut client = DemonClient::open().context("Failed to open DemoN device")?;
    let info = client.init().context("Failed to initialize DemoN device")?;

    eprintln!("DemoN Device Information:");
    eprintln!("  Device ID: {:?}", info.device_id);
    eprintln!("  Protocol Version: 0x{:04x}", info.protocol_version);
    eprintln!("  Firmware Version: 0x{:04x}", info.firmware_version);
    eprintln!("  Flash ID: 0x{:04x}", info.nand_id);
    eprintln!("  Mode: {:?}", info.mode);

    if let Some(manufacturer) = client.get_manufacturer_name() {
        eprintln!("  Manufacturer: {}", manufacturer);
    }

    if let Some(nand_info) = client.get_nand_info() {
        eprintln!("  NAND Info:");
        eprintln!("    Name: {}", nand_info.name);
        eprintln!("    Page Size: {} bytes", nand_info.page_size);
        eprintln!("    Spare Size: {} bytes", nand_info.spare_size);
        eprintln!("    Chip Size: {} MiB", nand_info.chip_size);
        eprintln!("    Pages Per Block: {}", nand_info.pages_per_block);
        eprintln!("    Total Blocks: {}", nand_info.num_blocks());
        eprintln!(
            "    Total File Size: {} bytes (0x{:x})",
            nand_info.file_size(),
            nand_info.file_size()
        );
fn lpc_info() -> Result<()> {
    let mut client = LpcClient::open().context("Failed to open LPC/XFlash device")?;
    client
        .init()
        .context("Failed to initialize LPC/XFlash device")?;

    let version = client.version.unwrap_or(0);
    eprintln!("LPC/XFlash Device Information:");
    eprintln!("  ARM Version: {}", version);

    match client.flash_init() {
        Ok(config) => {
            eprintln!("  Flash Config: 0x{:08X}", config.raw);
            eprintln!("  Controller Type: {}", config.controller_type);
            eprintln!("  Block Type: {}", config.block_type);
            eprintln!("  Page Size: 0x{:X} bytes", config.page_size);
            eprintln!("  Meta Size: 0x{:X} bytes", config.meta_size);
            eprintln!("  Meta Type: {}", config.meta_type);
            eprintln!("  Block Size: 0x{:X} bytes", config.block_size);
            eprintln!("  Size Blocks: 0x{:X}", config.size_blocks);
            eprintln!("  Size Small Blocks: 0x{:X}", config.size_small_blocks);
            eprintln!("  File Blocks: 0x{:X}", config.file_blocks);
            eprintln!(
                "  Full File Size: 0x{:X} bytes ({} MB)",
                config.file_size(),
                config.file_size() / (1024 * 1024)
            );
            client.flash_deinit()?;
        }
        Err(e) => {
            eprintln!("  Failed to initialize flash: {e}");
        }
    }

    Ok(())
}

fn demon_read_nand(out: std::path::PathBuf, start: u32, count: Option<u32>) -> Result<Duration> {
    let mut client = DemonClient::open().context("Failed to open DemoN device")?;
    let _info = client.init().context("Failed to initialize DemoN device")?;

    let nand_info = client
        .get_nand_info()
        .ok_or_else(|| anyhow::anyhow!("NAND device not recognized"))?;

    let total_blocks = nand_info.num_blocks() as u32;
    let block_size = nand_info.total_block_size() as usize;
    let blocks_to_read = count.unwrap_or(total_blocks.saturating_sub(start));

    eprintln!("NAND: {} ({} MiB)", nand_info.name, nand_info.chip_size);
fn lpc_read_nand(out: std::path::PathBuf, start: u32, count: Option<u32>) -> Result<Duration> {
    let mut client = LpcClient::open().context("Failed to open LPC/XFlash device")?;
    client
        .init()
        .context("Failed to initialize LPC/XFlash device")?;

    let config_ver = client.version.unwrap_or(0);

    let config = client.flash_init().context("Failed to initialize flash")?;
    let total_blocks = config.size_small_blocks;
    let block_size = 0x4200; // Fixed block size for LPC
    let blocks_to_read = count.unwrap_or(total_blocks.saturating_sub(start));

    eprintln!("LPC/XFlash: ARM Version {}", config_ver);
    eprintln!("Flash Config: 0x{:08X}", config.raw);
    eprintln!(
        "Block size: {} bytes, Total blocks: {}",
        block_size, total_blocks
    );
    eprintln!("Reading {} blocks from {}", blocks_to_read, start);

    let f = File::create(out).context("open output")?;
    let mut f = BufWriter::with_capacity(1024 * 1024, f);

    let t0 = Instant::now();
    let mut block_buf = vec![0u8; block_size];

    for i in 0..blocks_to_read {
        let block_num = start + i;

    for i in 0..blocks_to_read {
        let block_num = start + i;

        if (i & 0x3F) == 0 {
            eprintln!("Reading block {}/{}", i + 1, blocks_to_read);
        }

        let _len = client
            .read_block(block_num as u16, block_size, &mut block_buf)
            .with_context(|| format!("read block {}", block_num))?;
        f.write_all(&block_buf).context("write output")?;
    }

        let (status, data) = client
            .flash_read(block_num)
            .with_context(|| format!("read block {}", block_num))?;

        if crate::lpc::status::is_error(status) {
            bail!("Error reading block {}: status=0x{:X}", block_num, status);
        }

        f.write_all(&data).context("write output")?;
    }

    client.flash_deinit()?;
    f.flush().context("flush output")?;
    Ok(t0.elapsed())
}

fn demon_write_nand(input: std::path::PathBuf, start: u32) -> Result<()> {
    let mut client = DemonClient::open().context("Failed to open DemoN device")?;
    let _info = client.init().context("Failed to initialize DemoN device")?;

    let nand_info = client
        .get_nand_info()
        .ok_or_else(|| anyhow::anyhow!("NAND device not recognized"))?;

    let total_blocks = nand_info.num_blocks() as u32;
    let block_size = nand_info.total_block_size() as usize;
fn lpc_write_nand(input: std::path::PathBuf, start: u32) -> Result<()> {
    let mut client = LpcClient::open().context("Failed to open LPC/XFlash device")?;
    client
        .init()
        .context("Failed to initialize LPC/XFlash device")?;

    let config_ver = client.version.unwrap_or(0);

    let config = client.flash_init().context("Failed to initialize flash")?;
    let total_blocks = config.size_small_blocks;
    let block_size = 0x4200u32; // Fixed block size for LPC

    let input_meta = std::fs::metadata(&input).context("stat input")?;
    let input_len = input_meta.len() as usize;

    if input_len % block_size != 0 {
        bail!(
            "input size must be a multiple of {} bytes (block size)",
    if input_len % block_size as usize != 0 {
        bail!(
            "input size must be a multiple of {} bytes (LPC block size)",
            block_size
        );
    }

    let file_blocks = (input_len / block_size) as u32;

    if start >= total_blocks {
        bail!("start block {start} out of range (total blocks {total_blocks})");
    }
    if start + file_blocks > total_blocks {
        bail!(
            "requested range {}..{} out of range (total blocks {total_blocks})",
            start,
            start + file_blocks
        );
    }

    eprintln!("NAND: {} ({} MiB)", nand_info.name, nand_info.chip_size);
    let file_blocks = (input_len / block_size as usize) as u32;

    if start >= total_blocks {
        bail!(
            "start block {} out of range (total blocks {})",
            start,
            total_blocks
        );
    }
    if start + file_blocks > total_blocks {
        bail!(
            "requested range {}..{} out of range (total blocks {})",
            start,
            start + file_blocks,
            total_blocks
        );
    }

    eprintln!("LPC/XFlash: ARM Version {}", config_ver);
    eprintln!("Flash Config: 0x{:08X}", config.raw);
    eprintln!(
        "Block size: {} bytes, Writing {} blocks to {}",
        block_size, file_blocks, start
    );

    let f = File::open(input).context("open input")?;
    let mut f = BufReader::with_capacity(1024 * 1024, f);

    let mut block_buf = vec![0u8; block_size];
    let mut block_buf = vec![0u8; block_size as usize];
    for i in 0..file_blocks {
        let block_num = start + i;

        f.read_exact(&mut block_buf).context("read input block")?;
        client
            .write_block(block_num as u16, &block_buf)
            .with_context(|| format!("write block {}", block_num))?;

        let status = client
            .flash_write(block_num, &block_buf)
            .with_context(|| format!("write block {}", block_num))?;

        if crate::lpc::status::is_error(status) {
            bail!("Error writing block {}: status=0x{:X}", block_num, status);
        }

        if (i & 0x3F) == 0 {
            eprintln!("Written block {}/{}", i + 1, file_blocks);
        }
    }

    client.flash_deinit()?;
    Ok(())
}
