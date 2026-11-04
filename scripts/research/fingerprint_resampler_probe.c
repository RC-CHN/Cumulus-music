#include <assert.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <inttypes.h>
#ifdef _WIN32
#include <windows.h>
typedef HMODULE Handle;
static Handle load(const char *name) { return LoadLibraryA(name); }
#define symbol GetProcAddress
#else
#include <dlfcn.h>
typedef void *Handle;
static Handle load(const char *name) { return dlopen(name, RTLD_NOW | RTLD_LOCAL); }
#define symbol dlsym
#endif

typedef struct SwrContext SwrContext;
static SwrContext *(*make)(SwrContext *, int64_t, int, int, int64_t, int, int, int, void *);
static void (*free_swr)(SwrContext **);
static int (*init)(SwrContext *);
static int (*convert)(SwrContext *, uint8_t **, int, const uint8_t **, int);
static int64_t (*delay)(SwrContext *, int64_t);
static unsigned (*version)(void);
static int (*cpu_flags)(void);
static void (*force_cpu_flags)(int);
enum { S16 = 1, FLT = 3, MONO = 4, STEREO = 3, DEST = 8000, MAX_FRAMES = 144250 };

static void die(const char *operation) {
#ifdef _WIN32
    fprintf(stderr, "%s: Windows error %lu\n", operation, (unsigned long)GetLastError());
#else
    const char *error = dlerror();
    fprintf(stderr, "%s: %s\n", operation, error ? error : "unknown");
#endif
    exit(2);
}

#define BIND(target, module, name) do { \
    void *sym = (void *)symbol(module, name); \
    if (!sym) die(name); \
    memcpy(&(target), &sym, sizeof(target)); \
} while (0)

static SwrContext *context(int out_layout, int out_format, int out_rate, int in_layout, int in_format, int in_rate) {
    SwrContext *ctx = make(NULL, out_layout, out_format, out_rate, in_layout, in_format, in_rate, 0, NULL);
    assert(ctx && init(ctx) == 0);
    return ctx;
}

static uint32_t xorshift(uint32_t *state) {
    uint32_t x = *state;
    x ^= x << 13; x ^= x >> 17; x ^= x << 5;
    return *state = x;
}

static int16_t signed_sample(uint32_t *seed) {
    return (int16_t)((int32_t)(xorshift(seed) >> 16) - 32768);
}

static int bytes_per_sample(int format) { return format == FLT ? 4 : 2; }

static void fixture(void *data, int frames, int format, int pattern) {
    uint32_t seed = UINT32_C(0x2a23a904);
    static const int16_t clip[] = {32767, -32768, 32767, -32768, 0, 16384, -16384, 1, -1, 0};
    static const float over[] = {1.5f, -1.5f, 1.0f, -1.0f, 0.0f, 0.999969482421875f, -1.000030517578125f, 0.5f, -0.5f, 0.0f};
    for (int i = 0; i < frames; ++i) {
        const int16_t a = signed_sample(&seed), b = signed_sample(&seed);
        if (format == S16) {
            int16_t *const pcm = data;
            if (!pattern) {
                pcm[2*i] = i == 0 ? 32767 : (i == frames - 1 ? -32768 : a);
                pcm[2*i+1] = b;
            } else {
                const int block = (i / 233) % 10;
                pcm[2*i] = clip[block];
                pcm[2*i+1] = (i / 233) % 3 ? clip[block] : clip[(block+1)%10];
            }
        } else {
            float *const pcm = data;
            if (!pattern) {
                pcm[2*i] = (i == 0 ? 32767 : (i == frames - 1 ? -32768 : a)) * (1.0f / 32768.0f);
                pcm[2*i+1] = b * (1.0f / 32768.0f);
            } else {
                const int block = (i / 233) % 10;
                pcm[2*i] = over[block];
                pcm[2*i+1] = (i / 233) % 3 ? over[block] : over[(block+1)%10];
            }
        }
    }
}

static int frame_capacity(SwrContext *ctx, int input_frames, int in_rate, int out_rate) {
    const int64_t d = delay(ctx, in_rate);
    assert(d >= 0 && d <= MAX_FRAMES);
    const int64_t cap = ((d + input_frames) * out_rate + in_rate - 1) / in_rate;
    assert(cap >= 0 && cap <= MAX_FRAMES + 512);
    return (int)cap;
}

static int one(SwrContext *ctx, void *out, int cap, const void *in, int count) {
    uint8_t *out_p[1] = {out};
    const uint8_t *in_p[1] = {in};
    const int got = convert(ctx, out_p, cap, in ? in_p : NULL, count);
    assert(got >= 0 && got <= cap);
    return got;
}

static int chunk(int rate, int split, int packet, int left) {
    static const int pattern[] = {1,7,31,17,146,2,509,11,960,3,441,113,4};
    int frames = left;
    if (split == 1) frames = rate/100;
    if (split == 2) frames = pattern[packet%(sizeof(pattern)/sizeof(*pattern))];
    return frames < left ? frames : left;
}

static FILE *file(const char *prefix, const char *name, const char *suffix) {
    char path[160];
    const int n = snprintf(path, sizeof(path), "%s-%s-%s", prefix, name, suffix);
    assert(n > 0 && (size_t)n < sizeof(path));
    FILE *f = fopen(path, "wb");
    if (!f) die("fopen");
    return f;
}

