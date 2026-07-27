#ifndef NANDPROMAX_H
#define NANDPROMAX_H

#include <stdbool.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum {
    NANDPRO_DEV_AUTO = 0,
    NANDPRO_DEV_PICOFLASHER = 1,
    NANDPRO_DEV_FTDI = 2,
    NANDPRO_DEV_LPC = 3,
    NANDPRO_DEV_DEMON = 4
} NandProDeviceC;

typedef enum {
    NANDPRO_ADAPTER_AUTO = 0,
    NANDPRO_ADAPTER_USB = 1,
    NANDPRO_ADAPTER_TCP = 2
} NandProAdapterC;

typedef enum {
    NANDPRO_MEDIA_AUTO = 0,
    NANDPRO_MEDIA_SPI = 1,
    NANDPRO_MEDIA_EMMC = 2
} NandProMediaC;

typedef enum {
    FTDI_PAGE_FORMAT_AUTO = 0,
    FTDI_PAGE_FORMAT_SMALL = 1,
    FTDI_PAGE_FORMAT_BIG = 2
} FtdiPageFormatC;

/**
 * Read NAND or eMMC flash using any hardware flasher device.
 *
 * @param out_path Output file path
 * @param start Start block or LBA offset
 * @param count Block count to read (if count_has_val is true)
 * @param count_has_val Whether count has a value
 * @param device Target hardware device (AUTO probes in priority order)
 * @param adapter Interface adapter (AUTO, USB, TCP)
 * @param media Flash media type (AUTO, SPI, EMMC)
 * @param serial_or_addr Optional serial port or IP:port endpoint (or NULL for default)
 * @param elapsed_secs_out Output pointer to receive operation duration in seconds
 * @return 0 on success, negative error code on failure
 */
int nandpromax_read_nand_c(
    const char *out_path,
    uint32_t start,
    uint32_t count,
    bool count_has_val,
    NandProDeviceC device,
    NandProAdapterC adapter,
    NandProMediaC media,
    const char *serial_or_addr,
    double *elapsed_secs_out
);

/**
 * Write input file to NAND or eMMC flash using any hardware flasher device.
 *
 * @param input_path Input file path
 * @param start Start block or LBA offset
 * @param count Block count to write (if count_has_val is true)
 * @param count_has_val Whether count has a value
 * @param device Target hardware device (AUTO probes in priority order)
 * @param adapter Interface adapter (AUTO, USB, TCP)
 * @param media Flash media type (AUTO, SPI, EMMC)
 * @param serial_or_addr Optional serial port or IP:port endpoint (or NULL for default)
 * @param erase Erase block before writing
 * @param verify Verify written blocks
 * @param elapsed_secs_out Output pointer to receive operation duration in seconds
 * @return 0 on success, negative error code on failure
 */
int nandpromax_write_nand_c(
    const char *input_path,
    uint32_t start,
    uint32_t count,
    bool count_has_val,
    NandProDeviceC device,
    NandProAdapterC adapter,
    NandProMediaC media,
    const char *serial_or_addr,
    bool erase,
    bool verify,
    double *elapsed_secs_out
);

/**
 * Legacy FTDI-specific NAND read function.
 */
int ftdi_read_nand_c(
    const char *out_path,
    uint32_t start,
    uint32_t count,
    bool count_has_val,
    FtdiPageFormatC page_format,
    const char *ftdi_desc,
    int32_t ftdi_index,
    bool ftdi_index_has_val,
    uint32_t freq_hz,
    double *elapsed_secs_out
);

/**
 * Legacy FTDI-specific NAND write function.
 */
int ftdi_write_nand_c(
    const char *input_path,
    uint32_t start,
    uint32_t count,
    bool count_has_val,
    FtdiPageFormatC page_format,
    const char *ftdi_desc,
    int32_t ftdi_index,
    bool ftdi_index_has_val,
    uint32_t freq_hz,
    bool erase,
    bool verify,
    double *elapsed_secs_out
);

#ifdef __cplusplus
}
#endif

#endif // NANDPROMAX_H
