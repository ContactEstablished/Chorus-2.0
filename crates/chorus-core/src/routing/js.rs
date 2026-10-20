//! JavaScript's number formatting and string order, so routing's reasons, warnings and
//! rationales match Chorus 1's byte for byte: `String(n)`, `toFixed`, `toPrecision`, `Math.max`
//! and the code-unit `<` its sorts use. Rounding follows the ECMAScript rules (a tie rounds the
//! magnitude up), not Rust's `{:.N}`, which rounds a tie to even.

use std::cmp::Ordering;

/// Code-unit (UTF-16) order, never locale order, so `"B" < "a"`. Chorus 1's `byCodeUnit`.
pub fn by_code_unit(a: &str, b: &str) -> Ordering {
    a.encode_utf16().cmp(b.encode_utf16())
}

/// `Math.max(a, b)`: NaN wins, and `+0` beats `-0`.
pub fn max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a > b || (a == b && a.is_sign_positive()) {
        a
    } else {
        b
    }
}

/// `Math.min(a, b)`: NaN wins, and `-0` beats `+0`.
pub fn min(a: f64, b: f64) -> f64 {
    -max(-a, -b)
}

/// Splits `{:e}`-style text (`"1.2345e-7"`) into its digits, without the point, and exponent.
fn split_scientific(text: &str) -> (Vec<u8>, i32) {
    let (mantissa, exponent) = text.split_once('e').unwrap_or((text, "0"));
    let digits: Vec<u8> = mantissa.bytes().filter(u8::is_ascii_digit).map(|d| d - b'0').collect();
    (digits, exponent.parse().unwrap_or(0))
}

/// The shortest digits that round-trip `x` (finite, non-zero, positive) and the exponent of the
/// first one: `x = d1.d2d3… × 10^e`.
fn shortest_digits(x: f64) -> (Vec<u8>, i32) {
    let (mut digits, e) = split_scientific(&format!("{x:e}"));
    while digits.len() > 1 && digits.last() == Some(&0) {
        digits.pop();
    }
    (digits, e)
}

/// The exact decimal expansion of `x` (finite, non-zero, positive): every double has fewer than
/// 800 significant digits, so this is exact, not rounded.
fn exact_digits(x: f64) -> (Vec<u8>, i32) {
    split_scientific(&format!("{x:.1100e}"))
}

fn digit_string(digits: &[u8]) -> String {
    digits.iter().map(|d| char::from(b'0' + d)).collect()
}

/// Rounds `digits` to `keep` digits, a tie up; true when it carried into a new leading digit.
fn round_half_up(digits: &mut Vec<u8>, keep: usize) -> bool {
    let up = digits.get(keep).is_some_and(|d| *d >= 5);
    digits.resize(keep, 0);
    if !up {
        return false;
    }
    for d in digits.iter_mut().rev() {
        if *d == 9 {
            *d = 0;
        } else {
            *d += 1;
            return false;
        }
    }
    digits.insert(0, 1);
    true
}

