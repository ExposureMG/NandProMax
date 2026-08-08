use std::ffi::CStr;
use std::os::raw::{c_char, c_int, c_long, c_ulong, c_void};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use ftdi_embedded_hal::ftdi_mpsse::MpsseCmdExecutor;

use crate::ftdi::gpio::Device;
use crate::progress::Progress;
use crate::xsvf::sys::{
    libxsvf_play, LibxsvfHost, LibxsvfMem, LibxsvfMode, LibxsvfTapState,
};

const BUFFER_SIZE: usize = 1024 * 16;

#[derive(Clone, Copy, Default)]
struct BufferElement {
    tms: u8,
    tdi: u8,
    tdi_enable: u8,
    tdo: u8,
    tdo_enable: u8,
    rmask: u8,
}

pub struct FtdiJtagPlayer<'a> {
    data: Vec<u8>,
    data_pos: usize,
    device: Option<Device>,
    buffer: Vec<BufferElement>,
    buffer_i: usize,
    last_tms: i32,
    last_tdo: i32,
    error_rc: i32,
    desc: String,
    ftdi_index: Option<i32>,
    freq_hz: u32,
    progress: &'a mut dyn Progress,
}

impl<'a> FtdiJtagPlayer<'a> {
    pub fn new(
        data: Vec<u8>,
        desc: &str,
        ftdi_index: Option<i32>,
        freq_hz: u32,
        progress: &'a mut dyn Progress,
    ) -> Self {
        Self {
            data,
            data_pos: 0,
            device: None,
            buffer: vec![BufferElement::default(); BUFFER_SIZE],
            buffer_i: 0,
            last_tms: -1,
            last_tdo: -1,
            error_rc: 0,
            desc: desc.to_string(),
            ftdi_index,
            freq_hz,
            progress,
        }
    }

    pub fn play(&mut self, mode: LibxsvfMode) -> Result<()> {
        let mut host = LibxsvfHost {
            setup: Some(h_setup),
            shutdown: Some(h_shutdown),
            udelay: Some(h_udelay),
            getbyte: Some(h_getbyte),
            sync: Some(h_sync),
            pulse_tck: Some(h_pulse_tck),
            pulse_sck: None,
            set_trst: None,
            set_frequency: Some(h_set_frequency),
            report_tapstate: Some(h_report_tapstate),
            report_device: Some(h_report_device),
            report_status: Some(h_report_status),
            report_error: Some(h_report_error),
            realloc: Some(h_realloc),
            tap_state: LibxsvfTapState::Init,
            user_data: self as *mut Self as *mut c_void,
        };

        let rc = unsafe { libxsvf_play(&mut host as *mut LibxsvfHost, mode) };
        if rc != 0 || self.error_rc != 0 {
            bail!("SVF/XSVF playback failed (rc={}, error_rc={})", rc, self.error_rc);
        }
        Ok(())
    }

    fn setup(&mut self) -> c_int {
        let dev = if let Some(index) = self.ftdi_index {
            match Device::with_index(index) {
                Ok(d) => d,
                Err(e) => {
                    self.progress.log(&format!("Failed to open FTDI index {index}: {e:#}"));
                    return -1;
                }
            }
        } else {
            match Device::with_description(&self.desc) {
                Ok(d) => d,
                Err(_) => match Device::with_index(0) {
                    Ok(d) => d,
                    Err(e) => {
                        self.progress.log(&format!("Failed to open FTDI device: {e:#}"));
                        return -1;
                    }
                },
            }
        };

        self.device = Some(dev);
        let dev = self.device.as_mut().unwrap();

        // Exact pin configuration matching xsvftool-ft232h.c / xsvfplay_ftd2xx.c:
        // Divisor calculation for initial frequency (2 MHz):
        // 0x8B: 60 MHz master clock (no divide by 5)
        // 0x97: Disable adaptive clocking
        // 0x8D: Disable 3-phase clocking
        // 0x86, 0x02, 0x00: initial clock frequency (2 MHz)
        // 0x80, 0x08, 0x1b: initial line states (TMS=high, TCK=low, TDI=low; Directions: TCK, TDI, TMS, ADBUS4 output)
        // 0x85: disable loopback
        let init_commands: [u8; 10] = [
            0x8B, 0x97, 0x8D,
            0x86, 0x02, 0x00,
            0x80, 0x08, 0x1B,
            0x85,
        ];

        if dev.send(&init_commands).is_err() {
            self.progress.log("IO Error: FTDI MPSSE setup failed");
            return -1;
        }

        if self.freq_hz > 0 {
            self.set_frequency(self.freq_hz as c_int);
        }

        0
    }

    fn shutdown(&mut self) -> c_int {
        self.sync();
        self.error_rc
    }

