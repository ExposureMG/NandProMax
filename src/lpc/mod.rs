//! LPC/XFlash USB device support for NAND interaction

pub mod lpc;
pub mod usb;

pub use lpc::{status, Command, FlashConfig, LpcClient};
pub use usb::UsbClient;
