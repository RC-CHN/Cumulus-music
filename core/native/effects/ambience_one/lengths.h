#ifndef CUMULUS_AMBIENCE_ONE_LENGTHS_H
#define CUMULUS_AMBIENCE_ONE_LENGTHS_H

/* Fixed-bank1 scalar recovery. No original image/blob dependency.
 * Source: pinned neaudioeffects.dll B62C0, B6620, BEC90.
 * Valid bank1 sample-rate domain only; caller owns all storage.
 * Build with -std=c11 -fno-fast-math -ffp-contract=off and libm.
 */
#include <math.h>
#include <stdint.h>
#include <stdlib.h>

static inline float amb1_f32(float value) {
    volatile float rounded = value;
    return rounded;
}

static inline uint32_t amb1_next_seed(uint32_t value) {
    return value * UINT32_C(0x0bb38435) + UINT32_C(0x3619636b);
}

static int amb1_length_compare(const void *left, const void *right) {
    const float difference = amb1_f32(*(const float *)left - *(const float *)right);
    return difference < 0.0f ? -1 : difference > 0.0f ? 1 : 0;
}

static inline int amb1_prime_lengths(const float *values, uint32_t rate,
                                      uint32_t count, uint32_t *samples) {
    const float milliseconds = 0.001f;
    const float sample_rate = amb1_f32((float)rate);
    for (uint32_t index = 0; index < count; ++index) {
        const float converted = amb1_f32(amb1_f32(values[index] * milliseconds) * sample_rate);
        uint32_t candidate = (uint32_t)(int64_t)converted;
        if ((candidate & 1U) == 0) {
            ++candidate;
        }
        /* Original computes this exactly once, before any increments. */
        const int32_t bound = (int32_t)sqrt((double)candidate) + 1;
        unsigned attempts = 0;
        for (;;) {
            if (++attempts > 4096) return -1;
            uint32_t divisor = 3;
            while ((int32_t)divisor < bound && candidate % divisor != 0) {
                divisor += 2;
            }
            if ((int32_t)divisor < bound) {
                candidate += 2;
                continue;
            }
            if (index == 0) {
                if (candidate == 0) {
                    candidate = 2;
                    continue;
                }
            } else if (candidate == samples[index - 1]) {
                candidate += 2;
                continue;
            }
            samples[index] = candidate;
            break;
        }
    }
    return 0;
}

static inline int amb1_bank1_lengths(uint32_t rate,
                                                uint32_t out[16][4],
                                                uint32_t extra[16]) {
    const float shape[11] = {8.0f, 50.0f, 2.0f, 0.1f, 0.8f, 66.0f,
                             15.0f, 5.0f, 40.0f, 100.0f, 50.0f};
    const float one_percent = 0.01f;
    const float random_scale = 2.328306384496992e-12f;
    const float milliseconds = 0.001f;
    const float first = 100.0f;
    const float second = 100.0f;
    float values[64];
    uint32_t samples[64];
    const float center = amb1_f32(amb1_f32(amb1_f32(amb1_f32(shape[1] - shape[0]) * one_percent) * first) + shape[0]);
    const float ratio = amb1_f32(amb1_f32(amb1_f32(amb1_f32(shape[4] - shape[3]) * one_percent) * second) + shape[3]);
    const float low = amb1_f32(ratio * center);
    const float high = amb1_f32(amb1_f32(center - low) + center);
    const float span = amb1_f32(high - low);
    const float reciprocal = amb1_f32(1.0f / 63.0f);
    const float sample_rate = amb1_f32((float)rate);
    uint32_t seed = UINT32_C(0x006b4cc7);

    for (uint32_t index = 0; index < 64; ++index) {
        seed = amb1_next_seed(seed);
        const float base = amb1_f32(amb1_f32(amb1_f32(amb1_f32((float)index) * span) * reciprocal) + low);
        const float jitter = amb1_f32(amb1_f32(amb1_f32(shape[2] * random_scale) * base) * amb1_f32((float)seed));
        values[index] = amb1_f32(jitter + base);
    }
    qsort(values, 64, sizeof(values[0]), amb1_length_compare);

    if (amb1_prime_lengths(values, rate, 64, samples)) return -1;
    for (uint32_t node = 0; node < 16; ++node) {
        for (uint32_t ring = 0; ring < 4; ++ring) {
            out[node][ring] = samples[node + 16 * ring];
        }
    }

    const float extra_center = amb1_f32(shape[9] / 16.0f);
    const float extra_scale = amb1_f32(amb1_f32(shape[10] * random_scale) * extra_center);
    seed = UINT32_C(0x01bc0f99);
    for (uint32_t node = 0; node < 16; ++node) {
        seed = amb1_next_seed(seed);
        const float value = amb1_f32(amb1_f32(amb1_f32((float)seed) * extra_scale) + extra_center);
        const uint32_t frames = (uint32_t)(int64_t)amb1_f32(amb1_f32(value * milliseconds) * sample_rate);
        extra[node] = frames < 4 ? 4 : frames;
    }
    return 0;
}

/* B6020 differs from B62C0: descending geometric bases, another seed, then
 * the same sort / original fixed-bound odd-divisor conversion. */
static inline int amb1_allpass_lengths(uint32_t rate, uint32_t out[4]) {
    float values[4], base = 15.0f;
    uint32_t seed = UINT32_C(0x002ee007);
    for (unsigned i = 0; i < 4; ++i) {
        float jitter = amb1_f32(5.0f * 2.328306384496992e-12f);
        jitter = amb1_f32(jitter * base);
        seed = amb1_next_seed(seed);
        jitter = amb1_f32(jitter * amb1_f32((float)seed));
        values[i] = amb1_f32(jitter + base);
        const float ratio = amb1_f32(66.0f * 0.01f);
        base = amb1_f32(base * ratio);
    }
    qsort(values, 4, sizeof(values[0]), amb1_length_compare);
    return amb1_prime_lengths(values, rate, 4, out);
}

#endif
