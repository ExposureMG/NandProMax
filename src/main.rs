mod demon;
mod flasher;
mod ftdi;
mod interface;
mod lpc;
mod picoflasher;
mod tcp;

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use clap::Parser;

use crate::demon::DemonClient;
use crate::flasher::{run_read_nand, run_write_nand, FlashGeometry, NandFlasher};
use crate::interface::cli::{AdapterType, Cli, Command, DeviceType, FtdiPageFormat, MediaType};
use crate::lpc::LpcClient;
use crate::picoflasher::pfc::{
    Client, CMD_EMMC_DETECT, CMD_EMMC_GET_EXT_CSD, CMD_EMMC_INIT, CMD_EMMC_READ,
    CMD_EMMC_WRITE, CMD_EMMC_WRITE_MULTI, CMD_GET_FLASH_CONFIG, CMD_GET_VERSION,
    CMD_READ_FLASH, CMD_SET_SMC_WORKAROUND, CMD_START_SMC, CMD_STOP_SMC, CMD_WRITE_FLASH,
    CMD_WRITE_FLASH_MULTI, EMMC_BLOCK_BYTES, NAND_BLOCK_BYTES,
};
use crate::tcp::TcpServer;

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::ReadNand {
            out,
            device,
            adapter,
            media_type,
            start,
            count,
            serial,
            addr,
            ftdi_desc,
            ftdi_index,
            freq_hz,
            page_format,
            timeout_ms,
        } => {
            let timeout = Duration::from_millis(timeout_ms);
            let (target_dev, target_adapter, target_media) = auto_detect_device(
                device,
                adapter,
                media_type,
                serial.as_deref(),
                &addr,
                &ftdi_desc,
                ftdi_index,
                freq_hz,
                timeout,
            )?;

            eprintln!(
                "Using device={:?} adapter={:?} type={:?}",
                target_dev, target_adapter, target_media
            );

            let elapsed = match (target_dev, target_media) {
                (DeviceType::Picoflasher, MediaType::Spi) => {
                    let (mut client, resolved) = if target_adapter == AdapterType::Tcp {
                        Client::connect_tcp(&addr, timeout)?
                    } else {
                        let port = serial.as_deref().unwrap_or("");
                        Client::connect_usb(port, timeout)?
                    };
                    eprintln!("connected to {resolved}");
                    let (flash_config, blocks_total) = prepare_nand(&mut client)?;
                    let blocks = count.unwrap_or(blocks_total.saturating_sub(start));
                    let t0 = Instant::now();
                    read_nand(&mut client, out, start, blocks)?;
                    t0.elapsed()
                }
                (DeviceType::Picoflasher, MediaType::Emmc) => {
                    let (mut client, resolved) = if target_adapter == AdapterType::Tcp {
                        Client::connect_tcp(&addr, timeout)?
                    } else {
                        let port = serial.as_deref().unwrap_or("");
                        Client::connect_usb(port, timeout)?
                    };
                    eprintln!("connected to {resolved}");
                    let blocks_total = prepare_emmc(&mut client)?;
                    let blocks = count.unwrap_or(blocks_total.saturating_sub(start));
                    let t0 = Instant::now();
                    read_emmc(&mut client, out, start, blocks)?;
                    t0.elapsed()
                }
                (DeviceType::Ftdi, _) => {
                    ftdi_read_nand(out, start, count, page_format, &ftdi_desc, ftdi_index, freq_hz)?
                }
                (DeviceType::Lpc, _) => {
                    let mut client = LpcClient::open().context("Failed to open LPC device")?;
                    run_read_nand(&mut client, out, start, count)?
                }
                (DeviceType::Demon, _) => {
                    let mut client = DemonClient::open().context("Failed to open DemoN device")?;
                    run_read_nand(&mut client, out, start, count)?
                }
            };

            println!("ok ({:.3}s)", elapsed.as_secs_f64());
        }

        Command::WriteNand {
            input,
            device,
            adapter,
            media_type,
            start,
            count,
            erase,
            verify,
            serial,
            addr,
            ftdi_desc,
            ftdi_index,
            freq_hz,
            page_format,
            timeout_ms,
        } => {
            let timeout = Duration::from_millis(timeout_ms);
            let (target_dev, target_adapter, target_media) = auto_detect_device(
                device,
                adapter,
                media_type,
                serial.as_deref(),
                &addr,
                &ftdi_desc,
                ftdi_index,
                freq_hz,
                timeout,
            )?;

            eprintln!(
                "Using device={:?} adapter={:?} type={:?}",
                target_dev, target_adapter, target_media
            );

            match (target_dev, target_media) {
                (DeviceType::Picoflasher, MediaType::Spi) => {
                    let (mut client, resolved) = if target_adapter == AdapterType::Tcp {
                        Client::connect_tcp(&addr, timeout)?
                    } else {
                        let port = serial.as_deref().unwrap_or("");
                        Client::connect_usb(port, timeout)?
                    };
                    eprintln!("connected to {resolved}");
                    let (_flash_config, _blocks_total) = prepare_nand(&mut client)?;
                    write_nand(&mut client, input, start)?;
                }
                (DeviceType::Picoflasher, MediaType::Emmc) => {
                    let (mut client, resolved) = if target_adapter == AdapterType::Tcp {
                        Client::connect_tcp(&addr, timeout)?
                    } else {
                        let port = serial.as_deref().unwrap_or("");
                        Client::connect_usb(port, timeout)?
                    };
                    eprintln!("connected to {resolved}");
                    let _blocks_total = prepare_emmc(&mut client)?;
                    write_emmc(&mut client, input, start)?;
                }
                (DeviceType::Ftdi, _) => {
                    ftdi_write_nand(
                        input, start, count, page_format, &ftdi_desc, ftdi_index, freq_hz, erase, verify,
                    )?;
                }
                (DeviceType::Lpc, _) => {
                    let mut client = LpcClient::open().context("Failed to open LPC device")?;
                    run_write_nand(&mut client, input, start)?;
                }
                (DeviceType::Demon, _) => {
                    let mut client = DemonClient::open().context("Failed to open DemoN device")?;
                    run_write_nand(&mut client, input, start)?;
                }
            }

            println!("ok");
        }

        Command::Info {
            device,
            adapter,
            serial,
            addr,
            ftdi_desc,
            ftdi_index,
            freq_hz,
            timeout_ms,
        } => {
            let timeout = Duration::from_millis(timeout_ms);
            let target_dev = device.unwrap_or(DeviceType::Picoflasher);
            match target_dev {
                DeviceType::Picoflasher => {
                    let (mut client, resolved) = if adapter == Some(AdapterType::Tcp) {
                        Client::connect_tcp(&addr, timeout)?
                    } else {
                        let port = serial.as_deref().unwrap_or("");
                        Client::connect_usb(port, timeout)?
                    };
                    eprintln!("PicoFlasher connected to {resolved}");
                    let ver = client.cmd_u32(CMD_GET_VERSION, 0)?;
                    eprintln!("PicoFlasher Firmware Version: 0x{ver:08x}");
                }
                DeviceType::Ftdi => {
                    let mut xspi = crate::ftdi::spi::XSpi::open(&ftdi_desc, ftdi_index, freq_hz)?;
                    xspi.enter_flash_mode()?;
                    let flash_config = xspi.read_u32(0x00)?;
                    let geom = crate::ftdi::spi::sfc_init(flash_config)?;
                    eprintln!("FTDI SPI Flasher Config: 0x{flash_config:08x}");
                    eprintln!("NAND Size: {} MB", geom.nand_size_mb);
                }
                DeviceType::Lpc => {
                    lpc_info()?;
                }
                DeviceType::Demon => {
                    demon_info()?;
                }
            }
            println!("ok");
        }

        Command::ListDevices => {
            eprintln!("Listing available devices:");
            eprintln!("1. Serial ports:");
            let _ = list_serial_ports();
            eprintln!("2. FTDI devices:");
            let _ = ftdi_list();
            eprintln!("3. LPC devices:");
            let _ = lpc_list();
            eprintln!("4. DemoN devices:");
            let _ = demon_list();
            println!("ok");
        }

        Command::ReadPost {
            out,
            count,
            device,
            serial,
            baud,
            quiet,
            ftdi_desc,
            ftdi_index,
            poll_us,
        } => {
            if device == Some(DeviceType::Ftdi) {
                ftdi_read_post_stream(out, count, &ftdi_desc, ftdi_index, quiet, poll_us)?;
            } else {
                let port = serial.as_deref().context("ReadPost requires --serial <PORT>")?;
                read_post_stream(port, baud, Duration::from_secs(3), out, count, quiet)?;
            }
            println!("ok");
        }

        Command::ServeTcp { bind, device } => {
            let target_dev = device.unwrap_or(DeviceType::Ftdi);
            eprintln!("Starting TCP device server using {target_dev:?} backend on {bind}...");
            match target_dev {
                DeviceType::Ftdi => {
                    bail!("Serving FTDI over TCP server requires connected FTDI hardware");
                }
                DeviceType::Lpc => {
                    let client = LpcClient::open().context("Failed to open LPC device")?;
                    let mut server = TcpServer::bind(&bind, client)?;
                    server.run()?;
                }
                DeviceType::Demon => {
                    let client = DemonClient::open().context("Failed to open DemoN device")?;
                    let mut server = TcpServer::bind(&bind, client)?;
                    server.run()?;
                }
                DeviceType::Picoflasher => {
                    bail!("PicoFlasher already functions as a TCP flasher endpoint");
                }
            }
            println!("ok");
        }
    }
    Ok(())
}