static void run_case(const char *prefix, int rate, int format, int pattern, int split) {
    const int frames = pattern ? 4099 : rate * 3 + 250;
    const int unit = bytes_per_sample(format);
    assert(frames <= MAX_FRAMES);
    uint8_t *data = malloc((size_t)frames*unit*2);
    uint8_t *mono = malloc((MAX_FRAMES+512)*4);
    int16_t *out = malloc((MAX_FRAMES+512)*2);
    assert(data && mono && out);
    fixture(data, frames, format, pattern);
    char name[64];
    snprintf(name, sizeof(name), "%d-%s-%s-%s", rate, format == FLT ? "f32" : "i16", pattern ? "clip" : "noise",
             split == 0 ? "single" : (split == 1 ? "10ms" : "ragged"));
    FILE *fi = file(prefix, name, "input.bin");
    FILE *fm = file(prefix, name, "mono.bin");
    FILE *fo = file(prefix, name, "output.bin");
    FILE *ft = file(prefix, name, "tail.bin");
    FILE *fr = file(prefix, name, "packets.csv");
    assert(fwrite(data, 2*unit, frames, fi) == (size_t)frames);
    fclose(fi);

    SwrContext *mix = context(MONO,format,rate,STEREO,format,rate);
    SwrContext *resample = context(MONO,S16,DEST,MONO,format,rate);
    int consumed = 0, packet = 0, produced = 0, at_3s = -1, zeros = 0, low = 32767, high = -32768;
    int clipped_lo = 0, clipped_hi = 0;
    while (consumed < frames) {
        const int n = chunk(rate, split, packet, frames - consumed);
        const int64_t before_mix = delay(mix, rate);
        const int n_mono = one(mix, mono, frame_capacity(mix,n,rate,rate), data+(size_t)consumed*2*unit,n);
        assert(n_mono == n);
        assert(fwrite(mono, unit, n_mono, fm) == (size_t)n_mono);

        const int64_t before = delay(resample, rate);
        const int cap = frame_capacity(resample, n_mono, rate, DEST);
        const int got = one(resample, out, cap, mono, n_mono);
        assert(fwrite(out, sizeof(*out), got, fo) == (size_t)got);
        for (int i=0; i<got; ++i) {
            if (out[i] < low) low = out[i];
            if (out[i] > high) high = out[i];
            if (out[i] == -32768) ++clipped_lo;
            if (out[i] == 32767) ++clipped_hi;
        }
        const int64_t after = delay(resample, rate);
        fprintf(fr, "%d,%d,%d,%" PRId64 ",%d,%" PRId64 ",%d,%d,%" PRId64 "\n",
            packet, consumed, n, before_mix, n_mono, before, cap, got, after);
        consumed += n;
        if (consumed == 3*rate) at_3s = produced+got;
        produced += got;
        if (!got) ++zeros;
        ++packet;
    }

    // This additional comparison is not an official capture-loop flush.
    const int64_t delay_at_stop = delay(resample, rate);
    int tail = 0;
    for (int i=0; i<8; ++i) {
        const int got = one(resample, out, 512, NULL, 0);
        assert(fwrite(out, sizeof(*out), got, ft) == (size_t)got);
        tail += got;
        if (!got) break;
        assert(i!=7);
    }
    printf("{\"name\":\"%s\",\"frames\":%d,\"packets\":%d,\"zeroPackets\":%d,"
           "\"produced\":%d,\"producedAt3Seconds\":%d,\"inputDelayAtStop\":%" PRId64 ","
           "\"min\":%d,\"max\":%d,\"clippedLo\":%d,\"clippedHi\":%d,\"tail\":%d}\n",
        name, frames, packet, zeros, produced, at_3s, delay_at_stop, low, high, clipped_lo, clipped_hi, tail);
    free_swr(&resample);
    free_swr(&mix);
    assert(!resample && !mix);
    fclose(fm); fclose(fo); fclose(ft); fclose(fr);
    free(out); free(mono); free(data);
}

int main(int argc, char **argv) {
    assert(argc == 5);
    const Handle av = load(argv[2]);
    if (!av) die("load avutil");
    const Handle swr = load(argv[1]);
    if (!swr) die("load swresample");
    BIND(make,swr,"swr_alloc_set_opts");
    BIND(free_swr,swr,"swr_free");
    BIND(init,swr,"swr_init");
    BIND(convert,swr,"swr_convert");
    BIND(delay,swr,"swr_get_delay");
    BIND(version,swr,"swresample_version");
    BIND(cpu_flags,av,"av_get_cpu_flags");
    BIND(force_cpu_flags,av,"av_force_cpu_flags");
    assert(version() == 0x030764u);
    if (!strcmp(argv[4],"scalar")) force_cpu_flags(0);
    else assert(!strcmp(argv[4],"auto"));
    printf("{\"version\":%u,\"cpuFlags\":%d}\n", version(), cpu_flags());
    for (int rate_i=0;rate_i<2;rate_i++)
        for (int format_i=0;format_i<2;format_i++)
            for (int pattern=0;pattern<2;pattern++)
                for (int split=0;split<3;split++)
                    run_case(argv[3], rate_i ? 48000 : 44100, format_i ? FLT : S16, pattern, split);
    return 0;
}
