use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;
use std::time::{Duration, Instant};

pub mod commands;
pub mod demon;
pub mod flasher;
pub mod interface;
pub mod lpc;
pub mod picoflasher;
pub mod progress;
pub mod tcp;
pub mod types;

use crate::progress::{Progress, StderrProgress};
use crate::types::{DeviceType, MediaType};

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
}

impl NandProDeviceC {
    /// Convert a raw C integer into the enum, returning `None` for out-of-range values.
    pub fn from_c(value: c_int) -> Option<Self> {
        match value {
            0 => Some(NandProDeviceC::Auto),
            1 => Some(NandProDeviceC::Picoflasher),
            3 => Some(NandProDeviceC::Lpc),
            4 => Some(NandProDeviceC::Jrp),
            5 => Some(NandProDeviceC::Demon),
            _ => None,
        }
    }

    pub fn to_rust(self) -> Option<DeviceType> {
        match self {
            NandProDeviceC::Auto => None,
            NandProDeviceC::Picoflasher => Some(DeviceType::Pico),
            NandProDeviceC::Lpc => Some(DeviceType::Lpc),
            NandProDeviceC::Jrp => Some(DeviceType::Jrp),
            NandProDeviceC::Demon => Some(DeviceType::Demon),
        }
    }

    pub fn from_rust(opt: Option<DeviceType>) -> Self {
        match opt {
            None => NandProDeviceC::Auto,
            Some(DeviceType::Pico) => NandProDeviceC::Picoflasher,
            Some(DeviceType::Lpc) => NandProDeviceC::Lpc,
            Some(DeviceType::Jrp) => NandProDeviceC::Jrp,
            Some(DeviceType::Demon) => NandProDeviceC::Demon,
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
    /// Convert a raw C integer into the enum, returning `None` for out-of-range values.
    pub fn from_c(value: c_int) -> Option<Self> {
        match value {
            0 => Some(NandProMediaC::Auto),
            1 => Some(NandProMediaC::Spi),
            2 => Some(NandProMediaC::Emmc),
            _ => None,
        }
    }

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
// Error handling plumbing shared by every export
// ---------------------------------------------------------------------------

const RC_INVALID_ARG: i32 = -1;
const RC_EXEC_ERROR: i32 = -2;
const RC_PANIC: i32 = -3;

const DEFAULT_TIMEOUT_MS: u64 = 3000;

thread_local! {
    static LAST_ERROR: RefCell<CString> = RefCell::new(CString::default());
}

fn set_last_error(msg: &str) {
    // Interior NULs cannot be represented in a C string.
    let c_msg = CString::new(msg.replace('\0', " ")).unwrap_or_default();
    // `try_with` so a call during thread teardown cannot panic.
    let _ = LAST_ERROR.try_with(|e| *e.borrow_mut() = c_msg);
}

fn clear_last_error() {
    set_last_error("");
}

/// Failure of an exported call, before it is flattened into a return code.
enum CallError {
    Invalid(String),
    Exec(anyhow::Error),
}

impl From<anyhow::Error> for CallError {
    fn from(e: anyhow::Error) -> Self {
        CallError::Exec(e)
    }
}

fn invalid<T>(msg: impl Into<String>) -> Result<T, CallError> {
    Err(CallError::Invalid(msg.into()))
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "unknown panic".to_string()
    }
}

/// Run an export body: reset the last-error slot, catch panics, and map the
/// outcome to a return code while recording the error text.
fn guarded(body: impl FnOnce() -> Result<(), CallError>) -> i32 {
    clear_last_error();
    match catch_unwind(AssertUnwindSafe(body)) {
        Ok(Ok(())) => 0,
        Ok(Err(CallError::Invalid(msg))) => {
            set_last_error(&msg);
            RC_INVALID_ARG
        }
        Ok(Err(CallError::Exec(e))) => {
            set_last_error(&format!("{e:#}"));
            RC_EXEC_ERROR
        }
        Err(payload) => {
            set_last_error(&format!(
                "internal panic: {}",
                panic_message(payload.as_ref())
            ));
            RC_PANIC
        }
    }
}

fn device_arg(v: c_int) -> Result<Option<DeviceType>, CallError> {
    match NandProDeviceC::from_c(v) {
        Some(d) => Ok(d.to_rust()),
        None => invalid(format!("invalid device value {v}")),
    }
}

fn media_arg(v: c_int) -> Result<Option<MediaType>, CallError> {
    match NandProMediaC::from_c(v) {
        Some(m) => Ok(m.to_rust()),
        None => invalid(format!("invalid media value {v}")),
    }
}

fn timeout_or_default(timeout_ms: u64) -> u64 {
    if timeout_ms == 0 {
        DEFAULT_TIMEOUT_MS
    } else {
        timeout_ms
    }
}

// ---------------------------------------------------------------------------
// C-compatible Progress Callbacks
// ---------------------------------------------------------------------------

pub type LogCallbackC =
    Option<unsafe extern "C" fn(msg: *const c_char, user_data: *mut std::ffi::c_void)>;
pub type ProgressCallbackC =
    Option<unsafe extern "C" fn(done: u64, total: u64, user_data: *mut std::ffi::c_void)>;

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
            if let Ok(c_msg) = CString::new(msg) {
                unsafe {
                    f(c_msg.as_ptr(), self.user_data);
                }
            }
        }
    }

    fn update(&mut self, done: u64, total: u64) {
        if let Some(f) = self.update_fn {
            unsafe {
                f(done, total, self.user_data);
            }
        }
    }
}

