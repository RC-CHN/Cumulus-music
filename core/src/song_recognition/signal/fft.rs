use super::{FRAME_HOP, WINDOW_SAMPLES};
use std::sync::LazyLock;

pub(super) const POWER_BINS: usize = WINDOW_SAMPLES / 2 + 1;

static PLAN: LazyLock<FftPlan> = LazyLock::new(|| {
    let bytes: &[u8; WINDOW_SAMPLES * 3 * size_of::<f32>()] =
        include_bytes!("fft-coefficients.f32le");
    let float = |index: usize| {
        let start = index * size_of::<f32>();
        f32::from_le_bytes(bytes[start..start + size_of::<f32>()].try_into().unwrap())
    };
    FftPlan {
        window: std::array::from_fn(float),
        twiddles: std::array::from_fn(|i| Complex {
            re: float(WINDOW_SAMPLES + i * 2),
            im: float(WINDOW_SAMPLES + i * 2 + 1),
        }),
    }
});

struct FftPlan {
    window: [f32; WINDOW_SAMPLES],
    twiddles: [Complex; WINDOW_SAMPLES],
}

#[derive(Clone, Copy, Default)]
struct Complex {
    re: f32,
    im: f32,
}

impl Complex {
    fn add(self, b: Self) -> Self {
        Self {
            re: self.re + b.re,
            im: self.im + b.im,
        }
    }

    fn sub(self, b: Self) -> Self {
        Self {
            re: self.re - b.re,
            im: self.im - b.im,
        }
    }

    fn mul(self, b: Self) -> Self {
        Self {
            re: self.re * b.re - self.im * b.im,
            im: self.re * b.im + self.im * b.re,
        }
    }

    fn power(self) -> f32 {
        self.re * self.re + self.im * self.im
    }
}

pub(super) fn for_each_power_frame(pcm: &[i16], mut visit: impl FnMut(usize, &[f32; POWER_BINS])) {
    let plan = &*PLAN;
    let mut input = [0.0; WINDOW_SAMPLES];
    let mut output = [Complex::default(); WINDOW_SAMPLES];
    let mut power = [0.0; POWER_BINS];
    for (frame, samples) in pcm.windows(WINDOW_SAMPLES).step_by(FRAME_HOP).enumerate() {
        for ((value, &sample), &window) in input.iter_mut().zip(samples).zip(&plan.window) {
            let normalized = f32::from(sample) * (1.0 / 32768.0);
            *value = normalized * window;
        }
        transform(&mut output, &input, 0, 1, &plan.twiddles);
        for (power, &value) in power.iter_mut().zip(&output) {
            *power = value.power();
        }
        visit(frame, &power);
    }
}

// Native default factors: radix four until the final radix-two leaf.
// Each complex operation uses separate f32 rounding for its scalar operations.
fn transform(
    output: &mut [Complex],
    input: &[f32; WINDOW_SAMPLES],
    origin: usize,
    stride: usize,
    twiddles: &[Complex; WINDOW_SAMPLES],
) {
    if output.len() == 2 {
        let a = Complex {
            re: input[origin],
            im: 0.0,
        };
        let b = Complex {
            re: input[origin + stride],
            im: 0.0,
        }
        .mul(twiddles[0]);
        output[0] = a.add(b);
        output[1] = a.sub(b);
        return;
    }

    let width = output.len() / 4;
    for (branch, chunk) in output.chunks_exact_mut(width).enumerate() {
        transform(chunk, input, origin + branch * stride, stride * 4, twiddles);
    }

    for i in 0..width {
        let a0 = output[i];
        let u = output[i + width].mul(twiddles[i * stride]);
        let v = output[i + 2 * width].mul(twiddles[2 * i * stride]);
        let w = output[i + 3 * width].mul(twiddles[3 * i * stride]);
        let d = a0.sub(v);
        let a = a0.add(v);
        let b = u.add(w);
        let c = u.sub(w);
        output[i] = a.add(b);
        output[i + 2 * width] = a.sub(b);
        output[i + width] = Complex {
            re: d.re + c.im,
            im: d.im - c.re,
        };
        output[i + 3 * width] = Complex {
            re: d.re - c.im,
            im: d.im + c.re,
        };
    }
}
