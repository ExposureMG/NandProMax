use clap::ValueEnum;

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceType {
    /// PicoFlasher v4+ / DirtyPico (USB)
    Pico,
    /// NANDX / MTX (LPC/XFlash protocol)
    Lpc,
    /// JR-Programmer v1 / v2
    Jrp,
    /// TX DemoN
    Demon,
    /// ESPFlasher / PicoFlasher over TCP
    Esp,
}

/// Internal use only — not exposed to CLI since ESP now covers the TCP case.
#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdapterType {
    Usb,
    Tcp,
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Spi,
    Emmc,
}
