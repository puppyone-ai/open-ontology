#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <sys/syscall.h>

/* Fixture-only ARM64 Linux ABI bridge, never loaded by the product.
 * The uniform-memory Docker VM has no mbind syscall. Interleaving one node
 * is a no-op; preserve all other calls, modes and error conditions.
 * Pin dlsym to the image's older glibc ABI rather than the builder's ABI. */
__asm__(".symver dlsym,dlsym@GLIBC_2.17");
static long (*real_syscall)(long, ...);

__attribute__((constructor)) static void initialize(void) {
    real_syscall = dlsym(RTLD_NEXT, "syscall");
}

long syscall(long number, long a, long b, long mode, long mask, long size,
             long flags) {
    if (!real_syscall) {
        errno = ENOSYS;
        return -1;
    }
    long result = real_syscall(number, a, b, mode, mask, size, flags);
    if (number == SYS_mbind && mode == 3 && flags == 0 &&
        result == -1 && errno == ENOSYS)
        return 0;
    return result;
}
