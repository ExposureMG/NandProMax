#pragma once

#include <cstdint>
#include <optional>
#include <string>

#include "nandpromax.h"

namespace NandProMax {

enum class Device {
    Auto = NANDPRO_DEV_AUTO,
    PicoFlasher = NANDPRO_DEV_PICOFLASHER,
    Lpc = NANDPRO_DEV_LPC,
    Jrp = NANDPRO_DEV_JRP,
    Demon = NANDPRO_DEV_DEMON
};

enum class Media {
    Auto = NANDPRO_MEDIA_AUTO,
    Spi = NANDPRO_MEDIA_SPI,
    Emmc = NANDPRO_MEDIA_EMMC
};

struct ReadOptions {
    std::string out_path;
    uint32_t start = 0;
    std::optional<uint32_t> count;
    Device device = Device::Auto;
    Media media = Media::Auto;
    std::string serial; // USB serial port path (PicoFlasher); empty = auto-detect
};

struct WriteOptions {
    std::string input_path;
    uint32_t start = 0;
    std::optional<uint32_t> count;
    Device device = Device::Auto;
    Media media = Media::Auto;
    std::string serial; // USB serial port path (PicoFlasher); empty = auto-detect
    bool erase = true;
    bool verify = false;
};

struct Result {
    bool success = false;
    int error_code = -1;
    double elapsed_seconds = 0.0;
    std::string error; // Filled from nandpromax_last_error() when error_code != 0
};

namespace detail {
inline std::string lastError() {
    const char *msg = nandpromax_last_error();
    return msg ? std::string(msg) : std::string();
}
} // namespace detail

/**
 * Read NAND or eMMC flash using any hardware flasher device (C++ wrapper).
 */
inline Result readNand(const ReadOptions& opts) {
    double elapsed = 0.0;
    int rc = nandpromax_read_nand_c(
        opts.out_path.c_str(),
        opts.start,
        opts.count.value_or(0),
        opts.count.has_value(),
        static_cast<int>(opts.device),
        static_cast<int>(opts.media),
        opts.serial.empty() ? nullptr : opts.serial.c_str(),
        &elapsed
    );
    return Result{ rc == 0, rc, elapsed, rc == 0 ? std::string() : detail::lastError() };
}

/**
 * Write file to NAND or eMMC flash using any hardware flasher device (C++ wrapper).
 */
inline Result writeNand(const WriteOptions& opts) {
    double elapsed = 0.0;
    int rc = nandpromax_write_nand_c(
        opts.input_path.c_str(),
        opts.start,
        opts.count.value_or(0),
        opts.count.has_value(),
        static_cast<int>(opts.device),
        static_cast<int>(opts.media),
        opts.serial.empty() ? nullptr : opts.serial.c_str(),
        opts.erase,
        opts.verify,
        &elapsed
    );
    return Result{ rc == 0, rc, elapsed, rc == 0 ? std::string() : detail::lastError() };
}

} // namespace NandProMax
