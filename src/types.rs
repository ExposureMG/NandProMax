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
}

#[derive(ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaType {
    Spi,
    Emmc,
}
