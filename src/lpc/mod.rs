//! LPC/XFlash USB device support for NAND interaction

#[allow(clippy::module_inception)]
pub mod lpc;
pub mod usb;

pub use lpc::{status, Command, FlashConfig, LpcClient};
pub use usb::UsbClient;
