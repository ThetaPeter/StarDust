#include <lean/lean.h>
#include <stdbool.h>
#include <stdint.h>
#include <stddef.h>

uint8_t lean_w11_load_u8(size_t p) {
    return *(const uint8_t *)p;
}

lean_object *lean_w11_store_u8(size_t p, uint8_t v) {
    *(uint8_t *)p = v;
    return lean_box(0);
}

void lean_initialize_runtime_module(void);
lean_object *initialize_Mutate(uint8_t builtin);
lean_object *mutate_lean(size_t p, size_t n);

static void ensure_lean(void) {
    static bool inited = false;
    lean_object *res;
    if (inited) {
        return;
    }
    lean_initialize_runtime_module();
    res = initialize_Mutate(1);
    lean_io_mark_end_initialization();
    if (lean_io_result_is_ok(res)) {
        lean_dec_ref(res);
        lean_init_task_manager();
    } else {
        lean_io_result_show_error(res);
        lean_dec(res);
        __builtin_trap();
    }
    inited = true;
}

#ifdef _WIN32
__declspec(dllexport)
#endif
void mutate(uint8_t *p, size_t n) {
    ensure_lean();
    lean_object *r = mutate_lean((size_t)p, n);
    lean_dec(r);
}