/// `String(x)`: the shortest round-trip digits, in exponent form below `1e-6` and from `1e21`.
pub fn number(x: f64) -> String {
    if x.is_nan() {
        return "NaN".into();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity".into() } else { "-Infinity".into() };
    }
    if x == 0.0 {
        return "0".into();
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let (digits, e) = shortest_digits(x.abs());
    let s = digit_string(&digits);
    let k = digits.len() as i32;
    let n = e + 1;
    let body = if k <= n && n <= 21 {
        format!("{s}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &s[..n as usize], &s[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{s}", "0".repeat((-n) as usize))
    } else {
        let rest = if k > 1 { format!(".{}", &s[1..]) } else { String::new() };
        let exp = n - 1;
        format!("{}{rest}e{}{}", &s[..1], if exp >= 0 { "+" } else { "-" }, exp.abs())
    };
    format!("{sign}{body}")
}

/// `x.toFixed(fraction_digits)`.
pub fn to_fixed(x: f64, fraction_digits: usize) -> String {
    if !x.is_finite() || x.abs() >= 1e21 {
        return number(x);
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let text = format!("{:.1100}", x.abs());
    let (int_part, frac_part) = text.split_once('.').unwrap_or((&text, ""));
    let mut digits: Vec<u8> = int_part.bytes().chain(frac_part.bytes()).map(|d| d - b'0').collect();
    let keep = int_part.len() + fraction_digits;
    let int_len = int_part.len() + usize::from(round_half_up(&mut digits, keep));
    let s = digit_string(&digits);
    if fraction_digits == 0 {
        format!("{sign}{s}")
    } else {
        format!("{sign}{}.{}", &s[..int_len], &s[int_len..])
    }
}

/// `x.toPrecision(precision)`, for a precision of 1 to 100.
pub fn to_precision(x: f64, precision: usize) -> String {
    if !x.is_finite() {
        return number(x);
    }
    let p = precision.clamp(1, 100);
    if x == 0.0 {
        return if p == 1 { "0".into() } else { format!("0.{}", "0".repeat(p - 1)) };
    }
    let sign = if x < 0.0 { "-" } else { "" };
    let (mut digits, mut e) = exact_digits(x.abs());
    if round_half_up(&mut digits, p) {
        digits.truncate(p);
        e += 1;
    }
    let m = digit_string(&digits);
    let p_i = p as i32;
    let body = if e < -6 || e >= p_i {
        let rest = if p > 1 { format!(".{}", &m[1..]) } else { String::new() };
        format!("{}{rest}e{}{}", &m[..1], if e > 0 { "+" } else { "-" }, e.abs())
    } else if e == p_i - 1 {
        m
    } else if e >= 0 {
        let at = (e + 1) as usize;
        format!("{}.{}", &m[..at], &m[at..])
    } else {
        format!("0.{}{m}", "0".repeat((-(e + 1)) as usize))
    };
    format!("{sign}{body}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_unit_order_is_not_locale_order() {
        assert_eq!(by_code_unit("a", "b"), Ordering::Less);
        assert_eq!(by_code_unit("b", "a"), Ordering::Greater);
        assert_eq!(by_code_unit("a", "a"), Ordering::Equal);
        assert_eq!(by_code_unit("B", "a"), Ordering::Less, "locale order would put 'a' first");
        let mut v = ["b", "a", "B", "A", "a/fp8", "a"];
        v.sort_by(|a, b| by_code_unit(a, b));
        assert_eq!(v, ["A", "B", "a", "a", "a/fp8", "b"]);
        // UTF-16 order differs from code-point order above the BMP.
        assert_eq!(by_code_unit("\u{FF61}", "\u{1F600}"), Ordering::Greater);
    }

    #[test]
    fn numbers_print_as_javascript_prints_them() {
        for (x, s) in [
            (99.5, "99.5"),
            (95.0, "95"),
            (100.0, "100"),
            (-2.0, "-2"),
            (0.1, "0.1"),
            (1.01, "1.01"),
            (-0.0, "0"),
            (1e21, "1e+21"),
            (1.5e21, "1.5e+21"),
            (123_456_789_012_345_680_000.0, "123456789012345680000"),
            (1e-7, "1e-7"),
            (1.25e-7, "1.25e-7"),
            (0.000001, "0.000001"),
            (1456.5000000000027, "1456.5000000000027"),
        ] {
            assert_eq!(number(x), s, "{x:e}");
        }
        assert_eq!(number(f64::NAN), "NaN");
        assert_eq!(number(f64::INFINITY), "Infinity");
    }

    #[test]
    fn to_fixed_rounds_a_tie_up() {
        assert_eq!(to_fixed(48.46213, 1), "48.5");
        assert_eq!(to_fixed(119.909, 1), "119.9");
        assert_eq!(to_fixed(150.0, 1), "150.0");
        assert_eq!(to_fixed(0.5, 0), "1", "Rust's {{:.0}} gives 0");
        assert_eq!(to_fixed(2.5, 0), "3");
        assert_eq!(to_fixed(0.125, 2), "0.13");
        assert_eq!(to_fixed(1.005, 2), "1.00", "1.005 is below the tie in binary");
        assert_eq!(to_fixed(9.99, 1), "10.0");
        assert_eq!(to_fixed(99.99, 2), "99.99");
        assert_eq!(to_fixed(-0.0001, 2), "-0.00");
        assert_eq!(to_fixed(-0.0, 2), "0.00");
        assert_eq!(to_fixed(0.0, 0), "0");
        assert_eq!(to_fixed(1e21, 2), "1e+21");
    }

    #[test]
    fn to_precision_follows_ecmascript() {
        assert_eq!(to_precision(0.0165144, 3), "0.0165");
        assert_eq!(to_precision(0.044865, 3), "0.0449");
        assert_eq!(to_precision(0.30959, 3), "0.310");
        assert_eq!(to_precision(0.01942, 3), "0.0194");
        assert_eq!(to_precision(12.34, 3), "12.3");
        assert_eq!(to_precision(123.4, 3), "123");
        assert_eq!(to_precision(1234.5, 3), "1.23e+3");
        assert_eq!(to_precision(999.6, 3), "1.00e+3");
        assert_eq!(to_precision(0.00000123, 2), "0.0000012");
        assert_eq!(to_precision(1e-7, 2), "1.0e-7");
        assert_eq!(to_precision(0.125, 2), "0.13", "a tie rounds up");
        assert_eq!(to_precision(0.0, 3), "0.00");
        assert_eq!(to_precision(-1.5, 1), "-2");
        assert_eq!(to_precision(5.0, 1), "5");
    }

    #[test]
    fn max_is_math_max() {
        assert!(max(f64::NAN, 1.0).is_nan() && max(1.0, f64::NAN).is_nan());
        assert!(max(-0.0, 0.0).is_sign_positive() && max(0.0, -0.0).is_sign_positive());
        assert_eq!(max(2.0, 3.0), 3.0);
        assert_eq!(max(f64::NEG_INFINITY, -1.0), -1.0);
    }
}