unsafe fn wrap_progress<'a>(p: *const ProgressC) -> Box<dyn Progress + 'a> {
    if p.is_null() {
        Box::new(StderrProgress)
    } else {
        Box::new(CProgress {
            log_fn: (*p).log_fn,
            update_fn: (*p).update_fn,
            user_data: (*p).user_data,
        })
    }
}

unsafe fn cstr_to_option_string(
    ptr: *const c_char,
    what: &str,
) -> Result<Option<String>, CallError> {
    if ptr.is_null() {
        return Ok(None);
    }
    match CStr::from_ptr(ptr).to_str() {
        Ok(s) => Ok(Some(s.to_string())),
        Err(_) => invalid(format!("{what} is not valid UTF-8")),
    }
}

unsafe fn required_path(ptr: *const c_char, what: &str) -> Result<PathBuf, CallError> {
    match cstr_to_option_string(ptr, what)? {
        Some(s) => Ok(PathBuf::from(s)),
        None => invalid(format!("{what} must not be NULL")),
    }
}

// ---------------------------------------------------------------------------
// C API Exported Functions (commands.rs over cdylib)
//
// Return codes: 0 ok, -1 invalid argument, -2 execution error, -3 internal
// panic. On any non-zero return `nandpromax_last_error()` describes why.
// ---------------------------------------------------------------------------

/// Library version string (static, NUL-terminated, never NULL).
#[no_mangle]
pub extern "C" fn nandpromax_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

/// Description of the last error on the calling thread.
///
/// Never NULL: an empty string means the last call succeeded. The pointer is
/// valid until the next `nandpromax_*` call on the same thread.
#[no_mangle]
pub extern "C" fn nandpromax_last_error() -> *const c_char {
    catch_unwind(|| {
        LAST_ERROR
            .try_with(|e| e.borrow().as_ptr())
            .unwrap_or(c"".as_ptr())
    })
    .unwrap_or(c"".as_ptr())
}

/// Read NAND or eMMC flash using command handler settings.
/// Returns 0 on success, -1 on invalid argument, -2 on execution error,
/// -3 on internal panic.
///
/// # Safety
/// `out_path` must be a valid NUL-terminated string. `serial` must be NULL or
/// a valid NUL-terminated string. `progress` must be NULL or point to a valid
/// `ProgressC` whose callbacks (if set) are safe to call with its `user_data`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn nandpromax_cmd_read_nand(
    out_path: *const c_char,
    device: c_int,
    media_type: c_int,
    start: u32,
    count: u32,
    count_has_val: bool,
    serial: *const c_char,
    timeout_ms: u64,
    progress: *const ProgressC,
) -> i32 {
    guarded(|| {
        let out = required_path(out_path, "out_path")?;
        let dev = device_arg(device)?;
        let med = media_arg(media_type)?;
        let ser = cstr_to_option_string(serial, "serial")?;
        read_nand_impl(
            out,
            dev,
            med,
            start,
            count_has_val.then_some(count),
            ser,
            timeout_ms,
            progress,
        )
    })
}

