#include "HsFFI.h"
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

extern void mutate_hs(uint8_t *p, size_t n);

#ifdef _WIN32
__declspec(dllexport)
#endif
void mutate(uint8_t *p, size_t n) {
    static bool inited = false;
    if (!inited) {
        int argc = 1;
        char *argv_storage[] = { "stardust-hs", NULL };
        char **argv = argv_storage;
        hs_init(&argc, &argv);
        inited = true;
    }
    mutate_hs(p, n);
}
