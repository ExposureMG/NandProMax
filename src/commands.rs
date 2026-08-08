use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};

use crate::demon::DemonClient;
use crate::flasher::{run_read_nand, run_write_nand};
use crate::lpc::LpcClient;
use crate::picoflasher::pfc::{
    Client, CMD_EMMC_DETECT, CMD_EMMC_GET_EXT_CSD, CMD_EMMC_INIT, CMD_EMMC_READ, CMD_EMMC_WRITE,
    CMD_EMMC_WRITE_MULTI, CMD_GET_FLASH_CONFIG, CMD_GET_VERSION, CMD_READ_FLASH,
    CMD_SET_SMC_WORKAROUND, CMD_START_SMC, CMD_STOP_SMC, CMD_WRITE_FLASH, CMD_WRITE_FLASH_MULTI,
    EMMC_BLOCK_BYTES, NAND_BLOCK_BYTES,
};
use crate::progress::Progress;
use crate::tcp::TcpServer;
use crate::types::{AdapterType, DeviceType, FtdiPageFormat, MediaType};

/// Format and emit a log message through a [`Progress`] sink.
macro_rules! plog {
    ($p:expr, $($arg:tt)*) => {
        $p.log(&format!($($arg)*))
    };
}

// ---------------------------------------------------------------------------
// Top-level command handlers
// ---------------------------------------------------------------------------

pub fn cmd_read_nand(
    out: PathBuf,
    device: Option<DeviceType>,
    media_type: Option<MediaType>,
    start: u32,
    count: Option<u32>,
    serial: Option<String>,
    addr: String,
    ftdi_desc: String,
    ftdi_index: Option<i32>,
    freq_hz: u32,
    page_format: FtdiPageFormat,
    timeout_ms: u64,
    progress: &mut dyn Progress,
) -> Result<()> {
    let timeout = Duration::from_millis(timeout_ms);
    let (target_dev, target_adapter, target_media) = auto_detect_device(
        device,
        None,
        media_type,
        serial.as_deref(),
        &addr,
        &ftdi_desc,
        ftdi_index,
        freq_hz,
        timeout,
    )?;

    plog!(
        progress,
        "Using device={:?} adapter={:?} type={:?}",
        target_dev, target_adapter, target_media
    );

    let elapsed = match (target_dev, target_media) {
        (DeviceType::Pico, MediaType::Spi) => {
            let (mut client, resolved) = if target_adapter == AdapterType::Tcp {
                Client::connect_tcp(&addr, timeout)?
            } else {
                let port = serial.as_deref().unwrap_or("");
                Client::connect_usb(port, timeout)?
            };
            plog!(progress, "connected to {resolved}");
            let (_flash_config, blocks_total) = prepare_nand(&mut client, progress)?;
            let blocks = count.unwrap_or(blocks_total.saturating_sub(start));
            let t0 = Instant::now();
            read_nand(&mut client, out, start, blocks, progress)?;
            t0.elapsed()
        }
        (DeviceType::Pico, MediaType::Emmc) => {
            let (mut client, resolved) = if target_adapter == AdapterType::Tcp {
                Client::connect_tcp(&addr, timeout)?
            } else {
                let port = serial.as_deref().unwrap_or("");
                Client::connect_usb(port, timeout)?
            };
            plog!(progress, "connected to {resolved}");
            let blocks_total = prepare_emmc(&mut client, progress)?;
            let blocks = count.unwrap_or(blocks_total.saturating_sub(start));
            let t0 = Instant::now();
            read_emmc(&mut client, out, start, blocks, progress)?;
            t0.elapsed()
        }
        (DeviceType::Ftdi, _) => {
            ftdi_read_nand(out, start, count, page_format, &ftdi_desc, ftdi_index, freq_hz, progress)?
        }
        (DeviceType::Lpc, _) => {
            let mut client = LpcClient::open().context("Failed to open LPC device")?;
            run_read_nand(&mut client, out, start, count)?
        }
        (DeviceType::Demon, _) => {
            let mut client = DemonClient::open().context("Failed to open DemoN device")?;
            run_read_nand(&mut client, out, start, count)?
        }
        (DeviceType::Jrp, _) => bail!("JR-Programmer read not yet implemented"),
        // Esp is resolved to Pico+Tcp by auto_detect_device
        (DeviceType::Esp, _) => unreachable!("Esp resolved to Pico+Tcp in auto_detect_device"),
    };

    println!("ok ({:.3}s)", elapsed.as_secs_f64());
    plog!(progress, "Operation completed in {:.2}s", elapsed.as_secs_f64());
    Ok(())
}

