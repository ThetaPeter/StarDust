#include <stddef.h>
#include <stdint.h>

#if defined(_WIN32)
#define STARDUST_EXPORT __declspec(dllexport)
#else
#define STARDUST_EXPORT
#endif

/* Sum every byte except the little-endian u32 at offset 4, write that sum there. */
static void checksum_skip_u32_at_4(uint8_t *p, size_t n) {
    if (p == NULL || n < 8) {
        return;
    }
    uint32_t sum = 0;
    for (size_t i = 0; i < n; i++) {
        if (i >= 4 && i < 8) {
            continue;
        }
        sum += (uint32_t)p[i];
    }
    p[4] = (uint8_t)sum;
    p[5] = (uint8_t)(sum >> 8);
    p[6] = (uint8_t)(sum >> 16);
    p[7] = (uint8_t)(sum >> 24);
}

STARDUST_EXPORT void parse_frame(uint8_t *p, size_t n) {
    checksum_skip_u32_at_4(p, n);
}

STARDUST_EXPORT void seal_db(uint8_t *p, size_t n) {
    /* ProtocolLay.digest sits at offset 4, same skip as Frame.checksum. */
    checksum_skip_u32_at_4(p, n);
}
