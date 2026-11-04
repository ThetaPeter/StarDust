#include <stddef.h>
#include <stdint.h>

#if defined(_WIN32)
#define STARDUST_EXPORT __declspec(dllexport)
#else
#define STARDUST_EXPORT
#endif

STARDUST_EXPORT void process(uint8_t *p, size_t n) {
    for (size_t i = 0; i < n; i++) {
        p[i] += 1;
    }
}

STARDUST_EXPORT void write_a(uint8_t *p, size_t n) {
    for (size_t i = 0; i < n; i++) {
        p[i] = 0xA1;
    }
}

STARDUST_EXPORT void mut_b(uint8_t *p, size_t n) {
    for (size_t i = 0; i < n; i++) {
        p[i] += 1;
    }
}

/* Node: tag u8 at 0, next u64 LE at 8, size 16. Start at 8; at most 2 hops. */
STARDUST_EXPORT void arena_walk(uint8_t *base, size_t n) {
    uint64_t off = 8;
    for (int i = 0; i < 2; i++) {
        if (off == 0 || off > (uint64_t)n || (uint64_t)n - off < 16) {
            break;
        }
        base[off] = (uint8_t)(base[off] + 1);
        uint64_t next = 0;
        for (int b = 0; b < 8; b++) {
            next |= ((uint64_t)base[off + 8 + (size_t)b]) << (8 * b);
        }
        off = next;
    }
}

STARDUST_EXPORT void add_u32(uint8_t *p, size_t n, uint32_t x) {
    uint8_t add = (uint8_t)x;
    for (size_t i = 0; i < n; i++) {
        p[i] = (uint8_t)(p[i] + add);
    }
}

STARDUST_EXPORT uint32_t load_u32(uint8_t *p, size_t n) {
    uint32_t v = 0;
    size_t take = n < 4 ? n : 4;
    for (size_t i = 0; i < take; i++) {
        v |= ((uint32_t)p[i]) << (8 * (int)i);
    }
    return v;
}

STARDUST_EXPORT uint64_t plus_u64(uint8_t *p, size_t n, uint64_t x) {
    uint64_t v = 0;
    size_t take = n < 8 ? n : 8;
    for (size_t i = 0; i < take; i++) {
        v |= ((uint64_t)p[i]) << (8 * (int)i);
    }
    return v + x;
}

STARDUST_EXPORT void stamp_out(uint8_t *self, size_t n, uint8_t *out) {
    (void)self;
    (void)n;
    for (size_t i = 0; i < 8; i++) {
        out[i] = 0xC3;
    }
}

STARDUST_EXPORT void pair_write(uint8_t *a, size_t an, uint8_t *b, size_t bn) {
    for (size_t i = 0; i < an; i++) {
        a[i] = 0xA1;
    }
    for (size_t i = 0; i < bn; i++) {
        b[i] = 0xB2;
    }
}