#[allow(clippy::too_many_arguments)]
unsafe fn read_nand_impl(
    out: PathBuf,
    dev: Option<DeviceType>,
    med: Option<MediaType>,
    start: u32,
    count: Option<u32>,
    serial: Option<String>,
    timeout_ms: u64,
    progress: *const ProgressC,
) -> Result<(), CallError> {
    let mut prog = wrap_progress(progress);
    commands::cmd_read_nand(
        out,
        dev,
        med,
        start,
        count,
        serial,
        timeout_or_default(timeout_ms),
        prog.as_mut(),
    )?;
    Ok(())
}

/// Write NAND or eMMC flash using command handler settings.
/// Returns 0 on success, -1 on invalid argument, -2 on execution error,
/// -3 on internal panic.
///
/// # Safety
/// `input_path` must be a valid NUL-terminated string. `serial` must be NULL
/// or a valid NUL-terminated string. `progress` must be NULL or point to a
/// valid `ProgressC` whose callbacks (if set) are safe to call with its
/// `user_data`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn nandpromax_cmd_write_nand(
    input_path: *const c_char,
    device: c_int,
    media_type: c_int,
    start: u32,
    count: u32,
    count_has_val: bool,
    erase: bool,
    verify: bool,
    serial: *const c_char,
    timeout_ms: u64,
    progress: *const ProgressC,
) -> i32 {
    guarded(|| {
        let input = required_path(input_path, "input_path")?;
        let dev = device_arg(device)?;
        let med = media_arg(media_type)?;
        let ser = cstr_to_option_string(serial, "serial")?;
        write_nand_impl(
            input,
            dev,
            med,
            start,
            count_has_val.then_some(count),
            erase,
            verify,
            ser,
            timeout_ms,
            progress,
        )
    })
}

#[allow(clippy::too_many_arguments)]
unsafe fn write_nand_impl(
    input: PathBuf,
    dev: Option<DeviceType>,
    med: Option<MediaType>,
    start: u32,
    count: Option<u32>,
    erase: bool,
    verify: bool,
    serial: Option<String>,
    timeout_ms: u64,
    progress: *const ProgressC,
) -> Result<(), CallError> {
    let mut prog = wrap_progress(progress);
    commands::cmd_write_nand(
        input,
        dev,
        med,
        start,
        count,
        erase,
        verify,
        serial,
        timeout_or_default(timeout_ms),
        prog.as_mut(),
    )?;
    Ok(())
}

/// Output information about the target device.
/// Returns 0 on success, -1 on invalid argument, -2 on execution error,
/// -3 on internal panic.
///
/// # Safety
/// `serial` must be NULL or a valid NUL-terminated string. `progress` must be
/// NULL or point to a valid `ProgressC` whose callbacks (if set) are safe to
/// call with its `user_data`.
#[no_mangle]
pub unsafe extern "C" fn nandpromax_cmd_info(
    device: c_int,
    serial: *const c_char,
    timeout_ms: u64,
    progress: *const ProgressC,
) -> i32 {
    guarded(|| {
        let dev = device_arg(device)?;
        let ser = cstr_to_option_string(serial, "serial")?;
        let mut prog = wrap_progress(progress);
        commands::cmd_info(dev, ser, timeout_or_default(timeout_ms), prog.as_mut())?;
        Ok(())
    })
}

/// List available connected devices across LPC and DemoN backends.
/// Returns 0 on success, -2 on execution error, -3 on internal panic.
///
/// # Safety
/// `progress` must be NULL or point to a valid `ProgressC` whose callbacks (if
/// set) are safe to call with its `user_data`.
#[no_mangle]
pub unsafe extern "C" fn nandpromax_cmd_list_devices(progress: *const ProgressC) -> i32 {
    guarded(|| {
        let mut prog = wrap_progress(progress);
        commands::cmd_list_devices(prog.as_mut())?;
        Ok(())
    })
}

