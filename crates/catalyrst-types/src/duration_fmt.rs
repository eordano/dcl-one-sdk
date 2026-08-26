//! Canonical user-facing duration display.
//!
//! Format rules (user-facing text only):
//! - up to 9,999us: whole microseconds with comma thousands
//!   separators, e.g. "320us", "9,999us" (mu is U+00B5)
//! - 10ms up to 9,999ms: integer milliseconds with comma thousands
//!   separators, e.g. "42ms", "2,321ms", "9,999ms"
//! - 10s up to 10 minutes: seconds with exactly 2 decimals, e.g. "83.45s"
//! - above 10 minutes: hours/minutes/seconds, e.g. "0h:11m:23.45s"
//!
//! Keep in sync with the reference implementation in
//! catalyrst/crates/dcl-one-sdk/src/ux.rs (fmt_elapsed).

use std::time::Duration;

/// Format an elapsed [`Duration`] for user-facing display.
pub fn fmt_elapsed(d: Duration) -> String {
    // Both grouped tiers cap at 9,999, so grouping is at most one comma.
    fn grouped(n: u32, unit: &str) -> String {
        if n < 1_000 {
            format!("{n}{unit}")
        } else {
            format!("{},{:03}{unit}", n / 1_000, n % 1_000)
        }
    }
    let secs = d.as_secs();
    if secs < 10 {
        let micros = secs as u32 * 1_000_000 + d.subsec_micros();
        if micros < 10_000 {
            grouped(micros, "\u{00b5}s")
        } else {
            grouped(micros / 1_000, "ms")
        }
    } else if secs < 600 || (secs == 600 && d.subsec_nanos() == 0) {
        format!("{:.2}s", d.as_secs_f64())
    } else {
        // Work in rounded centiseconds so 11m59.999s carries to 12m rather
        // than printing "11m:60.00s".
        let cs = (d.as_secs_f64() * 100.0).round() as u64;
        let h = cs / 360_000;
        let m = (cs % 360_000) / 6_000;
        let s = (cs % 6_000) as f64 / 100.0;
        format!("{h}h:{m:02}m:{s:05.2}s")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fmt_elapsed_tiers() {
        // microseconds up to 9,999us, with thousands separators
        assert_eq!(fmt_elapsed(Duration::from_micros(320)), "320\u{00b5}s");
        assert_eq!(fmt_elapsed(Duration::from_micros(1_234)), "1,234\u{00b5}s");
        assert_eq!(fmt_elapsed(Duration::from_micros(9_999)), "9,999\u{00b5}s");
        // 10ms enters the millisecond tier
        assert_eq!(fmt_elapsed(Duration::from_millis(10)), "10ms");
        assert_eq!(fmt_elapsed(Duration::from_millis(2_321)), "2,321ms");
        assert_eq!(fmt_elapsed(Duration::from_millis(9_999)), "9,999ms");
        // sub-millisecond remainders truncate, matching Duration::as_millis
        assert_eq!(fmt_elapsed(Duration::from_micros(9_999_999)), "9,999ms");
        // 10s enters the seconds tier
        assert_eq!(fmt_elapsed(Duration::from_millis(10_000)), "10.00s");
        assert_eq!(fmt_elapsed(Duration::from_millis(83_450)), "83.45s");
        // 10 minutes inclusive stays in seconds
        assert_eq!(fmt_elapsed(Duration::from_secs(600)), "600.00s");
        // above 10 minutes: h:m:s
        assert_eq!(fmt_elapsed(Duration::from_millis(600_001)), "0h:10m:00.00s");
        assert_eq!(fmt_elapsed(Duration::from_millis(683_450)), "0h:11m:23.45s");
        assert_eq!(
            fmt_elapsed(Duration::from_millis(3_723_400)),
            "1h:02m:03.40s"
        );
        // rounding carries across the minute boundary
        assert_eq!(fmt_elapsed(Duration::from_millis(719_999)), "0h:12m:00.00s");
    }
}