    fn udelay(&mut self, usecs: c_long, _tms: c_int, _num_tck: c_long) {
        self.sync();
        if usecs > 0 {
            std::thread::sleep(Duration::from_micros(usecs as u64));
        }
    }

    fn getbyte(&mut self) -> c_int {
        if self.data_pos < self.data.len() {
            let b = self.data[self.data_pos];
            self.data_pos += 1;
            if (self.data_pos & 0xFFF) == 0 || self.data_pos == self.data.len() {
                self.progress.update(self.data_pos as u64, self.data.len() as u64);
            }
            b as c_int
        } else {
            -1
        }
    }

    fn sync(&mut self) -> c_int {
        if self.buffer_i == 0 {
            return self.error_rc;
        }

        let mut ftdibuf = Vec::with_capacity(4096);
        let mut expected_tdo = Vec::with_capacity(512);

        let mut i = 0;
        while i < self.buffer_i {
            let el = &self.buffer[i];

            // If we have a sequence of shift operations
            if el.tdi_enable != 0 || el.tdo_enable != 0 {
                let start_idx = i;
                let mut count = 0;
                while i < self.buffer_i && (self.buffer[i].tdi_enable != 0 || self.buffer[i].tdo_enable != 0) && count < 2048 {
                    count += 1;
                    i += 1;
                }

                let byte_count = count / 8;
                let bit_count = count % 8;

                if byte_count > 0 {
                    let opcode = if el.tdo_enable != 0 { 0x3B } else { 0x1B };
                    let len_val = (byte_count - 1) as u16;
                    ftdibuf.push(opcode);
                    ftdibuf.push((len_val & 0xFF) as u8);
                    ftdibuf.push((len_val >> 8) as u8);

                    for b in 0..byte_count {
                        let mut byte_val = 0u8;
                        let mut mask_val = 0u8;
                        let mut exp_val = 0u8;
                        for bit in 0..8 {
                            let item = &self.buffer[start_idx + b * 8 + bit];
                            if item.tdi != 0 {
                                byte_val |= 1 << bit;
                            }
                            if item.tdo_enable != 0 {
                                mask_val |= item.rmask << bit;
                                if item.tdo != 0 {
                                    exp_val |= 1 << bit;
                                }
                            }
                        }
                        ftdibuf.push(byte_val);
                        if el.tdo_enable != 0 {
                            expected_tdo.push((exp_val, mask_val));
                        }
                    }
                }

                if bit_count > 0 {
                    let opcode = if el.tdo_enable != 0 { 0x39 } else { 0x19 };
                    ftdibuf.push(opcode);
                    ftdibuf.push((bit_count - 1) as u8);

                    let mut byte_val = 0u8;
                    let mut mask_val = 0u8;
                    let mut exp_val = 0u8;
                    let bit_offset = byte_count * 8;
                    for bit in 0..bit_count {
                        let item = &self.buffer[start_idx + bit_offset + bit];
                        if item.tdi != 0 {
                            byte_val |= 1 << bit;
                        }
                        if item.tdo_enable != 0 {
                            mask_val |= item.rmask << bit;
                            if item.tdo != 0 {
                                exp_val |= 1 << bit;
                            }
                        }
                    }
                    ftdibuf.push(byte_val);
                    if el.tdo_enable != 0 {
                        expected_tdo.push((exp_val, mask_val));
                    }
                }
            } else {
                // TMS step operation (opcode 0x4B)
                let tms_val = if el.tms != 0 { 1 } else { 0 };
                let tdi_val = if el.tdi != 0 { 0x80 } else { 0 };
                ftdibuf.push(0x4B);
                ftdibuf.push(0); // 1 bit (length - 1 = 0)
                ftdibuf.push(tms_val | tdi_val);
                i += 1;
            }
        }

        // Send Immediate command
        ftdibuf.push(0x87);

        if let Some(dev) = self.device.as_mut() {
            if dev.send(&ftdibuf).is_err() {
                self.error_rc = -1;
                self.buffer_i = 0;
                return -1;
            }

            if !expected_tdo.is_empty() {
                let mut recv_buf = vec![0u8; expected_tdo.len()];
                if dev.recv(&mut recv_buf).is_err() {
                    self.error_rc = -1;
                    self.buffer_i = 0;
                    return -1;
                }

                for (idx, (exp, mask)) in expected_tdo.iter().enumerate() {
                    let got = recv_buf[idx];
                    if (got & mask) != (*exp & mask) {
                        self.progress.log(&format!(
                            "TDO Mismatch at byte {idx}: expected 0x{:02X}, got 0x{:02X} (mask 0x{:02X})",
                            exp, got, mask
                        ));
                        self.error_rc = -1;
                        break;
                    }
                }
            }
        }

        self.buffer_i = 0;
        self.error_rc
    }

