use std::os::raw::{c_char, c_int, c_long, c_ulong, c_void};

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibxsvfMode {
    Svf = 1,
    Xsvf = 2,
    Scan = 3,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibxsvfTapState {
    Init = 0,
    Reset = 1,
    Idle = 2,
    DrSelect = 3,
    DrCapture = 4,
    DrShift = 5,
    DrExit1 = 6,
    DrPause = 7,
    DrExit2 = 8,
    DrUpdate = 9,
    IrSelect = 10,
    IrCapture = 11,
    IrShift = 12,
    IrExit1 = 13,
    IrPause = 14,
    IrExit2 = 15,
    IrUpdate = 16,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibxsvfMem {
    XsvfTdiData = 0,
    XsvfTdoData = 1,
    XsvfTdoMask = 2,
    XsvfAddrMask = 3,
    XsvfDataMask = 4,
    SvfCommandBuf = 5,
    SvfSdrTdiData = 6,
    SvfSdrTdiMask = 7,
    SvfSdrTdoData = 8,
    SvfSdrTdoMask = 9,
    SvfSdrRetMask = 10,
    SvfSirTdiData = 11,
    SvfSirTdiMask = 12,
    SvfSirTdoData = 13,
    SvfSirTdoMask = 14,
    SvfSirRetMask = 15,
    SvfHdrTdiData = 16,
    SvfHdrTdiMask = 17,
    SvfHdrTdoData = 18,
    SvfHdrTdoMask = 19,
    SvfHdrRetMask = 20,
    SvfHirTdiData = 21,
    SvfHirTdiMask = 22,
    SvfHirTdoData = 23,
    SvfHirTdoMask = 24,
    SvfHirRetMask = 25,
    SvfTdrTdiData = 26,
    SvfTdrTdiMask = 27,
    SvfTdrTdoData = 28,
    SvfTdrTdoMask = 29,
    SvfTdrRetMask = 30,
    SvfTirTdiData = 31,
    SvfTirTdiMask = 32,
    SvfTirTdoData = 33,
    SvfTirTdoMask = 34,
    SvfTirRetMask = 35,
    Num = 36,
}

#[repr(C)]
pub struct LibxsvfHost {
    pub setup: Option<unsafe extern "C" fn(h: *mut LibxsvfHost) -> c_int>,
    pub shutdown: Option<unsafe extern "C" fn(h: *mut LibxsvfHost) -> c_int>,
    pub udelay: Option<unsafe extern "C" fn(h: *mut LibxsvfHost, usecs: c_long, tms: c_int, num_tck: c_long)>,
    pub getbyte: Option<unsafe extern "C" fn(h: *mut LibxsvfHost) -> c_int>,
    pub sync: Option<unsafe extern "C" fn(h: *mut LibxsvfHost) -> c_int>,
    pub pulse_tck: Option<
        unsafe extern "C" fn(
            h: *mut LibxsvfHost,
            tms: c_int,
            tdi: c_int,
            tdo: c_int,
            rmask: c_int,
            sync: c_int,
        ) -> c_int,
    >,
    pub pulse_sck: Option<unsafe extern "C" fn(h: *mut LibxsvfHost)>,
    pub set_trst: Option<unsafe extern "C" fn(h: *mut LibxsvfHost, v: c_int)>,
    pub set_frequency: Option<unsafe extern "C" fn(h: *mut LibxsvfHost, v: c_int) -> c_int>,
    pub report_tapstate: Option<unsafe extern "C" fn(h: *mut LibxsvfHost)>,
    pub report_device: Option<unsafe extern "C" fn(h: *mut LibxsvfHost, idcode: c_ulong)>,
    pub report_status: Option<unsafe extern "C" fn(h: *mut LibxsvfHost, message: *const c_char)>,
    pub report_error: Option<
        unsafe extern "C" fn(h: *mut LibxsvfHost, file: *const c_char, line: c_int, message: *const c_char),
    >,
    pub realloc: Option<
        unsafe extern "C" fn(
            h: *mut LibxsvfHost,
            ptr: *mut c_void,
            size: c_int,
            which: LibxsvfMem,
        ) -> *mut c_void,
    >,
    pub tap_state: LibxsvfTapState,
    pub user_data: *mut c_void,
}

#[link(name = "xsvf", kind = "static")]
extern "C" {
    pub fn libxsvf_play(h: *mut LibxsvfHost, mode: LibxsvfMode) -> c_int;
    pub fn libxsvf_state2str(tap_state: LibxsvfTapState) -> *const c_char;
    pub fn libxsvf_mem2str(which: LibxsvfMem) -> *const c_char;
}