pub fn cmd_write_nand(
    input: PathBuf,
    device: Option<DeviceType>,
    media_type: Option<MediaType>,
    start: u32,
    count: Option<u32>,
    erase: bool,
    verify: bool,
    serial: Option<String>,
    addr: String,
    ftdi_desc: String,
    ftdi_index: Option<i32>,
    freq_hz: u32,
    page_format: FtdiPageFormat,
    timeout_ms: u64,
    progress: &mut dyn Progress,
) -> Result<()> {
    let timeout = Duration::from_millis(timeout_ms);
    let (target_dev, target_adapter, target_media) = auto_detect_device(
        device,
        None,
        media_type,
        serial.as_deref(),
        &addr,
        &ftdi_desc,
        ftdi_index,
        freq_hz,
        timeout,
    )?;

    plog!(
        progress,
        "Using device={:?} adapter={:?} type={:?}",
        target_dev, target_adapter, target_media
    );

    let elapsed = match (target_dev, target_media) {
        (DeviceType::Pico, MediaType::Spi) => {
            let (mut client, resolved) = if target_adapter == AdapterType::Tcp {
                Client::connect_tcp(&addr, timeout)?
            } else {
                let port = serial.as_deref().unwrap_or("");
                Client::connect_usb(port, timeout)?
            };
            plog!(progress, "connected to {resolved}");
            let (_flash_config, _blocks_total) = prepare_nand(&mut client, progress)?;
            let t0 = Instant::now();
            write_nand(&mut client, input, start, progress)?;
            t0.elapsed()
        }
        (DeviceType::Pico, MediaType::Emmc) => {
            let (mut client, resolved) = if target_adapter == AdapterType::Tcp {
                Client::connect_tcp(&addr, timeout)?
            } else {
                let port = serial.as_deref().unwrap_or("");
                Client::connect_usb(port, timeout)?
            };
            plog!(progress, "connected to {resolved}");
            let _blocks_total = prepare_emmc(&mut client, progress)?;
            let t0 = Instant::now();
            write_emmc(&mut client, input, start, progress)?;
            t0.elapsed()
        }
        (DeviceType::Ftdi, _) => {
            ftdi_write_nand(
                input, start, count, page_format, &ftdi_desc, ftdi_index, freq_hz, erase, verify,
                progress,
            )?
        }
        (DeviceType::Lpc, _) => {
            let t0 = Instant::now();
            let mut client = LpcClient::open().context("Failed to open LPC device")?;
            run_write_nand(&mut client, input, start)?;
            t0.elapsed()
        }
        (DeviceType::Demon, _) => {
            let t0 = Instant::now();
            let mut client = DemonClient::open().context("Failed to open DemoN device")?;
            run_write_nand(&mut client, input, start)?;
            t0.elapsed()
        }
        (DeviceType::Jrp, _) => bail!("JR-Programmer write not yet implemented"),
        (DeviceType::Esp, _) => unreachable!("Esp resolved to Pico+Tcp in auto_detect_device"),
    };

    plog!(progress, "Operation completed in {:.2}s", elapsed.as_secs_f64());
    println!("ok ({:.3}s)", elapsed.as_secs_f64());
    Ok(())
}

pub fn cmd_info(
    device: Option<DeviceType>,
    serial: Option<String>,
    addr: String,
    ftdi_desc: String,
    ftdi_index: Option<i32>,
    freq_hz: u32,
    timeout_ms: u64,
    progress: &mut dyn Progress,
) -> Result<()> {
    let timeout = Duration::from_millis(timeout_ms);
    let target_dev = match device {
        Some(DeviceType::Esp) => {
            // ESP = PicoFlasher over TCP
            let (mut client, resolved) = Client::connect_tcp(&addr, timeout)?;
            plog!(progress, "PicoFlasher (ESP/TCP) connected to {resolved}");
            let ver = client.cmd_u32(CMD_GET_VERSION, 0)?;
            plog!(progress, "PicoFlasher Firmware Version: 0x{ver:08x}");
            println!("ok");
            return Ok(());
        }
        other => other.unwrap_or(DeviceType::Pico),
    };

    match target_dev {
        DeviceType::Pico => {
            let (mut client, resolved) = {
                let port = serial.as_deref().unwrap_or("");
                Client::connect_usb(port, timeout)?
            };
            plog!(progress, "PicoFlasher connected to {resolved}");
            let ver = client.cmd_u32(CMD_GET_VERSION, 0)?;
            plog!(progress, "PicoFlasher Firmware Version: 0x{ver:08x}");
        }
        DeviceType::Ftdi => {
            let mut xspi = crate::ftdi::spi::XSpi::open(&ftdi_desc, ftdi_index, freq_hz)?;
            xspi.enter_flash_mode()?;
            let flash_config = xspi.read_u32(0x00)?;
            let geom = crate::ftdi::spi::sfc_init(flash_config)?;
            plog!(progress, "FTDI SPI Flasher Config: 0x{flash_config:08x}");
            plog!(progress, "NAND Size: {} MB", geom.nand_size_mb);
        }
        DeviceType::Lpc => {
            lpc_info(progress)?;
        }
        DeviceType::Jrp => bail!("JR-Programmer info not yet implemented"),
        DeviceType::Demon => {
            demon_info(progress)?;
        }
        DeviceType::Esp => unreachable!(),
    }
    println!("ok");
    Ok(())
}