/// Detect LPC/XFlash device information for XSVF programming.
/// Returns 0 on success, -1 on invalid argument, -2 on execution error,
/// -3 on internal panic.
///
/// # Safety
/// `progress` must be NULL or point to a valid `ProgressC` whose callbacks (if
/// set) are safe to call with its `user_data`.
#[no_mangle]
pub unsafe extern "C" fn nandpromax_cmd_xsvf_detect(
    device: c_int,
    progress: *const ProgressC,
) -> i32 {
    guarded(|| {
        let dev = device_arg(device)?;
        let mut prog = wrap_progress(progress);
        commands::cmd_xsvf_detect(dev, prog.as_mut())?;
        Ok(())
    })
}

/// Program XSVF file to target CPLD / LPC device.
/// Returns 0 on success, -1 on invalid argument, -2 on execution error,
/// -3 on internal panic.
///
/// # Safety
/// `input_path` must be a valid NUL-terminated string. `progress` must be NULL
/// or point to a valid `ProgressC` whose callbacks (if set) are safe to call
/// with its `user_data`.
#[no_mangle]
pub unsafe extern "C" fn nandpromax_cmd_xsvf_write(
    input_path: *const c_char,
    device: c_int,
    progress: *const ProgressC,
) -> i32 {
    guarded(|| {
        let input = required_path(input_path, "input_path")?;
        let dev = device_arg(device)?;
        let mut prog = wrap_progress(progress);
        commands::cmd_xsvf_write(input, dev, prog.as_mut())?;
        Ok(())
    })
}

/// Start TCP device server using specified backend on bind address.
/// Returns 0 on success, -1 on invalid argument, -2 on execution error,
/// -3 on internal panic.
///
/// # Safety
/// `bind_addr` must be a valid NUL-terminated string. `progress` must be NULL
/// or point to a valid `ProgressC` whose callbacks (if set) are safe to call
/// with its `user_data`.
#[no_mangle]
pub unsafe extern "C" fn nandpromax_cmd_serve_tcp(
    bind_addr: *const c_char,
    device: c_int,
    progress: *const ProgressC,
) -> i32 {
    guarded(|| {
        let Some(bind) = cstr_to_option_string(bind_addr, "bind_addr")? else {
            return invalid("bind_addr must not be NULL");
        };
        let dev = device_arg(device)?;
        let mut prog = wrap_progress(progress);
        commands::cmd_serve_tcp(bind, dev, prog.as_mut())?;
        Ok(())
    })
}

/// Perform device auto-detection logic.
/// Returns 0 on success and populates out_device and out_media (each may be
/// NULL; values are `NandProDeviceC` / `NandProMediaC` constants). Returns -1
/// on invalid argument, -2 on execution error, -3 on internal panic.
///
/// # Safety
/// `serial` must be NULL or a valid NUL-terminated string. Each non-NULL
/// `out_*` pointer must be valid for writing a `c_int`.
#[no_mangle]
pub unsafe extern "C" fn nandpromax_auto_detect_device(
    user_device: c_int,
    user_media: c_int,
    serial: *const c_char,
    timeout_ms: u64,
    out_device: *mut c_int,
    out_media: *mut c_int,
) -> i32 {
    guarded(|| {
        let dev = device_arg(user_device)?;
        let med = media_arg(user_media)?;
        let ser = cstr_to_option_string(serial, "serial")?;
        let timeout = Duration::from_millis(timeout_or_default(timeout_ms));

        let (res_dev, res_med) = commands::auto_detect_device(dev, med, ser.as_deref(), timeout)?;
        if !out_device.is_null() {
            *out_device = NandProDeviceC::from_rust(Some(res_dev)) as c_int;
        }
        if !out_media.is_null() {
            *out_media = NandProMediaC::from_rust(Some(res_med)) as c_int;
        }
        Ok(())
    })
}

// ---------------------------------------------------------------------------
// Legacy C API Entry Points (Backwards Compatibility)
// ---------------------------------------------------------------------------

