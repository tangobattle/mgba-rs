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