pub fn cmd_list_devices(progress: &mut dyn Progress) -> Result<()> {
    plog!(progress, "Listing available devices:");
    plog!(progress, "1. FTDI devices:");
    let _ = ftdi_list(progress);
    plog!(progress, "2. LPC devices:");
    let _ = lpc_list(progress);
    plog!(progress, "3. DemoN devices:");
    let _ = demon_list(progress);
    println!("ok");
    Ok(())
}

pub fn cmd_xsvf_detect(
    device: Option<DeviceType>,
    progress: &mut dyn Progress,
) -> Result<()> {
    let target_dev = device.unwrap_or(DeviceType::Lpc);
    match target_dev {
        DeviceType::Lpc | DeviceType::Jrp => {
            let mut client = LpcClient::open().context("Failed to open LPC/XFlash device")?;
            client.init().context("Failed to initialize LPC/XFlash device")?;
            let version = client.version.unwrap_or(0);
            plog!(progress, "LPC/XFlash Device Information:");
            plog!(progress, "  ARM Version: {version}");
            match client.flash_init() {
                Ok(config) => {
                    plog!(progress, "  Flash Config: 0x{:08X}", config.raw);
                    plog!(progress, "  Controller Type: {}", config.controller_type);
                    plog!(progress, "  Block Type: {}", config.block_type);
                    plog!(progress, "  Page Size: 0x{:X} bytes", config.page_size);
                    plog!(progress, "  Block Size: 0x{:X} bytes", config.block_size);
                    plog!(progress, "  Size Blocks: 0x{:X}", config.size_blocks);
                    plog!(
                        progress,
                        "  Full File Size: 0x{:X} bytes ({} MB)",
                        config.file_size(),
                        config.file_size() / (1024 * 1024)
                    );
                    client.flash_deinit()?;
                }
                Err(e) => {
                    plog!(progress, "  Flash init failed: {e}");
                }
            }
        }
        other => bail!("XSVF detect not supported on {:?}", other),
    }
    println!("ok");
    Ok(())
}

pub fn cmd_xsvf_write(
    input: PathBuf,
    device: Option<DeviceType>,
    progress: &mut dyn Progress,
) -> Result<()> {
    let target_dev = device.unwrap_or(DeviceType::Lpc);
    match target_dev {
        DeviceType::Lpc | DeviceType::Jrp => {
            let data = std::fs::read(&input)
                .with_context(|| format!("read XSVF file {:?}", input))?;
            plog!(progress, "Loaded {} bytes from {:?}", data.len(), input);

            let mut client = LpcClient::open().context("Failed to open LPC/XFlash device")?;
            client.init().context("Failed to initialize LPC/XFlash device")?;
            client.xsvf_init().context("XSVF init failed")?;

            plog!(progress, "Programming XSVF...");
            client.xsvf_write(&data).context("XSVF write failed")?;
            client.xsvf_execute().context("XSVF execute failed")?;
            plog!(progress, "XSVF complete");
        }
        other => bail!("XSVF write not supported on {:?}", other),
    }
    println!("ok");
    Ok(())
}

pub fn cmd_serve_tcp(
    bind: String,
    device: Option<DeviceType>,
    progress: &mut dyn Progress,
) -> Result<()> {
    let target_dev = device.unwrap_or(DeviceType::Ftdi);
    plog!(
        progress,
        "Starting TCP device server using {target_dev:?} backend on {bind}..."
    );
    match target_dev {
        DeviceType::Ftdi => {
            bail!("Serving FTDI over TCP requires connected FTDI hardware");
        }
        DeviceType::Lpc | DeviceType::Jrp => {
            let client = LpcClient::open().context("Failed to open LPC device")?;
            let mut server = TcpServer::bind(&bind, client)?;
            server.run()?;
        }
        DeviceType::Demon => {
            let client = DemonClient::open().context("Failed to open DemoN device")?;
            let mut server = TcpServer::bind(&bind, client)?;
            server.run()?;
        }
        DeviceType::Pico | DeviceType::Esp => {
            bail!("PicoFlasher already functions as a TCP flasher endpoint");
        }
    }
    println!("ok");
    Ok(())
}

// ---------------------------------------------------------------------------
// Device auto-detection
// ---------------------------------------------------------------------------

