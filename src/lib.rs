use std::os::raw::c_char;

#[cfg(feature = "libftdi")]
mod ftdi;
#[cfg(feature = "libftdi")]
mod interface;

#[repr(C)]
pub enum FtdiPageFormatC {
	Auto = 0,
	Small = 1,
	Big = 2,
}

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
	#[cfg(not(feature = "libftdi"))]
	{
		let _ = (
			out_path,
			start,
			count,
			count_has_val,
			page_format,
			ftdi_desc,
			ftdi_index,
			ftdi_index_has_val,
			freq_hz,
			elapsed_secs_out,
		);
		-1
	}

	#[cfg(feature = "libftdi")]
	{
		use std::ffi::CStr;
		use std::path::PathBuf;

		if out_path.is_null() {
			return -1;
		}

		let path_str = match CStr::from_ptr(out_path).to_str() {
			Ok(s) => s,
			Err(_) => return -1,
		};
		let out = PathBuf::from(path_str);

		let desc_str = if ftdi_desc.is_null() {
			"auto"
		} else {
			match CStr::from_ptr(ftdi_desc).to_str() {
				Ok(s) => s,
				Err(_) => return -1,
			}
		};

		let count_opt = if count_has_val { Some(count) } else { None };
		let index_opt = if ftdi_index_has_val { Some(ftdi_index) } else { None };

		let format = match page_format {
			FtdiPageFormatC::Auto => interface::cli::FtdiPageFormat::Auto,
			FtdiPageFormatC::Small => interface::cli::FtdiPageFormat::Small,
			FtdiPageFormatC::Big => interface::cli::FtdiPageFormat::Big,
		};

		match ftdi_read_nand_impl(out, start, count_opt, format, desc_str, index_opt, freq_hz) {
			Ok(duration) => {
				if !elapsed_secs_out.is_null() {
					*elapsed_secs_out = duration.as_secs_f64();
				}
				0
			}
			Err(_) => -2,
		}
	}
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
	#[cfg(not(feature = "libftdi"))]
	{
		let _ = (
			input_path,
			start,
			count,
			count_has_val,
			page_format,
			ftdi_desc,
			ftdi_index,
			ftdi_index_has_val,
			freq_hz,
			erase,
			verify,
			elapsed_secs_out,
		);
		-1
	}

	#[cfg(feature = "libftdi")]
	{
		use std::ffi::CStr;
		use std::path::PathBuf;

		if input_path.is_null() {
			return -1;
		}

		let path_str = match CStr::from_ptr(input_path).to_str() {
			Ok(s) => s,
			Err(_) => return -1,
		};
		let input = PathBuf::from(path_str);

		let desc_str = if ftdi_desc.is_null() {
			"auto"
		} else {
			match CStr::from_ptr(ftdi_desc).to_str() {
				Ok(s) => s,
				Err(_) => return -1,
			}
		};

		let count_opt = if count_has_val { Some(count) } else { None };
		let index_opt = if ftdi_index_has_val { Some(ftdi_index) } else { None };

		let format = match page_format {
			FtdiPageFormatC::Auto => interface::cli::FtdiPageFormat::Auto,
			FtdiPageFormatC::Small => interface::cli::FtdiPageFormat::Small,
			FtdiPageFormatC::Big => interface::cli::FtdiPageFormat::Big,
		};

		match ftdi_write_nand_impl(
			input, start, count_opt, format, desc_str, index_opt, freq_hz, erase, verify,
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
}

#[cfg(feature = "libftdi")]
fn ftdi_read_nand_impl(
	out: std::path::PathBuf,
	start: u32,
	count: Option<u32>,
	page_format: interface::cli::FtdiPageFormat,
	ftdi_desc: &str,
	ftdi_index: Option<i32>,
	freq_hz: u32,
) -> anyhow::Result<std::time::Duration> {
	use std::fs::File;
	use std::io::{BufWriter, Write};

	use anyhow::{bail, Context};

	use crate::ftdi::spi::{sfc_init, xnand_clear_status, xnand_read_page_raw, XSpi};

	let mut xspi = XSpi::open(ftdi_desc, ftdi_index, freq_hz)?;
	xspi.enter_flash_mode()?;

	let flash_config = xspi.read_u32(0x00)?;
	let geom = sfc_init(flash_config)?;
	let total_small_pages = geom.pages_count_in_nand;

	let use_big_pages = match page_format {
		interface::cli::FtdiPageFormat::Auto => geom.large_block != 0,
		interface::cli::FtdiPageFormat::Small => false,
		interface::cli::FtdiPageFormat::Big => true,
	};

	let (start_small, pages_small) = if use_big_pages {
		if total_small_pages % 4 != 0 {
			bail!("NAND page count not divisible by 4; cannot use big-page format");
		}
		let total_big_pages = total_small_pages / 4;
		let pages_big = count.unwrap_or(total_big_pages.saturating_sub(start));
		(start * 4, pages_big * 4)
	} else {
		let pages = count.unwrap_or(total_small_pages.saturating_sub(start));
		(start, pages)
	};

	let f = File::create(out).context("open output")?;
	let mut f = BufWriter::with_capacity(1024 * 1024, f);

	let t0 = std::time::Instant::now();
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
	}

	f.flush().context("flush output")?;
	Ok(t0.elapsed())
}

#[cfg(feature = "libftdi")]
fn ftdi_write_nand_impl(
	input: std::path::PathBuf,
	start: u32,
	count: Option<u32>,
	page_format: interface::cli::FtdiPageFormat,
	ftdi_desc: &str,
	ftdi_index: Option<i32>,
	freq_hz: u32,
	erase: bool,
	verify: bool,
) -> anyhow::Result<std::time::Duration> {
	use std::fs::File;
	use std::io::{BufReader, Read};

	use anyhow::{bail, Context};

	use crate::ftdi::spi::{
		sfc_init, xnand_clear_status, xnand_erase_block, xnand_read_page_raw, xnand_write_page_raw,
		XSpi,
	};

	let input_meta = std::fs::metadata(&input).context("stat input")?;
	let input_len = input_meta.len() as usize;

	let mut xspi = XSpi::open(ftdi_desc, ftdi_index, freq_hz)?;
	xspi.enter_flash_mode()?;

	let flash_config = xspi.read_u32(0x00)?;
	let geom = sfc_init(flash_config)?;
	let total_small_pages = geom.pages_count_in_nand;

	let use_big_pages = match page_format {
		interface::cli::FtdiPageFormat::Auto => geom.large_block != 0,
		interface::cli::FtdiPageFormat::Small => false,
		interface::cli::FtdiPageFormat::Big => true,
	};

	let input_page_bytes = if use_big_pages { 0x840usize } else { 0x210usize };

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
		bail!("requested range out of range");
	}

	let f = File::open(input).context("open input file")?;
	let mut f = BufReader::with_capacity(1024 * 1024, f);
	let t0 = std::time::Instant::now();

	let mut page_buf = [0u8; 0x210];
	let mut big_buf = vec![0u8; 0x840];
	let mut verify_buf = [0u8; 0x210];

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
			xnand_read_page_raw(&mut xspi, page, &mut verify_buf)
				.with_context(|| format!("verify read page {page}"))?;
			if verify_buf != page_buf {
				bail!("verify failed at page {page}");
			}
		}
	}

	Ok(t0.elapsed())
}
