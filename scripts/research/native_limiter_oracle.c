#define _GNU_SOURCE
/* Linux x86-64 research child; invoke through original_limiter_oracle.py only.
 * No Windows imports, process initialization, or codec; memory seams only. */
#include <math.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/mman.h>
#include <sys/resource.h>
#include <unistd.h>

#define WIN __attribute__((ms_abi))
#define IMAGE 0x180000000ULL
#define TEXT_SIZE 0x1e2000
#define THUNK 0x180400000ULL
#define MAX_CAPACITY 2048
#define MAX_CHANNELS 2
#define MAX_BLOCKS 256

static struct { void *p; size_t n; } allocations[8];

static void WIN *zero_alloc(size_t count, size_t width) {
    if (!count || !width || count > 1048576 / width) _Exit(20);
    for (size_t i = 0; i < 8; ++i) {
        if (allocations[i].p) continue;
        allocations[i].p = calloc(count, width);
        if (!allocations[i].p) _Exit(21);
        allocations[i].n = count * width;
        return allocations[i].p;
    }
    _Exit(22);
}

static void WIN zero_free(void **slot) {
    void *p = *slot;
    if (!p) return;
    for (size_t i = 0; i < 8; ++i) {
        if (allocations[i].p != p) continue;
        free(p);
        allocations[i].p = NULL;
        allocations[i].n = 0;
        *slot = NULL;
        return;
    }
    _Exit(23);
}

static int owned(const void *p, size_t size) {
    uintptr_t ptr = (uintptr_t)p;
    for (size_t i = 0; i < 8; ++i) {
        uintptr_t start = (uintptr_t)allocations[i].p;
        size_t available = allocations[i].n;
        if (start && ptr >= start && ptr - start <= available
            && size <= available - (ptr - start)) return 1;
    }
    return 0;
}

static void WIN *copy_memory(void *to, const void *from, size_t n) {
    if (!owned(to, n) || !owned(from, n)) _Exit(24);
    return memmove(to, from, n);
}

static void WIN *fill_memory(void *to, int byte, size_t n) {
    if (!owned(to, n)) _Exit(25);
    return memset(to, byte, n);
}

static void seam(unsigned char *image, unsigned char *page, size_t index, uint32_t rva,
                 uintptr_t replacement) {
    unsigned char *target = page + index * 16;
    unsigned char *entry = image + rva;
    if (*entry != 0xe9) _Exit(26);
    int32_t offset = (int32_t)(THUNK + index * 16 - (IMAGE + rva + 5));
    memcpy(entry + 1, &offset, 4);
    /* movabs rax, host_address; jmp rax */
    target[0] = 0x48; target[1] = 0xb8;
    memcpy(target + 2, &replacement, 8);
    target[10] = 0xff; target[11] = 0xe0;
}

struct Parameters {
    double threshold, attack, release;
    uint32_t pad24;
    float output_gain;
    uint8_t infinite_ratio, pad33[7];
    double ratio;
    uint8_t flag48, pad49[7];
};
_Static_assert(sizeof(struct Parameters) == 56, "official limiter parameter layout");

typedef void *(WIN *Constructor)(void *);
typedef unsigned char (WIN *Initialize)(void *, uint32_t, uint32_t, uint32_t);
typedef void (WIN *Configure)(void *, struct Parameters *);
typedef void (WIN *Bypass)(void *, uint8_t);
typedef void (WIN *Reset)(void *);
typedef unsigned char (WIN *Process)(void *, float *, uint8_t *, uint32_t *);

int main(int argc, char **argv) {
    if (argc != 2) return 2;
    const struct rlimit cpu = {2, 2}, memory = {128 * 1024 * 1024, 128 * 1024 * 1024},
        core = {0, 0};
    if (setrlimit(RLIMIT_CPU, &cpu) || setrlimit(RLIMIT_AS, &memory)
        || setrlimit(RLIMIT_CORE, &core)) return 3;
    alarm(3);
    unsigned char *image = mmap((void *)IMAGE, 0x3a3000, PROT_READ | PROT_WRITE,
        MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE, -1, 0);
    unsigned char *thunks = mmap((void *)THUNK, 4096, PROT_READ | PROT_WRITE,
        MAP_PRIVATE | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE, -1, 0);
    if (image == MAP_FAILED || thunks == MAP_FAILED) return 4;
    FILE *input = fopen(argv[1], "rb");
    if (!input) return 5;
    size_t got = fread(image, 1, 0x3a3000, input);
    fclose(input);
    if (got != 0x3a2400) return 6;
    seam(image, thunks, 0, 0x2842, (uintptr_t)zero_alloc);
    seam(image, thunks, 1, 0x2d15, (uintptr_t)zero_free);
    seam(image, thunks, 2, 0x9421, (uintptr_t)copy_memory);
    seam(image, thunks, 3, 0x6c4e, (uintptr_t)fill_memory);
    if (mprotect(image + 0x1000, TEXT_SIZE, PROT_READ | PROT_EXEC)
        || mprotect(thunks, 4096, PROT_READ | PROT_EXEC)) return 7;
    uint32_t h[4];
    if (fread(h, 4, 4, stdin) != 4) return 8;
    uint32_t rate = h[0], channels = h[1], capacity = h[2], blocks = h[3];
    if ((rate != 44100 && rate != 48000 && rate != 96000)
        || !channels || channels > MAX_CHANNELS || !capacity || capacity > MAX_CAPACITY
        || !blocks || blocks > MAX_BLOCKS) return 9;
    _Alignas(16) unsigned char object[0xc8] = {0};
    if (((Constructor)(image + 0x63900))(object) != object) return 10;
    if (((Initialize)(image + 0x63eb0))(object, capacity, channels, rate) != 1) return 11;
    struct Parameters defaults = {
        .threshold = 0, .attack = (double)0.005f, .release = (double)0.1f,
        .output_gain = 0, .infinite_ratio = 1, .ratio = 4, .flag48 = 1,
    };
    ((Configure)(image + 0x654b0))(object, &defaults);
    ((Bypass)(image + 0x654a0))(object, 0);
    _Alignas(16) float samples[MAX_CAPACITY * MAX_CHANNELS];
    size_t samples_per_block = capacity * channels;
    for (uint32_t block = 0; block < blocks; ++block) {
        uint32_t params[3];
        if (fread(params, 4, 3, stdin) != 3 || params[0] > capacity
            || params[1] > 1 || params[2] > 3
            || fread(samples, 4, samples_per_block, stdin) != samples_per_block) return 12;
        uint32_t valid = params[0];
        uint8_t end = (uint8_t)params[1];
        for (size_t i = 0; i < samples_per_block; ++i)
            if (!isfinite(samples[i]) || fabsf(samples[i]) > 4.0f) return 13;
        if (params[2] == 1) ((Bypass)(image + 0x654a0))(object, 1);
        if (params[2] == 2) ((Bypass)(image + 0x654a0))(object, 0);
        if (params[2] == 3) ((Reset)(image + 0x65490))(object);
        ((Process)(image + 0x63b90))(object, samples, &end, &valid);
        if (valid > capacity || end > 1) return 14;
        uint32_t out[2] = {valid, end};
        if (fwrite(out, 4, 2, stdout) != 2
            || fwrite(samples, 4, samples_per_block, stdout) != samples_per_block) return 15;
    }
    if (fgetc(stdin) != EOF) return 16;
    ((Reset)(image + 0x63ac0))(object);
    for (size_t i = 0; i < 8; ++i) if (allocations[i].p) return 17;
    return 0;
}