    fn pulse_tck(
        &mut self,
        tms: c_int,
        tdi: c_int,
        tdo: c_int,
        rmask: c_int,
        sync: c_int,
    ) -> c_int {
        if self.buffer_i >= BUFFER_SIZE {
            self.sync();
        }

        let el = &mut self.buffer[self.buffer_i];
        el.tms = if tms > 0 { 1 } else { 0 };
        el.tdi = if tdi > 0 { 1 } else { 0 };
        el.tdi_enable = if tdi >= 0 { 1 } else { 0 };
        el.tdo = if tdo > 0 { 1 } else { 0 };
        el.tdo_enable = if tdo >= 0 { 1 } else { 0 };
        el.rmask = if rmask > 0 { 1 } else { 0 };
        self.buffer_i += 1;

        if sync != 0 {
            self.sync();
        }

        self.error_rc
    }

    fn set_frequency(&mut self, v: c_int) -> c_int {
        if v <= 0 {
            return -1;
        }

        // FT2232H MPSSE divisor calculation:
        // Clock = 60 MHz / ((1 + divisor) * 2)
        // => divisor = (60_000_000 / (2 * freq)) - 1
        let divisor = (60_000_000u32 / (2 * v as u32)).saturating_sub(1) as u16;
        let cmd: [u8; 3] = [
            0x86,
            (divisor & 0xFF) as u8,
            (divisor >> 8) as u8,
        ];

        if let Some(dev) = self.device.as_mut() {
            if dev.send(&cmd).is_ok() {
                return v;
            }
        }

        -1
    }
}

// ---------------------------------------------------------------------------
// C Callback trampoline functions for LibxsvfHost
// ---------------------------------------------------------------------------

unsafe extern "C" fn h_setup(h: *mut LibxsvfHost) -> c_int {
    let player = &mut *((*h).user_data as *mut FtdiJtagPlayer);
    player.setup()
}

unsafe extern "C" fn h_shutdown(h: *mut LibxsvfHost) -> c_int {
    let player = &mut *((*h).user_data as *mut FtdiJtagPlayer);
    player.shutdown()
}

unsafe extern "C" fn h_udelay(
    h: *mut LibxsvfHost,
    usecs: c_long,
    tms: c_int,
    num_tck: c_long,
) {
    let player = &mut *((*h).user_data as *mut FtdiJtagPlayer);
    player.udelay(usecs, tms, num_tck);
}

unsafe extern "C" fn h_getbyte(h: *mut LibxsvfHost) -> c_int {
    let player = &mut *((*h).user_data as *mut FtdiJtagPlayer);
    player.getbyte()
}

unsafe extern "C" fn h_sync(h: *mut LibxsvfHost) -> c_int {
    let player = &mut *((*h).user_data as *mut FtdiJtagPlayer);
    player.sync()
}

unsafe extern "C" fn h_pulse_tck(
    h: *mut LibxsvfHost,
    tms: c_int,
    tdi: c_int,
    tdo: c_int,
    rmask: c_int,
    sync: c_int,
) -> c_int {
    let player = &mut *((*h).user_data as *mut FtdiJtagPlayer);
    player.pulse_tck(tms, tdi, tdo, rmask, sync)
}

unsafe extern "C" fn h_set_frequency(h: *mut LibxsvfHost, v: c_int) -> c_int {
    let player = &mut *((*h).user_data as *mut FtdiJtagPlayer);
    player.set_frequency(v)
}

unsafe extern "C" fn h_report_tapstate(_h: *mut LibxsvfHost) {}

unsafe extern "C" fn h_report_device(_h: *mut LibxsvfHost, idcode: c_ulong) {
    let player = &mut *((*_h).user_data as *mut FtdiJtagPlayer);
    player.progress.log(&format!("JTAG Device ID: 0x{:08X}", idcode));
}

unsafe extern "C" fn h_report_status(_h: *mut LibxsvfHost, message: *const c_char) {
    if !message.is_null() {
        if let Ok(s) = CStr::from_ptr(message).to_str() {
            let player = &mut *((*_h).user_data as *mut FtdiJtagPlayer);
            player.progress.log(s);
        }
    }
}

unsafe extern "C" fn h_report_error(_h: *mut LibxsvfHost, _file: *const c_char, line: c_int, message: *const c_char) {
    if !message.is_null() {
        if let Ok(s) = CStr::from_ptr(message).to_str() {
            let player = &mut *((*_h).user_data as *mut FtdiJtagPlayer);
            player.progress.log(&format!("Libxsvf Error (line {line}): {s}"));
        }
    }
}

unsafe extern "C" fn h_realloc(
    _h: *mut LibxsvfHost,
    ptr: *mut c_void,
    size: c_int,
    _which: LibxsvfMem,
) -> *mut c_void {
    if size <= 0 {
        if !ptr.is_null() {
            libc::free(ptr);
        }
        std::ptr::null_mut()
    } else {
        libc::realloc(ptr, size as usize)
    }
}
