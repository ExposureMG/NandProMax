//! DemoN USB device support for NAND interaction

#[allow(clippy::module_inception)]
pub mod demon;
pub mod usb;

pub use demon::DemonClient;
