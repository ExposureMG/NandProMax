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
    NANDPRO_DEV_LPC = 3,
    NANDPRO_DEV_JRP = 4,
    NANDPRO_DEV_DEMON = 5
} NandProDeviceC;

typedef enum {
    NANDPRO_MEDIA_AUTO = 0,
    NANDPRO_MEDIA_SPI = 1,
    NANDPRO_MEDIA_EMMC = 2
} NandProMediaC;

typedef void (*LogCallbackC)(const char *msg, void *user_data);
typedef void (*ProgressCallbackC)(uint64_t done, uint64_t total, void *user_data);

typedef struct {
    LogCallbackC log_fn;
    ProgressCallbackC update_fn;
    void *user_data;
} ProgressC;

/*
 * Return codes (all functions returning int):
 *    0  success
 *   -1  invalid argument (NULL required pointer, out-of-range enum value,
 *       non-UTF-8 string)
 *   -2  execution error (device/IO failure)
 *   -3  internal panic caught at the API boundary
 *
 * Enum arguments are passed as plain int so out-of-range values from C are
 * rejected with -1 instead of being undefined behaviour. Pass the
 * NandProDeviceC / NandProMediaC constants above.
 * Likewise the out_* pointers of nandpromax_auto_detect_device are int *.
 *
 * Threading: error state is per thread. Strings are UTF-8.
 */

/**
 * Library version, e.g. "0.0.1". Static storage; never NULL.
 */
const char *nandpromax_version(void);

/**
 * Description of the last error on the calling thread. Never NULL: an empty
 * string means the most recent nandpromax_* call succeeded. The pointer is
 * valid until the next nandpromax_* call on the same thread; copy it if you
 * need to keep it.
 */
const char *nandpromax_last_error(void);

/** Full C API Exports */

/**
 * Read NAND or eMMC flash. `device` is a NandProDeviceC and `media_type` a
 * NandProMediaC. `serial` (USB serial port, PicoFlasher only) may be NULL to
 * auto-detect. `timeout_ms` of 0 means 3000. `progress` may be NULL (messages
 * go to stderr). Returns 0 / -1 / -2 / -3, see above.
 */
int nandpromax_cmd_read_nand(
    const char *out_path,
    int device,
    int media_type,
    uint32_t start,
    uint32_t count,
    bool count_has_val,
    const char *serial,
    uint64_t timeout_ms,
    const ProgressC *progress
);

/**
 * Write NAND or eMMC flash. `device` is a NandProDeviceC and `media_type` a
 * NandProMediaC. Other arguments as nandpromax_cmd_read_nand.
 */
int nandpromax_cmd_write_nand(
    const char *input_path,
    int device,
    int media_type,
    uint32_t start,
    uint32_t count,
    bool count_has_val,
    bool erase,
    bool verify,
    const char *serial,
    uint64_t timeout_ms,
    const ProgressC *progress
);

/** Print information about the target device. `device` is a NandProDeviceC. */
int nandpromax_cmd_info(
    int device,
    const char *serial,
    uint64_t timeout_ms,
    const ProgressC *progress
);

/** List connected devices (LPC and DemoN backends, serial ports). */
int nandpromax_cmd_list_devices(const ProgressC *progress);

/** Detect LPC/XFlash device information for XSVF programming. */
int nandpromax_cmd_xsvf_detect(int device, const ProgressC *progress);

/** Program an XSVF file to the target CPLD / LPC device. */
int nandpromax_cmd_xsvf_write(const char *input_path, int device, const ProgressC *progress);

/** Serve the selected backend over TCP on `bind_addr` (blocks). */
int nandpromax_cmd_serve_tcp(const char *bind_addr, int device, const ProgressC *progress);

/**
 * Auto-detect device and media. On success each non-NULL out_* receives a
 * NandProDeviceC / NandProMediaC value.
 */
int nandpromax_auto_detect_device(
    int user_device,
    int user_media,
    const char *serial,
    uint64_t timeout_ms,
    int *out_device,
    int *out_media
);

/** Legacy convenience wrappers */

/**
 * Read wrapper. `serial` is the USB serial port (may be NULL).
 * `elapsed_secs_out` (may be NULL) is written only on success.
 */
int nandpromax_read_nand_c(
    const char *out_path,
    uint32_t start,
    uint32_t count,
    bool count_has_val,
    int device,
    int media,
    const char *serial,
    double *elapsed_secs_out
);

/** Write wrapper; arguments as nandpromax_read_nand_c. */
int nandpromax_write_nand_c(
    const char *input_path,
    uint32_t start,
    uint32_t count,
    bool count_has_val,
    int device,
    int media,
    const char *serial,
    bool erase,
    bool verify,
    double *elapsed_secs_out
);

#ifdef __cplusplus
}
#endif

#endif // NANDPROMAX_H
