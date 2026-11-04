use super::FingerprintLogMode;
use std::sync::LazyLock;

const TABLE_LEN: usize = 129;
const THIRD: f32 = f32::from_bits(0x3eaa_aaab);
const TWELFTH: f32 = f32::from_bits(0x3daa_aaab);
const EIGHTIETH: f32 = f32::from_bits(0x3c4c_cccd);

static TABLES: LazyLock<[[f32; TABLE_LEN]; 3]> = LazyLock::new(|| {
    let bytes: &[u8; TABLE_LEN * 3 * size_of::<f32>()] = include_bytes!("log-coefficients.f32le");
    std::array::from_fn(|table| {
        std::array::from_fn(|index| {
            let start = (table * TABLE_LEN + index) * size_of::<f32>();
            f32::from_le_bytes(bytes[start..start + size_of::<f32>()].try_into().unwrap())
        })
    })
});

pub(super) fn positive_normal(value: f32, mode: FingerprintLogMode) -> f32 {
    debug_assert!(value.is_normal() && value.is_sign_positive());
    let w = value - 1.0;
    if w.abs() < 0.0625 {
        let r = w / (2.0 + w);
        let c = w * r;
        let z = r + r;
        let z2 = z * z;
        let z3 = z * z2;
        return w + match mode {
            FingerprintLogMode::Sse => (z2 * EIGHTIETH + TWELFTH) * z3 - c,
            FingerprintLogMode::Fma => z2.mul_add(EIGHTIETH, TWELFTH).mul_add(z3, -c),
        };
    }

    let [head, tail, reciprocal] = &*TABLES;
    let bits = value.to_bits();
    let e = ((bits >> 23) as i32 - 127) as f32;
    let m = bits & 0x007f_ffff;
    let j = (m & 0x007f_0000) + ((m & 0x8000) << 1);
    let i = (j >> 16) as usize;
    let r = (f32::from_bits(j | 0x3f00_0000) - f32::from_bits(m | 0x3f00_0000)) * reciprocal[i];
    let r2 = r * r;
    let (hi, lo) = match mode {
        FingerprintLogMode::Sse => {
            let p = r + (r * THIRD + 0.5) * r2;
            (head[128] * e + head[i], (tail[128] * e - p) + tail[i])
        }
        FingerprintLogMode::Fma => {
            let p = THIRD.mul_add(r, 0.5).mul_add(r2, r);
            (
                head[128].mul_add(e, head[i]),
                tail[128].mul_add(e, -p) + tail[i],
            )
        }
    };
    hi + lo
}