fn auto_detect_device(
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
        let adapter = user_adapter.unwrap_or(AdapterType::Usb);
        let media = user_media.unwrap_or(MediaType::Spi);
        return Ok((dev, adapter, media));
    }

    if user_adapter == Some(AdapterType::Tcp) {
        let media = user_media.unwrap_or(MediaType::Spi);
        return Ok((DeviceType::Picoflasher, AdapterType::Tcp, media));
    }

    if let Some(port) = serial {
        if Client::connect_usb(port, timeout).is_ok() {
            let media = user_media.unwrap_or(MediaType::Spi);
            return Ok((DeviceType::Picoflasher, AdapterType::Usb, media));
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

    let adapter = user_adapter.unwrap_or(AdapterType::Usb);
    let media = user_media.unwrap_or(MediaType::Spi);
    Ok((DeviceType::Picoflasher, adapter, media))
}

fn ftdi_read_post_stream(
    out: Option<PathBuf>,
    count: Option<u64>,
    ftdi_desc: &str,
    ftdi_index: Option<i32>,
    quiet: bool,
    poll_us: u64,
) -> Result<()> {
    use crate::ftdi::gpio::Device;
    use ftdi_embedded_hal::ftdi_mpsse::{MpsseCmdExecutor, MpsseSettings};
    use ftdi_embedded_hal::libftd2xx;
    use ftdi_embedded_hal::libftd2xx::FtdiCommon as _;

    let mut dev = if let Some(index) = ftdi_index {
        Device::with_index(index).with_context(|| format!("ftdi-index={index}"))?
    } else if ftdi_desc != "auto" {
        Device::with_description(ftdi_desc)
            .with_context(|| format!("open ftdi by description: {ftdi_desc:?}"))?
    } else {
        let devices = libftd2xx::list_devices().context("list ftdi devices")?;
        let mut cands: Vec<libftd2xx::DeviceInfo> = devices
            .into_iter()
            .filter(|d| d.vendor_id == 0x0403 && d.product_id == 0x6010)
            .collect();
        if cands.is_empty() {
            let num = libftd2xx::num_devices().context("query number of FTDI devices")?;
            bail!("no FTDI 0403:6010 devices found (libftd2xx sees {num} device(s))");
        }
        cands.sort_by_key(|d| score_desc(&d.description));
        let chosen = cands.first().unwrap();
        Device::with_description(&chosen.description).with_context(|| {
            format!(
                "open FTDI by auto-selected description: {:?}",
                chosen.description
            )
        })?
    };

    let settings = MpsseSettings {
        latency_timer: Duration::from_millis(1),
        in_transfer_size: 65536,
        read_timeout: Duration::from_secs(5),
        write_timeout: Duration::from_secs(5),
        ..Default::default()
    };

    MpsseCmdExecutor::init(&mut dev, &settings)
        .context("initialize mpsse")?;
    dev.inner
        .set_bit_mode(0x00, libftd2xx::BitMode::Reset)
        .context("set bitmode reset")?;
    dev.inner
        .set_bit_mode(0x00, libftd2xx::BitMode::Mpsse)
        .context("set bitmode mpsse")?;

    dev.inner
        .write_all(&[0x80, 0x00, 0x0B])
        .context("gpio dir")?;

    let poll_dur = Duration::from_micros(poll_us);
    let mut out_file = if let Some(path) = out {
        Some(BufWriter::new(
            File::create(path).context("open post output")?,
        ))
    } else {
        None
    };

    let mut total = 0u64;
    let mut last = 0xFFu8;
    loop {
        if let Some(limit) = count {
            if total >= limit {
                break;
            }
        }

        let pins = dev.inner.bit_mode().context("bit_mode")?;
        let post = pins & 0xFF;
        if post != last {
            last = post;
            total += 1;
            if !quiet {
                println!("POST: 0x{post:02X}");
            }
            if let Some(f) = out_file.as_mut() {
                f.write_all(&[post]).context("write post byte")?;
                f.flush().context("flush post byte")?;
            }
        }
        std::thread::sleep(poll_dur);
    }
    Ok(())
}

fn score_desc(desc: &str) -> i32 {
    let lower = desc.to_ascii_lowercase();
    if lower.contains("nandpro") {
        0
    } else if lower.contains("j-runner") || lower.contains("jrunner") {
        1
    } else if lower.contains("squirt") {
        2
    } else if lower.contains("tx") {
        3
    } else {
        10
    }
}

fn list_serial_ports() -> Result<()> {
    let ports = serialport::available_ports().context("list serial ports")?;
    if ports.is_empty() {
        eprintln!("no serial ports found");
        return Ok(());
    }
    for p in ports {
        eprintln!("port: {}", p.port_name);
    }
    Ok(())
}

fn read_post_stream(
    port: &str,
    baud: u32,
    timeout: Duration,
    out: Option<PathBuf>,
    count: Option<u64>,
    quiet: bool,
) -> Result<()> {
    let mut serial = serialport::new(port, baud)
        .timeout(timeout)
        .open()
        .with_context(|| format!("open serial {port}"))?;

    let mut out_file = if let Some(path) = out {
        Some(BufWriter::new(
            File::create(path).context("open post output")?,
        ))
    } else {
        None
    };

    let mut total = 0u64;
    let mut buf = [0u8; 1];
    loop {
        if let Some(limit) = count {
            if total >= limit {
                break;
            }
        }
        match serial.read_exact(&mut buf) {
            Ok(()) => {
                let post = buf[0];
                total += 1;
                if !quiet {
                    println!("POST: 0x{post:02X}");
                }
                if let Some(f) = out_file.as_mut() {
                    f.write_all(&[post]).context("write post byte")?;
                    f.flush().context("flush post byte")?;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(e) => return Err(e).context("read post serial"),
        }
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
) -> Result<Duration> {
    use crate::ftdi::spi::{sfc_init, xnand_clear_status, xnand_read_page_raw, XSpi};

    eprintln!("ftdi freq_hz={freq_hz}");
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

    eprintln!(
        "flash_config=0x{flash_config:08x} nand={}MB page_format={} start={} count={}",
        geom.nand_size_mb, unit_name, start, pages_small / if use_big_pages { 4 } else { 1 }
    );

    let f = File::create(out).context("open output")?;
    let mut f = BufWriter::with_capacity(1024 * 1024, f);

    let t0 = Instant::now();
    let mut page_buf = [0u8; 0x210];
    let mut big_buf = vec![0u8; 0x840];
    for i in 0..pages_small {
        if (i & 0xFF) == 0 {
            xnand_clear_status(&mut xspi).context("clear status")?;
        }

        let page = start_small + i;
        xnand_read_page_raw(&mut xspi, page, &mut page_buf)
            .with_context(|| format!("read page {page}"))?;

        if use_big_pages {
            let idx = (i as usize % 4) * 0x210;
            big_buf[idx..idx + 0x210].copy_from_slice(&page_buf);
            if (i & 3) == 3 {
                f.write_all(&big_buf)?;
            }
        } else {
            f.write_all(&page_buf)?;
        }

        if (i & 0x3FF) == 0 {
            let done = i + 1;
            let total = pages_small;
            eprintln!("read {done}/{total} pages");
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
) -> Result<Duration> {
    use crate::ftdi::spi::{
        sfc_init, xnand_clear_status, xnand_erase_block, xnand_read_page_raw, xnand_write_page_raw, XSpi,
    };

    let input_meta = std::fs::metadata(&input).context("stat input")?;
    let input_len = input_meta.len() as usize;

    eprintln!("ftdi freq_hz={freq_hz}");
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

    eprintln!(
        "flash_config=0x{flash_config:08x} nand={}MB page_format={} start={} pages={} erase={} verify={} (input_pages={file_pages})",
        geom.nand_size_mb, unit_name, start, pages, erase, verify
    );

    let f = File::open(input).context("open input")?;
    let mut f = BufReader::with_capacity(1024 * 1024, f);

    let t0 = Instant::now();
    let mut page_buf = [0u8; 0x210];
    let mut big_buf = vec![0u8; 0x840];
    for i in 0..pages_small {
        if (i & 0xFF) == 0 {
            xnand_clear_status(&mut xspi).context("clear status")?;
        }

        if use_big_pages {
            if (i & 3) == 0 {
                f.read_exact(&mut big_buf).context("read input page")?;
            }
            let idx = (i as usize % 4) * 0x210;
            page_buf.copy_from_slice(&big_buf[idx..idx + 0x210]);
        } else {
            f.read_exact(&mut page_buf).context("read input page")?;
        }

        let page = start_small + i;
        if erase && (page % geom.page_count_in_block) == 0 {
            xnand_erase_block(&mut xspi, page).with_context(|| format!("erase block at page {page}"))?;
        }
        xnand_write_page_raw(&mut xspi, page, &page_buf)
            .with_context(|| format!("write page {page}"))?;

        if verify {
            let mut verify_buf = [0u8; 0x210];
            xnand_read_page_raw(&mut xspi, page, &mut verify_buf)
                .with_context(|| format!("verify read page {page}"))?;
            if verify_buf != page_buf {
                let mut first = None;
                for (idx, (a, b)) in page_buf.iter().zip(verify_buf.iter()).enumerate() {
                    if a != b {
                        first = Some((idx, *a, *b));
                        break;
                    }
                }
                if let Some((idx, exp, got)) = first {
                    bail!(
                        "verify failed at page {page} (first mismatch at +0x{idx:x}: expected 0x{exp:02x}, got 0x{got:02x})"
                    );
                } else {
                    bail!("verify failed at page {page} (data mismatch)");
                }
            }
        }

        if (i & 0x3FF) == 0 {
            let done = i + 1;
            let total = pages_small;
            eprintln!("wrote {done}/{total} pages");
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
            "  [{i}] type={:?} serial={:?} desc={:?}",
            d.device_type, d.serial_number, d.description
        );
    }
    Ok(())
}

fn prepare_nand(client: &mut Client) -> Result<(u32, u32)> {
    let ver = client.cmd_u32(CMD_GET_VERSION, 0).context("GET_VERSION")?;
    eprintln!("pfc version=0x{ver:08x}");
    let _ = client.cmd_u32(CMD_STOP_SMC, 0);
    let _ = client.cmd_u32(CMD_SET_SMC_WORKAROUND, 1);
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

fn read_nand(client: &mut Client, out: PathBuf, start: u32, blocks: u32) -> Result<()> {
    let f = File::create(out).context("open output")?;
    let mut f = BufWriter::with_capacity(1024 * 1024, f);

    let end_block = start + blocks;
    let mut current_block = start;
    while current_block < end_block {
        let read_bytes =
            client.cmd_exact_bytes(CMD_READ_FLASH, current_block, NAND_BLOCK_BYTES)?;
        if read_bytes.len() != NAND_BLOCK_BYTES {
            bail!("block read mismatch at {current_block}: expected {NAND_BLOCK_BYTES}, got {}", read_bytes.len());
        }
        f.write_all(&read_bytes)?;
        current_block += 1;
        if ((current_block - start) & 0x3F) == 0 || current_block == end_block {
            eprintln!("read {}/{} blocks", current_block - start, blocks);
        }
    }

    let _ = client.cmd_u32(CMD_START_SMC, 0);

    f.flush().context("flush output")?;
    Ok(())
}

fn write_nand(client: &mut Client, input: PathBuf, start: u32) -> Result<()> {
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
            eprintln!("written {}/{} blocks", i, blocks);
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
                eprintln!("written {}/{} blocks", i, blocks);
            }
        }
    }

    let _ = client.cmd_u32(CMD_START_SMC, 0);

    Ok(())
}

fn prepare_emmc(client: &mut Client) -> Result<u32> {
    let ver = client.cmd_u32(CMD_GET_VERSION, 0).context("GET_VERSION")?;
    eprintln!("pfc version=0x{ver:08x}");
    let _ = client.cmd_u32(CMD_STOP_SMC, 0);
    let _ = client.cmd_u32(CMD_SET_SMC_WORKAROUND, 1);

    let ret = client.cmd_u32(CMD_EMMC_INIT, 0).context("EMMC_INIT")?;
    if ret != 0 {
        bail!("EMMC_INIT failed: {ret}");
    }

    let ret = client.cmd_u32(CMD_EMMC_DETECT, 0).context("EMMC_DETECT")?;
    if ret != 0 {
        bail!("EMMC_DETECT failed: {ret}");
    }

    let ext_csd = client
        .cmd_exact_bytes(CMD_EMMC_GET_EXT_CSD, 0, 512)
        .context("EMMC_GET_EXT_CSD")?;
    if ext_csd.len() != 512 {
        bail!("EXT_CSD mismatch: expected 512, got {}", ext_csd.len());
    }

    let sec_count = u32::from_le_bytes(ext_csd[212..216].try_into().unwrap());
    eprintln!("emmc sec_count={sec_count} (~{} MB)", sec_count / 2048);
    Ok(sec_count)
}

fn read_emmc(client: &mut Client, out: PathBuf, start: u32, blocks: u32) -> Result<()> {
    let f = File::create(out).context("open output")?;
    let mut f = BufWriter::with_capacity(1024 * 1024, f);

    let end_lba = start + blocks;
    let mut current_lba = start;

    while current_lba < end_lba {
        let read_bytes =
            client.cmd_exact_bytes(CMD_EMMC_READ, current_lba, EMMC_BLOCK_BYTES)?;
        if read_bytes.len() != EMMC_BLOCK_BYTES {
            bail!("emmc block read mismatch at {current_lba}: expected {EMMC_BLOCK_BYTES}, got {}", read_bytes.len());
        }
        f.write_all(&read_bytes)?;
        current_lba += 1;
        if ((current_lba - start) & 0x3FF) == 0 || current_lba == end_lba {
            eprintln!("read {}/{} blocks", current_lba - start, blocks);
        }
    }

    let _ = client.cmd_u32(CMD_START_SMC, 0);

    f.flush().context("flush output")?;
    Ok(())
}

fn write_emmc(client: &mut Client, input: PathBuf, start: u32) -> Result<()> {
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

impl NandFlasher for DemonClient {
    fn geometry(&mut self) -> Result<FlashGeometry> {
        let _info = self.init().context("Failed to initialize DemoN device")?;
        let nand_info = self
            .get_nand_info()
            .ok_or_else(|| anyhow::anyhow!("NAND device not recognized"))?;
        Ok(FlashGeometry {
            name: nand_info.name.to_string(),
            chip_size_mb: nand_info.chip_size,
            block_size: nand_info.total_block_size() as usize,
            total_blocks: nand_info.num_blocks() as u32,
        })
    }

    fn read_block(&mut self, block: u32, buf: &mut [u8]) -> Result<()> {
        let _len = self
            .read_block(block as u16, buf.len(), buf)
            .with_context(|| format!("read block {block}"))?;
        Ok(())
    }

    fn write_block(&mut self, block: u32, buf: &[u8]) -> Result<()> {
        self.write_block(block as u16, buf)
            .with_context(|| format!("write block {block}"))?;
        Ok(())
    }
}

fn demon_list() -> Result<()> {
    use crate::demon::usb::UsbClient;

    match UsbClient::open() {
        Ok(_) => {
            eprintln!("DemoN device found");
            Ok(())
        }
        Err(e) => {
            eprintln!("DemoN device not found: {e}");
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
    }

    Ok(())
}

// LPC/XFlash functions

impl NandFlasher for LpcClient {
    fn geometry(&mut self) -> Result<FlashGeometry> {
        self.init().context("Failed to initialize LPC/XFlash device")?;
        let version = self.version.unwrap_or(0);
        let config = self.flash_init().context("Failed to initialize flash")?;
        Ok(FlashGeometry {
            name: format!("LPC/XFlash (ARM v{version})"),
            chip_size_mb: (config.file_size() / (1024 * 1024)) as u32,
            block_size: 0x4200,
            total_blocks: config.size_small_blocks,
        })
    }

    fn read_block(&mut self, block: u32, buf: &mut [u8]) -> Result<()> {
        let (status, data) = self.flash_read(block)?;
        if crate::lpc::status::is_error(status) {
            bail!("Error reading block {block}: status=0x{status:X}");
        }
        if data.len() != buf.len() {
            bail!("Block read length mismatch: expected {}, got {}", buf.len(), data.len());
        }
        buf.copy_from_slice(&data);
        Ok(())
    }

    fn write_block(&mut self, block: u32, buf: &[u8]) -> Result<()> {
        let status = self.flash_write(block, buf)?;
        if crate::lpc::status::is_error(status) {
            bail!("Error writing block {block}: status=0x{status:X}");
        }
        Ok(())
    }

    fn deinit(&mut self) -> Result<()> {
        self.flash_deinit()?;
        Ok(())
    }
}

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