pub fn auto_detect_device(
    user_device: Option<DeviceType>,
    user_adapter: Option<AdapterType>,
    user_media: Option<MediaType>,
    serial: Option<&str>,
    addr: &str,
    ftdi_desc: &str,
    ftdi_index: Option<i32>,
    freq_hz: u32,
    timeout: Duration,
) -> Result<(DeviceType, AdapterType, MediaType)> {
    if let Some(dev) = user_device {
        match dev {
            // ESP = PicoFlasher over TCP
            DeviceType::Esp => {
                let media = user_media.unwrap_or(MediaType::Spi);
                return Ok((DeviceType::Pico, AdapterType::Tcp, media));
            }
            DeviceType::Jrp => {
                bail!("JR-Programmer is not yet supported");
            }
            _ => {
                let adapter = user_adapter.unwrap_or(AdapterType::Usb);
                let media = user_media.unwrap_or(MediaType::Spi);
                return Ok((dev, adapter, media));
            }
        }
    }

    if user_adapter == Some(AdapterType::Tcp) {
        let media = user_media.unwrap_or(MediaType::Spi);
        return Ok((DeviceType::Pico, AdapterType::Tcp, media));
    }

    if let Some(port) = serial {
        if Client::connect_usb(port, timeout).is_ok() {
            let media = user_media.unwrap_or(MediaType::Spi);
            return Ok((DeviceType::Pico, AdapterType::Usb, media));
        }
    }

    if crate::ftdi::spi::XSpi::open(ftdi_desc, ftdi_index, freq_hz).is_ok() {
        let media = user_media.unwrap_or(MediaType::Spi);
        return Ok((DeviceType::Ftdi, AdapterType::Usb, media));
    }

    if LpcClient::open().is_ok() {
        let media = user_media.unwrap_or(MediaType::Spi);
        return Ok((DeviceType::Lpc, AdapterType::Usb, media));
    }

    if DemonClient::open().is_ok() {
        let media = user_media.unwrap_or(MediaType::Spi);
        return Ok((DeviceType::Demon, AdapterType::Usb, media));
    }

    // Try ESP/TCP as last resort if an addr is reachable
    if Client::connect_tcp(addr, timeout).is_ok() {
        let media = user_media.unwrap_or(MediaType::Spi);
        return Ok((DeviceType::Pico, AdapterType::Tcp, media));
    }

    let adapter = user_adapter.unwrap_or(AdapterType::Usb);
    let media = user_media.unwrap_or(MediaType::Spi);
    Ok((DeviceType::Pico, adapter, media))
}

// ---------------------------------------------------------------------------
// FTDI helpers
// ---------------------------------------------------------------------------

fn ftdi_list(progress: &mut dyn Progress) -> Result<()> {
    let devs = crate::ftdi::list_devices()?;
    plog!(progress, "ftdi list_devices len={}", devs.len());
    for d in &devs {
        plog!(
            progress,
            "  [{}] type={} serial={} desc={}",
            d.index, d.device_type, d.serial_number, d.description
        );
    }
    Ok(())
}

