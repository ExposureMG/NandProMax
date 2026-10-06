# NandProMax

Cross-platform Xbox 360 NAND/eMMC flasher tool. One command-line program and one C/C++ library that talk to the common hardware flashers:
PicoFlasher (USB), LPC (XFlash) programmers and TX DemoN.

> Flashing writes directly to your console's storage. Make a verified backup (read it twice and compare) before you write anything.

## Supported devices

| Device | Connection | CLI `-d` value | Status |
| --- | --- | --- | --- |
| PicoFlasher v4 USB | USB serial (CDC) | `pico` | Untested |
| LPC (JRP v2, NAND-X, etc) | USB (libusb) | `lpc` / `jrp` | Untested |
| TX DemoN | USB (libusb) | `demon` | Untested |
| CMSIS-DAP v2 | | none | |

If `-d` is omitted the tool tries to detect the device. Run `nandpromax list-devices` to see what is connected.

## Building

You need a stable Rust toolchain (1.85 or newer, see `rust-version` in `Cargo.toml`).

```sh
cargo build --release
```

Outputs are in `target/release/`:

- `nandpromax` (`nandpromax.exe` on Windows): the CLI
- `libnandpromax.so` / `libnandpromax.dylib` / `nandpromax.dll`: the shared library
- `libnandpromax.a` / `nandpromax.lib`: the static library

Linux needs the udev development headers and `pkg-config`:

```sh
# Debian / Ubuntu
sudo apt install libudev-dev pkg-config
# Fedora
sudo dnf install systemd-devel pkgconf-pkg-config
# Arch
sudo pacman -S systemd pkgconf
```

On Windows and macOS no extra packages are needed. libusb is built from source when it is not found on the system.

## Platform setup

### Linux

Install the udev rules so your user can open the devices without root:

```sh
sudo cp 69-npm.rules /etc/udev/rules.d/69-npm.rules
sudo udevadm control --reload-rules
sudo udevadm trigger
```

Then unplug and replug the device. The rules use `TAG+="uaccess"`, which grants access to the user logged in at the local seat. For SSH or other headless sessions, use the `dialout` group for serial ports or add your own rule with `GROUP=`.

### Windows

- PicoFlasher over USB appears as a CDC COM port. Pass it with `--serial COM5`, or omit it to auto-detect.
- LPC and TX DemoN are accessed through libusb. Install the WinUSB driver for the device with [Zadig](https://zadig.akeo.ie/), then replug it.

### macOS

No driver or rules are needed. PicoFlasher shows up as a `/dev/cu.usbmodem*` serial port; pass it with `--serial` if auto-detection finds more than one.

## Command-line usage

```text
nandpromax <COMMAND> [OPTIONS]
```

`nandpromax --help` and `nandpromax <command> --help` are the authoritative reference for flags and defaults. The examples below show the common forms.

Connection options (shared by most commands):

- `-d, --device <pico|lpc|jrp|demon>`: hardware device (auto-detected if omitted)
- `--serial <PORT>`: USB serial port for PicoFlasher, e.g. `/dev/ttyACM0` or `COM5` (auto-detected if omitted)
- `--timeout-ms <MS>`: operation timeout (default 3000)

Range options for read and write:

- `--start <N>`: first block / LBA (default 0)
- `--count <N>`: number of blocks / LBAs (default: all)

Write options:

- `--erase <true|false>`: currently has no effect; the tool never issues an erase itself, and whether a write erases first is up to the device firmware
- `--verify`: verify each block after writing

With PicoFlasher, the console SMC is stopped during a transfer and restarted afterwards. If a write or `--verify` fails after data has been written, the SMC is deliberately left stopped so the console does not boot a partial image; re-run the write or power cycle the console.

### List and inspect hardware

```sh
nandpromax list-devices
nandpromax info
nandpromax info -d pico --serial /dev/ttyACM0
```

### NAND

```sh
# dump the whole NAND
nandpromax nand read nand.bin
nandpromax nand read nand.bin -d pico --serial /dev/ttyACM0

# dump part of it
nandpromax nand read part.bin --start 0 --count 1024

# write an image, verifying afterwards
nandpromax nand write nand.bin --verify
nandpromax nand write nand.bin -d lpc --verify
```

### eMMC

```sh
nandpromax emmc read emmc.bin
nandpromax emmc write emmc.bin --verify
```

### XSVF / JTAG (LPC / JRP)

```sh
nandpromax xsvf detect -d lpc
nandpromax xsvf write firmware.xsvf -d lpc
```

### Bridge hardware over TCP (LPC / DemoN)

```sh
nandpromax serve-tcp -d lpc                      # listens on 127.0.0.1:8383
nandpromax serve-tcp --bind 0.0.0.0:8383 -d lpc  # exposes the flasher to the network
```

The server has no authentication: anyone who can connect can read and write the flash. Only bind a non-loopback address on a network you trust. The wire format and supported commands are described in the module docs in `src/tcp/server.rs`; this crate does not include a client for it.

## C / C++ library

The crate builds as `cdylib`, `staticlib` and `rlib`. Headers are in `include/`:

- `nandpromax.h`: C API (`nandpromax_cmd_read_nand`, `nandpromax_cmd_write_nand`, `nandpromax_cmd_info`, `nandpromax_cmd_list_devices`, `nandpromax_cmd_xsvf_detect`, `nandpromax_cmd_xsvf_write`, `nandpromax_cmd_serve_tcp`, `nandpromax_auto_detect_device` and the legacy `nandpromax_read_nand_c` / `nandpromax_write_nand_c` wrappers)
- `nandpromax.hpp`: header-only C++ wrapper (`NandProMax::readNand`, `NandProMax::writeNand`)

Progress and log output is delivered through the `ProgressC` struct (a log callback, a progress callback and a `user_data` pointer). Pass `NULL` for no callbacks.

Return codes:

| Code | Meaning |
| --- | --- |
| `0` | Success |
| `-1` | Invalid argument |
| `-2` | The operation failed |
| `-3` | Internal error |

After a non-zero return, `nandpromax_last_error` gives a message describing what went wrong. See `include/nandpromax.h` for its exact signature.

Linking example (Linux, shared library):

```sh
cc main.c -Iinclude -Ltarget/release -lnandpromax -o demo
LD_LIBRARY_PATH=target/release ./demo
```

```cpp
#include "nandpromax.hpp"

NandProMax::ReadOptions opts;
opts.out_path = "nand.bin";
opts.device = NandProMax::Device::PicoFlasher;
auto r = NandProMax::readNand(opts);
if (!r.success) { /* r.error_code */ }
```

## Credits

- ExposureMG - PicoFlasher, ESPFlasher
- Tuxuser - LPC, TX DemoN
---
- MATE / Hax360 - PicoFlasher v4
- [cOz] - DemoNTool
- G33KatWork & Juvenal - XFlash

## License

GNU General Public License v2.0 only. See `LICENSE`.
