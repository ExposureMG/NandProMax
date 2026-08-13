use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub mod commands;
pub mod demon;
pub mod flasher;
pub mod interface;
pub mod lpc;
pub mod picoflasher;
pub mod probe;
pub mod progress;
pub mod tcp;
pub mod types;
pub mod verify;
pub mod xsvf;

use crate::progress::{Progress, StderrProgress};
use crate::types::{AdapterType, DeviceType, MediaType};

// ---------------------------------------------------------------------------
// C-compatible Enums
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NandProDeviceC {
    Auto = 0,
    Picoflasher = 1,
    Lpc = 3,
    Jrp = 4,
    Demon = 5,
    Esp = 6,
}

impl NandProDeviceC {
    pub fn to_rust(self) -> Option<DeviceType> {
        match self {
            NandProDeviceC::Auto => None,
            NandProDeviceC::Picoflasher => Some(DeviceType::Pico),
            NandProDeviceC::Lpc => Some(DeviceType::Lpc),
            NandProDeviceC::Jrp => Some(DeviceType::Jrp),
            NandProDeviceC::Demon => Some(DeviceType::Demon),
            NandProDeviceC::Esp => Some(DeviceType::Esp),
        }
    }

    pub fn from_rust(opt: Option<DeviceType>) -> Self {
        match opt {
            None => NandProDeviceC::Auto,
            Some(DeviceType::Pico) => NandProDeviceC::Picoflasher,
            Some(DeviceType::Lpc) => NandProDeviceC::Lpc,
            Some(DeviceType::Jrp) => NandProDeviceC::Jrp,
            Some(DeviceType::Demon) => NandProDeviceC::Demon,
            Some(DeviceType::Esp) => NandProDeviceC::Esp,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NandProAdapterC {
    Auto = 0,
    Usb = 1,
    Tcp = 2,
}

impl NandProAdapterC {
    pub fn to_rust(self) -> Option<AdapterType> {
        match self {
            NandProAdapterC::Auto => None,
            NandProAdapterC::Usb => Some(AdapterType::Usb),
            NandProAdapterC::Tcp => Some(AdapterType::Tcp),
        }
    }

    pub fn from_rust(opt: Option<AdapterType>) -> Self {
        match opt {
            None => NandProAdapterC::Auto,
            Some(AdapterType::Usb) => NandProAdapterC::Usb,
            Some(AdapterType::Tcp) => NandProAdapterC::Tcp,
        }
    }
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NandProMediaC {
    Auto = 0,
    Spi = 1,
    Emmc = 2,
}

impl NandProMediaC {
    pub fn to_rust(self) -> Option<MediaType> {
        match self {
            NandProMediaC::Auto => None,
            NandProMediaC::Spi => Some(MediaType::Spi),
            NandProMediaC::Emmc => Some(MediaType::Emmc),
        }
    }

    pub fn from_rust(opt: Option<MediaType>) -> Self {
        match opt {
            None => NandProMediaC::Auto,
            Some(MediaType::Spi) => NandProMediaC::Spi,
            Some(MediaType::Emmc) => NandProMediaC::Emmc,
        }
    }
}

// ---------------------------------------------------------------------------
// C-compatible Progress Callbacks
// ---------------------------------------------------------------------------

pub type LogCallbackC = Option<unsafe extern "C" fn(msg: *const c_char, user_data: *mut std::ffi::c_void)>;
pub type ProgressCallbackC = Option<unsafe extern "C" fn(done: u64, total: u64, user_data: *mut std::ffi::c_void)>;

#[repr(C)]
pub struct ProgressC {
    pub log_fn: LogCallbackC,
    pub update_fn: ProgressCallbackC,
    pub user_data: *mut std::ffi::c_void,
}

struct CProgress {
    log_fn: LogCallbackC,
    update_fn: ProgressCallbackC,
    user_data: *mut std::ffi::c_void,
}

impl Progress for CProgress {
    fn log(&mut self, msg: &str) {
        if let Some(f) = self.log_fn {
            if let Ok(c_msg) = std::ffi::CString::new(msg) {
                unsafe { f(c_msg.as_ptr(), self.user_data); }
            }
        }
    }

    fn update(&mut self, done: u64, total: u64) {
        if let Some(f) = self.update_fn {
            unsafe { f(done, total, self.user_data); }
        }
    }
}

fn wrap_progress<'a>(p: *const ProgressC) -> Box<dyn Progress + 'a> {
    if p.is_null() {
        Box::new(StderrProgress)
    } else {
        unsafe {
            Box::new(CProgress {
                log_fn: (*p).log_fn,
                update_fn: (*p).update_fn,
                user_data: (*p).user_data,
            })
        }
    }
}

unsafe fn cstr_to_option_string(ptr: *const c_char) -> Option<String> {
    if ptr.is_null() {
        None
    } else {
        CStr::from_ptr(ptr).to_str().ok().map(|s| s.to_string())
    }
}

unsafe fn cstr_to_string_or_default(ptr: *const c_char, default: &str) -> String {
    if ptr.is_null() {
        default.to_string()
    } else {
        CStr::from_ptr(ptr).to_str().unwrap_or(default).to_string()
    }
}

// ---------------------------------------------------------------------------
// C API Exported Functions (commands.rs over cdylib)
// ---------------------------------------------------------------------------

/// Read NAND or eMMC flash using command handler settings.
/// Returns 0 on success, -1 on invalid argument, -2 on execution error.
#[no_mangle]
pub unsafe extern "C" fn nandpromax_cmd_read_nand(
    out_path: *const c_char,
    device: NandProDeviceC,
    media_type: NandProMediaC,
    start: u32,
    count: u32,
    count_has_val: bool,
    serial: *const c_char,
    addr: *const c_char,
    timeout_ms: u64,
    progress: *const ProgressC,
) -> i32 {
    if out_path.is_null() {
        return -1;
    }
    let path_str = match CStr::from_ptr(out_path).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let out = PathBuf::from(path_str);
    let dev = device.to_rust();
    let med = media_type.to_rust();
    let cnt = if count_has_val { Some(count) } else { None };
    let ser = cstr_to_option_string(serial);
    let ad = cstr_to_string_or_default(addr, "192.168.4.1:3232");
    let timeout = if timeout_ms == 0 { 3000 } else { timeout_ms };
    let mut prog = wrap_progress(progress);

    match commands::cmd_read_nand(
        out, dev, med, start, cnt, ser, ad, timeout, prog.as_mut(),
    ) {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// Write NAND or eMMC flash using command handler settings.
/// Returns 0 on success, -1 on invalid argument, -2 on execution error.
#[no_mangle]
pub unsafe extern "C" fn nandpromax_cmd_write_nand(
    input_path: *const c_char,
    device: NandProDeviceC,
    media_type: NandProMediaC,
    start: u32,
    count: u32,
    count_has_val: bool,
    erase: bool,
    verify: bool,
    serial: *const c_char,
    addr: *const c_char,
    timeout_ms: u64,
    progress: *const ProgressC,
) -> i32 {
    if input_path.is_null() {
        return -1;
    }
    let path_str = match CStr::from_ptr(input_path).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let input = PathBuf::from(path_str);
    let dev = device.to_rust();
    let med = media_type.to_rust();
    let cnt = if count_has_val { Some(count) } else { None };
    let ser = cstr_to_option_string(serial);
    let ad = cstr_to_string_or_default(addr, "192.168.4.1:3232");
    let timeout = if timeout_ms == 0 { 3000 } else { timeout_ms };
    let mut prog = wrap_progress(progress);

    match commands::cmd_write_nand(
        input, dev, med, start, cnt, erase, verify, ser, ad, timeout, prog.as_mut(),
    ) {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// Output information about the target device.
/// Returns 0 on success, negative value on error.
#[no_mangle]
pub unsafe extern "C" fn nandpromax_cmd_info(
    device: NandProDeviceC,
    serial: *const c_char,
    addr: *const c_char,
    timeout_ms: u64,
    progress: *const ProgressC,
) -> i32 {
    let dev = device.to_rust();
    let ser = cstr_to_option_string(serial);
    let ad = cstr_to_string_or_default(addr, "192.168.4.1:3232");
    let timeout = if timeout_ms == 0 { 3000 } else { timeout_ms };
    let mut prog = wrap_progress(progress);

    match commands::cmd_info(dev, ser, ad, timeout, prog.as_mut()) {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// List available connected devices across LPC and DemoN backends.
/// Returns 0 on success, negative value on error.
#[no_mangle]
pub unsafe extern "C" fn nandpromax_cmd_list_devices(
    progress: *const ProgressC,
) -> i32 {
    let mut prog = wrap_progress(progress);
    match commands::cmd_list_devices(prog.as_mut()) {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// Detect LPC/XFlash device information for XSVF programming.
/// Returns 0 on success, negative value on error.
#[no_mangle]
pub unsafe extern "C" fn nandpromax_cmd_xsvf_detect(
    device: NandProDeviceC,
    progress: *const ProgressC,
) -> i32 {
    let dev = device.to_rust();
    let mut prog = wrap_progress(progress);

    match commands::cmd_xsvf_detect(dev, prog.as_mut()) {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// Program XSVF file to target CPLD / LPC device.
/// Returns 0 on success, negative value on error.
#[no_mangle]
pub unsafe extern "C" fn nandpromax_cmd_xsvf_write(
    input_path: *const c_char,
    device: NandProDeviceC,
    progress: *const ProgressC,
) -> i32 {
    if input_path.is_null() {
        return -1;
    }
    let path_str = match CStr::from_ptr(input_path).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let input = PathBuf::from(path_str);
    let dev = device.to_rust();
    let mut prog = wrap_progress(progress);

    match commands::cmd_xsvf_write(input, dev, prog.as_mut()) {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// Start TCP device server using specified backend on bind address.
/// Returns 0 on success, negative value on error.
#[no_mangle]
pub unsafe extern "C" fn nandpromax_cmd_serve_tcp(
    bind_addr: *const c_char,
    device: NandProDeviceC,
    progress: *const ProgressC,
) -> i32 {
    if bind_addr.is_null() {
        return -1;
    }
    let bind_str = match CStr::from_ptr(bind_addr).to_str() {
        Ok(s) => s,
        Err(_) => return -1,
    };
    let dev = device.to_rust();
    let mut prog = wrap_progress(progress);

    match commands::cmd_serve_tcp(bind_str.to_string(), dev, prog.as_mut()) {
        Ok(()) => 0,
        Err(_) => -2,
    }
}

/// Perform device auto-detection logic.
/// Returns 0 on success and populates out_device, out_adapter, out_media.
#[no_mangle]
pub unsafe extern "C" fn nandpromax_auto_detect_device(
    user_device: NandProDeviceC,
    user_adapter: NandProAdapterC,
    user_media: NandProMediaC,
    serial: *const c_char,
    addr: *const c_char,
    timeout_ms: u64,
    out_device: *mut NandProDeviceC,
    out_adapter: *mut NandProAdapterC,
    out_media: *mut NandProMediaC,
) -> i32 {
    let dev = user_device.to_rust();
    let ad = user_adapter.to_rust();
    let med = user_media.to_rust();
    let ser = cstr_to_option_string(serial);
    let addr_str = cstr_to_string_or_default(addr, "192.168.4.1:3232");
    let timeout = Duration::from_millis(if timeout_ms == 0 { 3000 } else { timeout_ms });

    match commands::auto_detect_device(dev, ad, med, ser.as_deref(), &addr_str, timeout) {
        Ok((res_dev, res_ad, res_med)) => {
            if !out_device.is_null() {
                *out_device = NandProDeviceC::from_rust(Some(res_dev));
            }
            if !out_adapter.is_null() {
                *out_adapter = NandProAdapterC::from_rust(Some(res_ad));
            }
            if !out_media.is_null() {
                *out_media = NandProMediaC::from_rust(Some(res_med));
            }
            0
        }
        Err(_) => -2,
    }
}

// ---------------------------------------------------------------------------
// Legacy C API Entry Points (Backwards Compatibility)
// ---------------------------------------------------------------------------

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
    let t0 = Instant::now();
    let is_tcp = adapter == NandProAdapterC::Tcp;
    let serial_ptr = if is_tcp { std::ptr::null() } else { serial_or_addr };
    let addr_ptr = if is_tcp { serial_or_addr } else { std::ptr::null() };

    let res = nandpromax_cmd_read_nand(
        out_path,
        device,
        media,
        start,
        count,
        count_has_val,
        serial_ptr,
        addr_ptr,
        0,
        std::ptr::null(),
    );

    if res == 0 && !elapsed_secs_out.is_null() {
        *elapsed_secs_out = t0.elapsed().as_secs_f64();
    }
    res
}

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
    let t0 = Instant::now();
    let is_tcp = adapter == NandProAdapterC::Tcp;
    let serial_ptr = if is_tcp { std::ptr::null() } else { serial_or_addr };
    let addr_ptr = if is_tcp { serial_or_addr } else { std::ptr::null() };

    let res = nandpromax_cmd_write_nand(
        input_path,
        device,
        media,
        start,
        count,
        count_has_val,
        erase,
        verify,
        serial_ptr,
        addr_ptr,
        0,
        std::ptr::null(),
    );

    if res == 0 && !elapsed_secs_out.is_null() {
        *elapsed_secs_out = t0.elapsed().as_secs_f64();
    }
    res
}
