// Syscall shims for the wasm32-unknown-unknown build.
//
// libmgba's libc surface is almost entirely pure compute (string/memory,
// snprintf, math, dlmalloc-on-memory.grow), which wasi-libc provides
// without any imports. The handful of syscall-backed symbols below are
// defined here instead, so the wasi-libc objects that would import
// wasi_snapshot_preview1 are never pulled into the link and the final
// module needs no WASI runtime at all. The build asserts this: a
// gbaroll build step fails if any wasi_snapshot_preview1 import
// survives.

#include <stddef.h>
#include <sys/time.h>
#include <time.h>
#include <wasi/api.h>

// Provided by the Rust side of the module (js_sys::Date::now()).
extern double gbaroll_now_unix_ms(void);

// mgba's one native clock read is gettimeofday() in core/serialize.c
// (savestate metadata stamps).
int gettimeofday(struct timeval *restrict tv, void *restrict tz) {
    (void)tz;
    if (tv) {
        double ms = gbaroll_now_unix_ms();
        tv->tv_sec = (time_t)(ms / 1000.0);
        tv->tv_usec = (suseconds_t)((ms - (double)tv->tv_sec * 1000.0) * 1000.0);
    }
    return 0;
}

int clock_gettime(clockid_t clock, struct timespec *ts) {
    (void)clock;
    if (ts) {
        double ms = gbaroll_now_unix_ms();
        ts->tv_sec = (time_t)(ms / 1000.0);
        ts->tv_nsec = (long)((ms - (double)ts->tv_sec * 1000.0) * 1000000.0);
    }
    return 0;
}

time_t time(time_t *out) {
    time_t t = (time_t)(gbaroll_now_unix_ms() / 1000.0);
    if (out) {
        *out = t;
    }
    return t;
}

// Trap instead of proc_exit.
_Noreturn void abort(void) {
    __builtin_trap();
}

// wasi-libc routes all stdio through these three FILE backends; stdout/
// stderr's FILE globals reference them. Swallowing writes (and stubbing
// seek/close) makes printf-family calls no-ops without fd_write/fd_seek/
// fd_close imports. mgba's default stdout logger is dead code anyway —
// the Rust side installs its own mLog hook.
size_t __stdio_write(void *f, const unsigned char *buf, size_t len) {
    (void)f;
    (void)buf;
    return len;
}

long long __stdio_seek(void *f, long long off, int whence) {
    (void)f;
    (void)off;
    (void)whence;
    return -1;
}

int __stdio_close(void *f) {
    (void)f;
    return 0;
}

// Zero-fill "entropy": nothing in the emulator core wants randomness
// for anything security-relevant.
int getentropy(void *buffer, size_t len) {
    unsigned char *p = buffer;
    for (size_t i = 0; i < len; i++) {
        p[i] = 0;
    }
    return 0;
}

// The syscall floor: wasi-libc's implementations of the __wasi_*
// functions are thin wrappers over wasi_snapshot_preview1 imports, all
// living in ONE archive member (__wasilibc_real.o) — so referencing any
// single one pulls every import along. shim.c + wasi-stubs.c (generated,
// see tools/gen-wasi-stubs.py) together define the complete set, keeping
// that object out of the link entirely: any libc path that still reaches
// for a syscall gets a well-defined answer instead of the module growing
// a WASI import. The behavior-bearing ones live here; the pure
// "not supported" remainder is generated.

__wasi_errno_t __wasi_environ_get(uint8_t **environ_ptrs, uint8_t *environ_buf) {
    (void)environ_ptrs;
    (void)environ_buf;
    return 0;
}

__wasi_errno_t __wasi_environ_sizes_get(__wasi_size_t *count, __wasi_size_t *buf_size) {
    *count = 0;
    *buf_size = 0;
    return 0;
}

// BADF is the "no more preopens" sentinel that ends libc's preopen scan.
__wasi_errno_t __wasi_fd_prestat_get(__wasi_fd_t fd, __wasi_prestat_t *prestat) {
    (void)fd;
    (void)prestat;
    return __WASI_ERRNO_BADF;
}

__wasi_errno_t __wasi_random_get(uint8_t *buf, __wasi_size_t buf_len) {
    for (__wasi_size_t i = 0; i < buf_len; i++) {
        buf[i] = 0;
    }
    return 0;
}

__wasi_errno_t __wasi_sched_yield(void) {
    return 0;
}

_Noreturn void __wasi_proc_exit(__wasi_exitcode_t code) {
    (void)code;
    __builtin_trap();
}
