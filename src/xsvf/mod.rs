pub mod sys;

use std::path::Path;

use anyhow::{Context, Result};

use crate::progress::Progress;
use crate::xsvf::sys::LibxsvfMode;

pub fn play_file_ftdi(
    input_path: &Path,
    ftdi_desc: &str,
    ftdi_index: Option<i32>,
    freq_hz: u32,
    progress: &mut dyn Progress,
) -> Result<()> {
    let data = std::fs::read(input_path)
        .with_context(|| format!("read SVF/XSVF input file {:?}", input_path))?;

    let is_svf = input_path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("svf"))
        .unwrap_or(false);

    let mode = if is_svf {
        LibxsvfMode::Svf
    } else {
        LibxsvfMode::Xsvf
    };

    let mode_str = if is_svf { "SVF" } else { "XSVF" };
    progress.log(&format!("Loaded {} bytes from {:?} (Mode: {mode_str})", data.len(), input_path));

    let mut player = crate::ftdi::jtag::FtdiJtagPlayer::new(data, ftdi_desc, ftdi_index, freq_hz, progress);
    player.play(mode)?;
    progress.log(&format!("{mode_str} programming completed successfully"));
    Ok(())
}
