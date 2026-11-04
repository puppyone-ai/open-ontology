#define _GNU_SOURCE
#include <assert.h>
#include <dlfcn.h>
#include <errno.h>
#include <stdio.h>
#include <sys/mman.h>
#include <sys/syscall.h>
#include <unistd.h>

int main(void) {
    void *libc = dlopen("libc.so.6", RTLD_NOW);
    assert(libc);
    long (*native)(long, ...) = dlsym(libc, "syscall");
    assert(native);
    assert(syscall(SYS_getpid) == getpid());
    void *memory = mmap(NULL, 4096, PROT_READ | PROT_WRITE,
                        MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
    assert(memory != MAP_FAILED);
    unsigned long node = 1;
    assert(native(SYS_mbind, memory, 4096, 3, &node, 64, 0) == -1);
    assert(errno == ENOSYS); /* This test requires the actual uniform VM. */
    assert(syscall(SYS_mbind, memory, 4096, 3, &node, 64, 0) == 0);
    assert(syscall(SYS_mbind, memory, 4096, 2, &node, 64, 0) == -1);
    assert(errno == ENOSYS); /* A different policy must not be suppressed. */
    assert(syscall(SYS_mbind, memory, 4096, 3, &node, 64, 1) == -1);
    assert(errno == ENOSYS); /* Nor a request with flags. */
    assert(munmap(memory, 4096) == 0);
    puts("uniform VM: only missing interleave hint bypassed; forwarding passed");
    return 0;
}