/// Legacy read wrapper.
/// Returns 0 on success, -1 on invalid argument, -2 on execution error,
/// -3 on internal panic. `elapsed_secs_out` is written only on success.
///
/// # Safety
/// `out_path` must be a valid NUL-terminated string. `serial` must be NULL or
/// a valid NUL-terminated string. `elapsed_secs_out` must be NULL or
/// valid for writing an `f64`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn nandpromax_read_nand_c(
    out_path: *const c_char,
    start: u32,
    count: u32,
    count_has_val: bool,
    device: c_int,
    media: c_int,
    serial: *const c_char,
    elapsed_secs_out: *mut f64,
) -> i32 {
    guarded(|| {
        let t0 = Instant::now();
        let out = required_path(out_path, "out_path")?;
        let dev = device_arg(device)?;
        let med = media_arg(media)?;
        let ser = cstr_to_option_string(serial, "serial")?;

        read_nand_impl(
            out,
            dev,
            med,
            start,
            count_has_val.then_some(count),
            ser,
            0,
            std::ptr::null(),
        )?;

        if !elapsed_secs_out.is_null() {
            *elapsed_secs_out = t0.elapsed().as_secs_f64();
        }
        Ok(())
    })
}

/// Legacy write wrapper.
/// Returns 0 on success, -1 on invalid argument, -2 on execution error,
/// -3 on internal panic. `elapsed_secs_out` is written only on success.
///
/// # Safety
/// `input_path` must be a valid NUL-terminated string. `serial` must be NULL or
/// a valid NUL-terminated string. `elapsed_secs_out` must be NULL or
/// valid for writing an `f64`.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn nandpromax_write_nand_c(
    input_path: *const c_char,
    start: u32,
    count: u32,
    count_has_val: bool,
    device: c_int,
    media: c_int,
    serial: *const c_char,
    erase: bool,
    verify: bool,
    elapsed_secs_out: *mut f64,
) -> i32 {
    guarded(|| {
        let t0 = Instant::now();
        let input = required_path(input_path, "input_path")?;
        let dev = device_arg(device)?;
        let med = media_arg(media)?;
        let ser = cstr_to_option_string(serial, "serial")?;

        write_nand_impl(
            input,
            dev,
            med,
            start,
            count_has_val.then_some(count),
            erase,
            verify,
            ser,
            0,
            std::ptr::null(),
        )?;

        if !elapsed_secs_out.is_null() {
            *elapsed_secs_out = t0.elapsed().as_secs_f64();
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn last_error() -> String {
        unsafe { CStr::from_ptr(nandpromax_last_error()) }
            .to_string_lossy()
            .into_owned()
    }

    #[test]
    fn from_c_rejects_out_of_range() {
        assert_eq!(NandProDeviceC::from_c(5), Some(NandProDeviceC::Demon));
        assert_eq!(NandProDeviceC::from_c(2), None);
        assert_eq!(NandProDeviceC::from_c(6), None);
        assert_eq!(NandProDeviceC::from_c(-1), None);
        assert_eq!(NandProDeviceC::from_c(i32::MAX), None);
        assert_eq!(NandProMediaC::from_c(2), Some(NandProMediaC::Emmc));
        assert_eq!(NandProMediaC::from_c(99), None);
    }

    #[test]
    fn invalid_enum_returns_minus_one_with_message() {
        let rc = unsafe { nandpromax_cmd_info(99, std::ptr::null(), 0, std::ptr::null()) };
        assert_eq!(rc, -1);
        assert!(last_error().contains("invalid device"), "{}", last_error());

        let rc = unsafe {
            nandpromax_auto_detect_device(
                0,
                7,
                std::ptr::null(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(rc, -1);
        assert!(last_error().contains("invalid media"));
    }

    #[test]
    fn null_required_pointer_is_invalid_argument() {
        let rc = unsafe { nandpromax_cmd_xsvf_write(std::ptr::null(), 0, std::ptr::null()) };
        assert_eq!(rc, -1);
        assert!(last_error().contains("input_path"));
    }

    #[test]
    fn panic_is_caught_and_reported() {
        let rc = guarded(|| panic!("boom"));
        assert_eq!(rc, -3);
        assert!(last_error().contains("boom"));
    }

    #[test]
    fn exec_error_is_recorded_and_next_call_clears_it() {
        let rc = guarded(|| Err(anyhow::anyhow!("inner").context("outer").into()));
        assert_eq!(rc, -2);
        assert_eq!(last_error(), "outer: inner");
        assert_eq!(guarded(|| Ok(())), 0);
        assert_eq!(last_error(), "");
    }

    #[test]
    fn version_is_nul_terminated_crate_version() {
        let v = unsafe { CStr::from_ptr(nandpromax_version()) };
        assert_eq!(v.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
    }
}
