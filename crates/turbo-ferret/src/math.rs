//! Small `f32` helpers so the `no_std` core needs no `libm`.

/// Largest f32 below which values may have a fractional part.
const FRAC_LIMIT: f32 = 8_388_608.0; // 2^23

pub fn abs(x: f32) -> f32 {
    if x < 0.0 { -x } else { x }
}

pub fn trunc(x: f32) -> f32 {
    if !(abs(x) < FRAC_LIMIT) {
        return x; // large, inf or NaN
    }
    (x as i32) as f32
}

pub fn floor(x: f32) -> f32 {
    let t = trunc(x);
    if t > x { t - 1.0 } else { t }
}

pub fn ceil(x: f32) -> f32 {
    let t = trunc(x);
    if t < x { t + 1.0 } else { t }
}

/// Round half away from zero (like C `roundf`).
pub fn round(x: f32) -> f32 {
    if x >= 0.0 { floor(x + 0.5) } else { ceil(x - 0.5) }
}

/// Square root: bit-trick estimate refined by Newton steps.
pub fn sqrt(x: f32) -> f32 {
    if x <= 0.0 || x.is_nan() {
        return if x == 0.0 { 0.0 } else { f32::NAN };
    }
    if x.is_infinite() {
        return x;
    }
    let mut y = f32::from_bits((x.to_bits() >> 1) + 0x1fbd_1df5);
    for _ in 0..3 {
        y = 0.5 * (y + x / y);
    }
    y
}

/// e^x: range reduction by ln 2, then a polynomial on the remainder.
pub fn exp(x: f32) -> f32 {
    if x.is_nan() {
        return x;
    }
    if x > 88.7 {
        return f32::INFINITY;
    }
    if x < -103.0 {
        return 0.0;
    }
    const LN2: f32 = core::f32::consts::LN_2;
    let k = floor(x / LN2 + 0.5);
    let r = x - k * LN2; // |r| <= ln2/2
    // Taylor to r^7 is accurate to ~1e-7 relative on this range.
    let mut p = 1.0 / 5040.0;
    p = p * r + 1.0 / 720.0;
    p = p * r + 1.0 / 120.0;
    p = p * r + 1.0 / 24.0;
    p = p * r + 1.0 / 6.0;
    p = p * r + 0.5;
    p = p * r + 1.0;
    p = p * r + 1.0;
    // scale by 2^k (k in [-149, 128])
    let mut k = k as i32;
    let mut result = p;
    while k > 127 {
        result *= f32::from_bits(254u32 << 23); // 2^127
        k -= 127;
    }
    while k < -126 {
        result *= f32::from_bits(1 << 23); // 2^-126
        k += 126;
    }
    result * f32::from_bits(((k + 127) as u32) << 23)
}

/// Integer power of an f32.
pub fn powi(mut x: f32, mut n: i32) -> f32 {
    if n < 0 {
        x = 1.0 / x;
        n = -n;
    }
    let mut r = 1.0;
    while n > 0 {
        if n & 1 == 1 {
            r *= x;
        }
        x *= x;
        n >>= 1;
    }
    r
}

/// Round an f32 to the nearest i32.
pub fn round_i32(x: f32) -> i32 {
    round(x) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rounding() {
        assert_eq!(floor(1.5), 1.0);
        assert_eq!(floor(-1.5), -2.0);
        assert_eq!(floor(-2.0), -2.0);
        assert_eq!(ceil(1.2), 2.0);
        assert_eq!(ceil(-1.2), -1.0);
        assert_eq!(round(2.5), 3.0);
        assert_eq!(round(-2.5), -3.0);
        assert_eq!(round(2.4), 2.0);
        assert_eq!(floor(1e10), 1e10);
        assert_eq!(trunc(-3.7), -3.0);
    }

    #[test]
    fn sqrt_accuracy() {
        for &v in &[0.0f32, 1e-6, 0.25, 1.0, 2.0, 3.0, 100.0, 12345.678, 1e20] {
            let s = sqrt(v);
            let want = (v as f64).sqrt() as f32;
            assert!(abs(s - want) <= want * 1e-6 + 1e-12, "sqrt({v}) = {s}, want {want}");
        }
    }

    #[test]
    fn exp_accuracy() {
        let mut x = -20.0f32;
        while x < 20.0 {
            let got = exp(x);
            let want = (x as f64).exp() as f32;
            assert!(abs(got - want) <= want * 2e-6, "exp({x}) = {got}, want {want}");
            x += 0.37;
        }
        assert_eq!(exp(0.0), 1.0);
        assert_eq!(exp(-200.0), 0.0);
        assert!(exp(100.0).is_infinite());
    }

    #[test]
    fn powi_works() {
        assert_eq!(powi(2.0, 10), 1024.0);
        assert_eq!(powi(2.0, -1), 0.5);
        assert_eq!(powi(0.5, 3), 0.125);
    }
}