fn ftdi_read_nand(
    out: PathBuf,
    start: u32,
    count: Option<u32>,
    page_format: FtdiPageFormat,
    ftdi_desc: &str,
    ftdi_index: Option<i32>,
    freq_hz: u32,
    progress: &mut dyn Progress,
) -> Result<Duration> {
    use crate::ftdi::spi::{
        sfc_init, xnand_clear_status, xnand_read_batch, XSpi, NAND_READ_BATCH_PAGES,
    };

    plog!(progress, "ftdi freq_hz={freq_hz}");
    let mut xspi = XSpi::open(ftdi_desc, ftdi_index, freq_hz)?;
    xspi.enter_flash_mode()?;

    let flash_config = xspi.read_u32(0x00)?;
    let geom = sfc_init(flash_config)?;
    let total_small_pages = geom.pages_count_in_nand;

    let use_big_pages = match page_format {
        FtdiPageFormat::Auto => geom.large_block != 0,
        FtdiPageFormat::Small => false,
        FtdiPageFormat::Big => true,
    };

    let (start_small, pages_small, unit_name) = if use_big_pages {
        if total_small_pages % 4 != 0 {
            bail!("NAND page count not divisible by 4; cannot use big-page format");
        }
        let total_big_pages = total_small_pages / 4;
        let pages_big = count.unwrap_or(total_big_pages.saturating_sub(start));
        (start * 4, pages_big * 4, "big (0x840)")
    } else {
        let pages = count.unwrap_or(total_small_pages.saturating_sub(start));
        (start, pages, "small (0x210)")
    };

    plog!(
        progress,
        "flash_config=0x{flash_config:08x} nand={}MB page_format={} start={} count={}",
        geom.nand_size_mb,
        unit_name,
        start,
        pages_small / if use_big_pages { 4 } else { 1 }
    );

    let f = File::create(out).context("open output")?;
    let mut f = BufWriter::with_capacity(1024 * 1024, f);

    let t0 = Instant::now();
    let mut batch_buf = vec![0u8; NAND_READ_BATCH_PAGES * 0x210];
    let mut big_buf = vec![0u8; 0x840];

    let mut i = 0usize;
    let total_pages = pages_small as usize;
    while i < total_pages {
        let batch_size = (total_pages - i).min(NAND_READ_BATCH_PAGES);
        let start_page = start_small + (i as u32);

        xnand_clear_status(&mut xspi).context("clear status")?;
        xnand_read_batch(&mut xspi, start_page, batch_size, &mut batch_buf[..batch_size * 0x210])
            .with_context(|| format!("batch read starting at page {start_page} ({batch_size} pages)"))?;

        for p in 0..batch_size {
            let page_buf = &batch_buf[p * 0x210..(p + 1) * 0x210];
            if use_big_pages {
                let idx = ((i + p) % 4) * 0x210;
                big_buf[idx..idx + 0x210].copy_from_slice(page_buf);
                if ((i + p) & 3) == 3 {
                    f.write_all(&big_buf)?;
                }
            } else {
                f.write_all(page_buf)?;
            }
        }

        let old_i = i;
        i += batch_size;

        if (old_i & 0x3FF) != (i & 0x3FF) || i == total_pages {
            plog!(progress, "read {i}/{pages_small} pages");
            progress.update(i as u64, pages_small as u64);
        }
    }

    f.flush().context("flush output")?;
    Ok(t0.elapsed())
}

fn ftdi_write_nand(
    input: PathBuf,
    start: u32,
    count: Option<u32>,
    page_format: FtdiPageFormat,
    ftdi_desc: &str,
    ftdi_index: Option<i32>,
    freq_hz: u32,
    erase: bool,
    verify: bool,
    progress: &mut dyn Progress,
) -> Result<Duration> {
    use crate::ftdi::spi::{
        sfc_init, xnand_clear_status, xnand_erase_block, xnand_read_batch, xnand_write_page_raw,
        XSpi, NAND_READ_BATCH_PAGES,
    };

    let input_meta = std::fs::metadata(&input).context("stat input")?;
    let input_len = input_meta.len() as usize;

    plog!(progress, "ftdi freq_hz={freq_hz}");
    let mut xspi = XSpi::open(ftdi_desc, ftdi_index, freq_hz)?;
    xspi.enter_flash_mode()?;

    let flash_config = xspi.read_u32(0x00)?;
    let geom = sfc_init(flash_config)?;
    let total_small_pages = geom.pages_count_in_nand;

    let use_big_pages = match page_format {
        FtdiPageFormat::Auto => geom.large_block != 0,
        FtdiPageFormat::Small => false,
        FtdiPageFormat::Big => true,
    };

    let (input_page_bytes, unit_name) = if use_big_pages {
        (0x840usize, "big (0x840)")
    } else {
        (0x210usize, "small (0x210)")
    };

    if input_len % input_page_bytes != 0 {
        bail!("input size must be a multiple of 0x{input_page_bytes:x} bytes");
    }

    let file_pages = (input_len / input_page_bytes) as u32;
    let pages = count.unwrap_or(file_pages);
    if pages > file_pages {
        bail!("input has {file_pages} pages but --count={pages} requested");
    }

    let (start_small, pages_small) = if use_big_pages {
        if total_small_pages % 4 != 0 {
            bail!("NAND page count not divisible by 4; cannot use big-page format");
        }
        (start * 4, pages * 4)
    } else {
        (start, pages)
    };

    if start_small >= total_small_pages {
        bail!("start page {start} out of range");
    }
    if start_small + pages_small > total_small_pages {
        bail!(
            "requested range {}..{} out of range",
            start_small,
            start_small + pages_small
        );
    }

    plog!(
        progress,
        "flash_config=0x{flash_config:08x} nand={}MB page_format={} start={} pages={} erase={} verify={} (input_pages={file_pages})",
        geom.nand_size_mb, unit_name, start, pages, erase, verify
    );

    let input_bytes = std::fs::read(&input).context("read input file into memory")?;

    let t0 = Instant::now();
    let mut page_buf = [0u8; 0x210];
    let mut written_batch_buf = vec![0u8; NAND_READ_BATCH_PAGES * 0x210];
    let mut readback_batch_buf = vec![0u8; NAND_READ_BATCH_PAGES * 0x210];
    let mut batch_count = 0usize;
    let mut batch_start_page = start_small;

    for i in 0..pages_small {
        if (i & 0xFF) == 0 {
            xnand_clear_status(&mut xspi).context("clear status")?;
        }

        if use_big_pages {
            let big_page_idx = (i as usize / 4) * 0x840;
            let sub_idx = (i as usize % 4) * 0x210;
            page_buf.copy_from_slice(&input_bytes[big_page_idx + sub_idx..big_page_idx + sub_idx + 0x210]);
        } else {
            let small_page_idx = i as usize * 0x210;
            page_buf.copy_from_slice(&input_bytes[small_page_idx..small_page_idx + 0x210]);
        }

        let page = start_small + i;
        if erase && (page % geom.page_count_in_block) == 0 {
            xnand_erase_block(&mut xspi, flash_config, page)
                .with_context(|| format!("erase block at page {page}"))?;
        }
        xnand_write_page_raw(&mut xspi, page, &page_buf)
            .with_context(|| format!("write page {page}"))?;

        if verify {
            if batch_count == 0 {
                batch_start_page = page;
            }
            written_batch_buf[batch_count * 0x210..(batch_count + 1) * 0x210]
                .copy_from_slice(&page_buf);
            batch_count += 1;

            if batch_count == NAND_READ_BATCH_PAGES || i + 1 == pages_small {
                xnand_read_batch(
                    &mut xspi,
                    batch_start_page,
                    batch_count,
                    &mut readback_batch_buf[..batch_count * 0x210],
                )
                .with_context(|| {
                    format!("verify batch read starting at page {batch_start_page} ({batch_count} pages)")
                })?;

                let exp_slice = &written_batch_buf[..batch_count * 0x210];
                let got_slice = &readback_batch_buf[..batch_count * 0x210];
                if exp_slice != got_slice {
                    for (offset, (&exp, &got)) in exp_slice.iter().zip(got_slice.iter()).enumerate() {
                        if exp != got {
                            let failed_page = batch_start_page + (offset / 0x210) as u32;
                            let page_off = offset % 0x210;
                            bail!(
                                "verify failed at page {failed_page} (first mismatch at +0x{page_off:x}: expected 0x{exp:02x}, got 0x{got:02x})"
                            );
                        }
                    }
                }
                batch_count = 0;
            }
        }

        if (i & 0x3FF) == 0 {
            let done = i + 1;
            plog!(progress, "wrote {done}/{pages_small} pages");
            progress.update(done as u64, pages_small as u64);
        }
    }

    Ok(t0.elapsed())
}

