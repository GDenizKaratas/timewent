//! Idle durations are stored as integer milliseconds.
//!
//! Rounding rule: seconds → ms rounds to the nearest millisecond (halves away from zero);
//! ms → seconds is `ms / 1000.0`. Because IEEE division is correctly rounded, any value with
//! at most three decimals (e.g. `0.4`, `20.001`) comes back bit-identical to the parsed
//! decimal, so millisecond-precision samples round-trip exactly. Finer precision is dropped.

/// Seconds → whole milliseconds. NaN and negatives (never produced by the probe) become 0;
/// values beyond `i64` saturate.
pub fn secs_to_ms(secs: f64) -> i64 {
    if secs.is_nan() || secs <= 0.0 {
        return 0;
    }
    // `as` saturates for out-of-range floats, including +inf.
    (secs * 1000.0).round() as i64
}

pub fn ms_to_secs(ms: i64) -> f64 {
    ms as f64 / 1000.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(s: f64) -> f64 {
        ms_to_secs(secs_to_ms(s))
    }

    #[test]
    fn millisecond_precision_values_round_trip_bit_exactly() {
        for s in [0.0, 0.001, 0.4, 1.25, 20.001, 59.999, 86_400.123, 1e9] {
            assert_eq!(round_trip(s).to_bits(), s.to_bits(), "{s}");
        }
        for ms in 0..20_000 {
            let s: f64 = format!("{}.{:03}", ms / 1000, ms % 1000)
                .parse()
                .expect("f64");
            assert_eq!(round_trip(s).to_bits(), s.to_bits(), "{s}");
        }
    }

    #[test]
    fn finer_precision_rounds_to_nearest_millisecond() {
        assert_eq!(secs_to_ms(1.23456), 1_235);
        assert_eq!(secs_to_ms(0.0004), 0);
        assert_eq!(secs_to_ms(0.0005), 1);
        assert_eq!(secs_to_ms(2.9999), 3_000);
    }

    #[test]
    fn nonsense_inputs_clamp() {
        assert_eq!(secs_to_ms(f64::NAN), 0);
        assert_eq!(secs_to_ms(-3.0), 0);
        assert_eq!(secs_to_ms(f64::INFINITY), i64::MAX);
    }
}
