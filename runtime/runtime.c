#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <stdint.h>

/* ---- printing helpers ---- */

void kroa_print_i64(int64_t v) {
    printf("%lld\n", (long long)v);
}

void kroa_print_f64(double v) {
    printf("%f\n", v);
}

void kroa_print_bool(_Bool v) {
    puts(v ? "true" : "false");
}

void kroa_print_str(const char *ptr, int64_t len) {
    if (!ptr || len < 0) {
        return;
    }
    fwrite(ptr, 1, (size_t)len, stdout);
    fputc('\n', stdout);
}

/* ---- lexical arenas (stack of bump allocators) ---- */

typedef struct KroaArena {
    char *data;
    size_t capacity;
    size_t offset;
    struct KroaArena *prev;
} KroaArena;

static KroaArena *kroa_arena_stack = NULL;

void kroa_arena_enter(void) {
    KroaArena *a = (KroaArena *)calloc(1, sizeof(KroaArena));
    if (!a) {
        fprintf(stderr, "kroa: failed to create arena\n");
        abort();
    }
    a->capacity = 4096;
    a->data = (char *)malloc(a->capacity);
    if (!a->data) {
        fprintf(stderr, "kroa: failed to allocate arena buffer\n");
        abort();
    }
    a->offset = 0;
    a->prev = kroa_arena_stack;
    kroa_arena_stack = a;
}

void kroa_arena_exit(void) {
    KroaArena *a = kroa_arena_stack;
    if (!a) {
        return;
    }
    kroa_arena_stack = a->prev;
    free(a->data);
    free(a);
}

void *kroa_arena_alloc(int64_t nbytes) {
    if (nbytes < 0) {
        fprintf(stderr, "kroa: negative arena allocation\n");
        abort();
    }
    KroaArena *a = kroa_arena_stack;
    if (!a) {
        /* allocate a transient arena if none is open */
        kroa_arena_enter();
        a = kroa_arena_stack;
    }
    size_t need = (size_t)nbytes;
    /* 16-byte align */
    size_t align = 16;
    size_t aligned = (a->offset + (align - 1)) & ~(align - 1);
    if (aligned + need > a->capacity) {
        size_t new_cap = a->capacity * 2;
        while (new_cap < aligned + need) {
            new_cap *= 2;
        }
        char *nd = (char *)realloc(a->data, new_cap);
        if (!nd) {
            fprintf(stderr, "kroa: arena realloc failed\n");
            abort();
        }
        a->data = nd;
        a->capacity = new_cap;
    }
    void *p = a->data + aligned;
    a->offset = aligned + need;
    return p;
}

/* Convert UTF-8 (ptr,len) into a NUL-terminated C string allocated in the current arena.
   Rejects interior NUL bytes. */
char *kroa_str_to_cstr(const char *ptr, int64_t len) {
    if (!ptr || len < 0) {
        fprintf(stderr, "kroa: invalid string for C conversion\n");
        abort();
    }
    for (int64_t i = 0; i < len; i++) {
        if (ptr[i] == '\0') {
            fprintf(stderr, "kroa: string contains interior NUL; cannot convert to c_string\n");
            abort();
        }
    }
    char *out = (char *)kroa_arena_alloc(len + 1);
    memcpy(out, ptr, (size_t)len);
    out[len] = '\0';
    return out;
}

/* ---- safe indexing / slicing ---- */

void kroa_bounds_panic(int64_t index, int64_t len) {
    fprintf(
        stderr,
        "kroa: index out of bounds: index=%lld, len=%lld\n",
        (long long)index,
        (long long)len
    );
    abort();
}

void kroa_slice_bounds_panic(int64_t start, int64_t end, int64_t len) {
    fprintf(
        stderr,
        "kroa: slice out of bounds: start=%lld, end=%lld, len=%lld\n",
        (long long)start,
        (long long)end,
        (long long)len
    );
    abort();
}