// ---------------------------------------------------------------------------
// PicoFlasher NAND helpers
// ---------------------------------------------------------------------------

fn prepare_nand(client: &mut Client, progress: &mut dyn Progress) -> Result<(u32, u32)> {
    let ver = client.cmd_u32(CMD_GET_VERSION, 0).context("GET_VERSION")?;
    plog!(progress, "pfc version=0x{ver:08x}");
    let _ = client.cmd_void(CMD_STOP_SMC, 0);
    let _ = client.cmd_void(CMD_SET_SMC_WORKAROUND, 1);
    let flash_config = client
        .cmd_u32(CMD_GET_FLASH_CONFIG, 0)
        .context("GET_FLASH_CONFIG")?;
    let blocks_total = blocks_from_flash_config(flash_config);
    Ok((flash_config, blocks_total))
}

fn blocks_from_flash_config(config: u32) -> u32 {
    let size_code = (config >> 17) & 0x03;
    match size_code {
        0 => 1024,
        1 => 2048,
        2 => 4096,
        _ => 1024,
    }
}

fn read_nand(
    client: &mut Client,
    out: PathBuf,
    start: u32,
    blocks: u32,
    progress: &mut dyn Progress,
) -> Result<()> {
    let f = File::create(out).context("open output")?;
    let mut f = BufWriter::with_capacity(1024 * 1024, f);

    let end_block = start + blocks;
    let mut current_block = start;
    while current_block < end_block {
        let (ret, read_bytes) = client.read_with_ret(CMD_READ_FLASH, current_block, NAND_BLOCK_BYTES)?;
        if ret != 0 {
            bail!("block read failed at block {current_block}: status {ret}");
        }
        let read_bytes = read_bytes.context("missing data buffer")?;
        if read_bytes.len() != NAND_BLOCK_BYTES {
            bail!(
                "block read mismatch at {current_block}: expected {NAND_BLOCK_BYTES}, got {}",
                read_bytes.len()
            );
        }
        f.write_all(&read_bytes)?;
        current_block += 1;
        if ((current_block - start) & 0x3F) == 0 || current_block == end_block {
            let done = current_block - start;
            plog!(progress, "read {done}/{blocks} blocks");
            progress.update(done as u64, blocks as u64);
        }
    }

    let _ = client.cmd_void(CMD_START_SMC, 0);
    f.flush().context("flush output")?;
    Ok(())
}

