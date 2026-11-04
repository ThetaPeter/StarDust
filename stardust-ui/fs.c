#include <stddef.h>
#include <stdint.h>

#if defined(_WIN32)
#define STARDUST_EXPORT __declspec(dllexport)
#else
#define STARDUST_EXPORT
#endif

/* Node repr_c: name_len u64 @0, name [64] @8, kind u8 @72, size 80.
   File: + size u64 @80, size 88.
   Dir: + nchild u64 @80, child [u64;64] @88, size 600. */

#define NAME_MAX 64
#define CHILD_MAX 64

static uint64_t ru64(const uint8_t *p) {
    uint64_t v = 0;
    for (int i = 0; i < 8; i++) {
        v |= ((uint64_t)p[i]) << (8 * i);
    }
    return v;
}

STARDUST_EXPORT uint64_t fs_kind(uint8_t *p, size_t n) {
    if (n < 73) {
        return 0;
    }
    return (uint64_t)p[72];
}

STARDUST_EXPORT uint64_t fs_name(uint8_t *p, size_t n, uint8_t *out, uint64_t cap) {
    uint64_t len;
    uint64_t i;
    if (n < 72 || cap == 0) {
        return 0;
    }
    len = ru64(p);
    if (len > NAME_MAX) {
        len = NAME_MAX;
    }
    if (len > cap) {
        len = cap;
    }
    for (i = 0; i < len; i++) {
        out[i] = p[8 + i];
    }
    return len;
}

STARDUST_EXPORT uint64_t fs_child_count(uint8_t *p, size_t n) {
    if (n < 88) {
        return 0;
    }
    return ru64(p + 80);
}

STARDUST_EXPORT uint64_t fs_child_at(uint8_t *p, size_t n, uint64_t i) {
    uint64_t c;
    if (n < 600) {
        return 0;
    }
    c = ru64(p + 80);
    if (i >= c || i >= CHILD_MAX) {
        return 0;
    }
    return ru64(p + 88 + (i * 8));
}
