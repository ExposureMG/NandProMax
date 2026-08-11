#pragma once

#include <cstdint>
#include <optional>
#include <string>

#include "nandpromax.h"

namespace NandProMax {

enum class Device {
    Auto = NANDPRO_DEV_AUTO,
    PicoFlasher = NANDPRO_DEV_PICOFLASHER,
    Ftdi = NANDPRO_DEV_FTDI,
    Lpc = NANDPRO_DEV_LPC,
    Jrp = NANDPRO_DEV_JRP,
    Demon = NANDPRO_DEV_DEMON,
    Esp = NANDPRO_DEV_ESP
};

enum class Adapter {
    Auto = NANDPRO_ADAPTER_AUTO,
    Usb = NANDPRO_ADAPTER_USB,
    Tcp = NANDPRO_ADAPTER_TCP
};

enum class Media {
    Auto = NANDPRO_MEDIA_AUTO,
    Spi = NANDPRO_MEDIA_SPI,
    Emmc = NANDPRO_MEDIA_EMMC
};

enum class PageFormat {
    Auto = FTDI_PAGE_FORMAT_AUTO,
    Small = FTDI_PAGE_FORMAT_SMALL,
    Big = FTDI_PAGE_FORMAT_BIG
};

struct ReadOptions {
    std::string out_path;
    uint32_t start = 0;
    std::optional<uint32_t> count;
    Device device = Device::Auto;
    Adapter adapter = Adapter::Auto;
    Media media = Media::Auto;
    std::string endpoint; // Serial port path or IP:port address
};

struct WriteOptions {
    std::string input_path;
    uint32_t start = 0;
    std::optional<uint32_t> count;
    Device device = Device::Auto;
    Adapter adapter = Adapter::Auto;
    Media media = Media::Auto;
    std::string endpoint; // Serial port path or IP:port address
    bool erase = true;
    bool verify = false;
};

struct Result {
    bool success = false;
    int error_code = -1;
    double elapsed_seconds = 0.0;
};

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
        static_cast<NandProDeviceC>(opts.device),
        static_cast<NandProAdapterC>(opts.adapter),
        static_cast<NandProMediaC>(opts.media),
        opts.endpoint.empty() ? nullptr : opts.endpoint.c_str(),
        &elapsed
    );
    return Result{ rc == 0, rc, elapsed };
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
        static_cast<NandProDeviceC>(opts.device),
        static_cast<NandProAdapterC>(opts.adapter),
        static_cast<NandProMediaC>(opts.media),
        opts.endpoint.empty() ? nullptr : opts.endpoint.c_str(),
        opts.erase,
        opts.verify,
        &elapsed
    );
    return Result{ rc == 0, rc, elapsed };
}

} // namespace NandProMax