fn write_nand(
    client: &mut Client,
    input: PathBuf,
    start: u32,
    progress: &mut dyn Progress,
) -> Result<()> {
    let mut buf = vec![];
    File::open(input)
        .context("open input")?
        .read_to_end(&mut buf)
        .context("read input")?;

    if buf.len() % NAND_BLOCK_BYTES != 0 {
        bail!(
            "input size must be a multiple of 0x{:x} (got 0x{:x})",
            NAND_BLOCK_BYTES,
            buf.len()
        );
    }

    let blocks = (buf.len() / NAND_BLOCK_BYTES) as u32;
    let mut i = 0u32;
    if client.supports_multi_write() {
        while i < blocks {
            let remaining = blocks - i;
            let chunk_blocks = remaining.min(64);
            let block = start + i;
            let off = (i as usize) * NAND_BLOCK_BYTES;
            let end = off + (chunk_blocks as usize) * NAND_BLOCK_BYTES;
            let (ret, idx) = client.write_multi(
                CMD_WRITE_FLASH_MULTI,
                block,
                NAND_BLOCK_BYTES,
                &buf[off..end],
            )?;
            if ret != 0 {
                bail!("write failed at block {}: {ret}", block + idx);
            }
            i += chunk_blocks;
            plog!(progress, "written {i}/{blocks} blocks");
            progress.update(i as u64, blocks as u64);
        }
    } else {
        while i < blocks {
            let block = start + i;
            let off = (i as usize) * NAND_BLOCK_BYTES;
            let end = off + NAND_BLOCK_BYTES;
            let ret = client.write_single(CMD_WRITE_FLASH, block, &buf[off..end])?;
            if ret != 0 {
                bail!("write failed at block {}: {ret}", block);
            }
            i += 1;
            if (i & 0x3F) == 0 || i == blocks {
                plog!(progress, "written {i}/{blocks} blocks");
                progress.update(i as u64, blocks as u64);
            }
        }
    }

    let _ = client.cmd_void(CMD_START_SMC, 0);
    Ok(())
}

// ---------------------------------------------------------------------------
// PicoFlasher eMMC helpers
// ---------------------------------------------------------------------------

fn prepare_emmc(client: &mut Client, progress: &mut dyn Progress) -> Result<u32> {
    let ver = client.cmd_u32(CMD_GET_VERSION, 0).context("GET_VERSION")?;
    plog!(progress, "pfc version=0x{ver:08x}");
    let _ = client.cmd_void(CMD_STOP_SMC, 0);
    let _ = client.cmd_void(CMD_SET_SMC_WORKAROUND, 1);

    let ret = client.cmd_u32(CMD_EMMC_INIT, 0).context("EMMC_INIT")?;
    if ret != 0 {
        bail!("EMMC_INIT failed: {ret}");
    }

    let ret = client.cmd_u8(CMD_EMMC_DETECT, 0).context("EMMC_DETECT")?;
    if ret == 0 {
        bail!("EMMC_DETECT failed (returned 0)");
    }

    let ext_csd = client
        .cmd_exact_bytes(CMD_EMMC_GET_EXT_CSD, 0, 512)
        .context("EMMC_GET_EXT_CSD")?;
    if ext_csd.len() != 512 {
        bail!("EXT_CSD mismatch: expected 512, got {}", ext_csd.len());
    }

    let sec_count = u32::from_le_bytes(ext_csd[212..216].try_into().unwrap());
    plog!(progress, "emmc sec_count={sec_count} (~{} MB)", sec_count / 2048);
    Ok(sec_count)
}

fn read_emmc(
    client: &mut Client,
    out: PathBuf,
    start: u32,
    blocks: u32,
    progress: &mut dyn Progress,
) -> Result<()> {
    let f = File::create(out).context("open output")?;
    let mut f = BufWriter::with_capacity(1024 * 1024, f);

    let end_lba = start + blocks;
    let mut current_lba = start;

    while current_lba < end_lba {
        let (ret, read_bytes) = client.read_with_ret(CMD_EMMC_READ, current_lba, EMMC_BLOCK_BYTES)?;
        if ret != 0 {
            bail!("emmc block read failed at lba {current_lba}: status {ret}");
        }
        let read_bytes = read_bytes.context("missing data buffer")?;
        if read_bytes.len() != EMMC_BLOCK_BYTES {
            bail!(
                "emmc block read mismatch at {current_lba}: expected {EMMC_BLOCK_BYTES}, got {}",
                read_bytes.len()
            );
        }
        f.write_all(&read_bytes)?;
        current_lba += 1;
        if ((current_lba - start) & 0x3FF) == 0 || current_lba == end_lba {
            let done = current_lba - start;
            plog!(progress, "read {done}/{blocks} blocks");
            progress.update(done as u64, blocks as u64);
        }
    }

    let _ = client.cmd_void(CMD_START_SMC, 0);
    f.flush().context("flush output")?;
    Ok(())
}

