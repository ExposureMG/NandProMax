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
    NANDPRO_DEV_DEMON = 5,
    NANDPRO_DEV_ESP = 6
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

typedef void (*LogCallbackC)(const char *msg, void *user_data);
typedef void (*ProgressCallbackC)(uint64_t done, uint64_t total, void *user_data);

typedef struct {
    LogCallbackC log_fn;
    ProgressCallbackC update_fn;
    void *user_data;
} ProgressC;

/** Full C API Exports */

int nandpromax_cmd_read_nand(
    const char *out_path,
    NandProDeviceC device,
    NandProMediaC media_type,
    uint32_t start,
    uint32_t count,
    bool count_has_val,
    const char *serial,
    const char *addr,
    uint64_t timeout_ms,
    const ProgressC *progress
);

int nandpromax_cmd_write_nand(
    const char *input_path,
    NandProDeviceC device,
    NandProMediaC media_type,
    uint32_t start,
    uint32_t count,
    bool count_has_val,
    bool erase,
    bool verify,
    const char *serial,
    const char *addr,
    uint64_t timeout_ms,
    const ProgressC *progress
);

int nandpromax_cmd_info(
    NandProDeviceC device,
    const char *serial,
    const char *addr,
    uint64_t timeout_ms,
    const ProgressC *progress
);

int nandpromax_cmd_list_devices(const ProgressC *progress);

int nandpromax_cmd_xsvf_detect(NandProDeviceC device, const ProgressC *progress);

int nandpromax_cmd_xsvf_write(const char *input_path, NandProDeviceC device, const ProgressC *progress);

int nandpromax_cmd_serve_tcp(const char *bind_addr, NandProDeviceC device, const ProgressC *progress);

int nandpromax_auto_detect_device(
    NandProDeviceC user_device,
    NandProAdapterC user_adapter,
    NandProMediaC user_media,
    const char *serial,
    const char *addr,
    uint64_t timeout_ms,
    NandProDeviceC *out_device,
    NandProAdapterC *out_adapter,
    NandProMediaC *out_media
);

/** Legacy convenience wrappers */

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

#ifdef __cplusplus
}
#endif

#endif // NANDPROMAX_H