fn write_emmc(
    client: &mut Client,
    input: PathBuf,
    start: u32,
    progress: &mut dyn Progress,
) -> Result<()> {
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
            plog!(progress, "written {i}/{blocks} blocks");
            progress.update(i as u64, blocks as u64);
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
                plog!(progress, "written {i}/{blocks} blocks");
                progress.update(i as u64, blocks as u64);
            }
        }
    }

    let _ = client.cmd_void(CMD_START_SMC, 0);
    Ok(())
}

// ---------------------------------------------------------------------------
// DemoN helpers
// ---------------------------------------------------------------------------

fn demon_list(progress: &mut dyn Progress) -> Result<()> {
    use crate::demon::usb::UsbClient;
    match UsbClient::open() {
        Ok(_) => plog!(progress, "DemoN device found"),
        Err(e) => plog!(progress, "DemoN device not found: {e}"),
    }
    Ok(())
}

fn demon_info(progress: &mut dyn Progress) -> Result<()> {
    let mut client = DemonClient::open().context("Failed to open DemoN device")?;
    let info = client.init().context("Failed to initialize DemoN device")?;

    plog!(progress, "DemoN Device Information:");
    plog!(progress, "  Device ID: {:?}", info.device_id);
    plog!(progress, "  Protocol Version: 0x{:04x}", info.protocol_version);
    plog!(progress, "  Firmware Version: 0x{:04x}", info.firmware_version);
    plog!(progress, "  Flash ID: 0x{:04x}", info.nand_id);
    plog!(progress, "  Mode: {:?}", info.mode);

    if let Some(manufacturer) = client.get_manufacturer_name() {
        plog!(progress, "  Manufacturer: {}", manufacturer);
    }

    if let Some(nand_info) = client.get_nand_info() {
        plog!(progress, "  NAND Info:");
        plog!(progress, "    Name: {}", nand_info.name);
        plog!(progress, "    Page Size: {} bytes", nand_info.page_size);
        plog!(progress, "    Spare Size: {} bytes", nand_info.spare_size);
        plog!(progress, "    Chip Size: {} MiB", nand_info.chip_size);
        plog!(progress, "    Pages Per Block: {}", nand_info.pages_per_block);
        plog!(progress, "    Total Blocks: {}", nand_info.num_blocks());
        plog!(
            progress,
            "    Total File Size: {} bytes (0x{:x})",
            nand_info.file_size(),
            nand_info.file_size()
        );
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// LPC / XFlash helpers
// ---------------------------------------------------------------------------

fn lpc_list(progress: &mut dyn Progress) -> Result<()> {
    use crate::lpc::usb::UsbClient;
    match UsbClient::open() {
        Ok(_) => plog!(progress, "LPC/XFlash device found"),
        Err(e) => plog!(progress, "LPC/XFlash device not found: {e}"),
    }
    Ok(())
}

fn lpc_info(progress: &mut dyn Progress) -> Result<()> {
    let mut client = LpcClient::open().context("Failed to open LPC/XFlash device")?;
    client
        .init()
        .context("Failed to initialize LPC/XFlash device")?;

    let version = client.version.unwrap_or(0);
    plog!(progress, "LPC/XFlash Device Information:");
    plog!(progress, "  ARM Version: {}", version);

    match client.flash_init() {
        Ok(config) => {
            plog!(progress, "  Flash Config: 0x{:08X}", config.raw);
            plog!(progress, "  Controller Type: {}", config.controller_type);
            plog!(progress, "  Block Type: {}", config.block_type);
            plog!(progress, "  Page Size: 0x{:X} bytes", config.page_size);
            plog!(progress, "  Meta Size: 0x{:X} bytes", config.meta_size);
            plog!(progress, "  Meta Type: {}", config.meta_type);
            plog!(progress, "  Block Size: 0x{:X} bytes", config.block_size);
            plog!(progress, "  Size Blocks: 0x{:X}", config.size_blocks);
            plog!(progress, "  Size Small Blocks: 0x{:X}", config.size_small_blocks);
            plog!(progress, "  File Blocks: 0x{:X}", config.file_blocks);
            plog!(
                progress,
                "  Full File Size: 0x{:X} bytes ({} MB)",
                config.file_size(),
                config.file_size() / (1024 * 1024)
            );
            client.flash_deinit()?;
        }
        Err(e) => {
            plog!(progress, "  Failed to initialize flash: {e}");
        }
    }

    Ok(())
}
