// Java's Double.toString and Float.toString (the shortest decimal that reads
// back as the same value, closest to it where several are as short, in
// plain notation from 1e-3 up to 1e7 and in computerized scientific
// notation outside), as Gson writes a JSON number.

fn layout(negative: bool, digits: &str, exp: i32) -> String {
    let mut out = String::new();
    if negative {
        out.push('-');
    }
    if (-3..7).contains(&exp) {
        if exp >= 0 {
            let int_len = exp as usize + 1;
            if digits.len() <= int_len {
                out.push_str(digits);
                for _ in digits.len()..int_len {
                    out.push('0');
                }
                out.push_str(".0");
            } else {
                out.push_str(&digits[..int_len]);
                out.push('.');
                out.push_str(&digits[int_len..]);
            }
        } else {
            out.push_str("0.");
            for _ in 0..(-exp - 1) {
                out.push('0');
            }
            out.push_str(digits);
        }
    } else {
        out.push_str(&digits[..1]);
        out.push('.');
        if digits.len() > 1 {
            out.push_str(&digits[1..]);
        } else {
            out.push('0');
        }
        out.push('E');
        out.push_str(&exp.to_string());
    }
    out
}

// The digits and decimal exponent of a scientific-notation string.
fn split(sci: &str) -> (String, i32) {
    let (mantissa, exp) = sci.split_once('e').expect("scientific notation");
    (mantissa.replace('.', ""), exp.parse().expect("exponent"))
}

// The digits (and exponent) of the shortest decimal that reads back as the
// value. Rust and Java agree on it except where two decimals of that length
// are equally close to the value (it lies exactly between them): Java takes
// the one whose last digit is even, Rust the upper. `format` writes the
// value in scientific notation with a number of digits after the point,
// `shortest` with as few as read back, and `reads_back` whether a decimal
// reads back as the value.
fn java_digits(shortest: String, format: impl Fn(usize) -> String, reads_back: impl Fn(&str) -> bool) -> (String, i32) {
    let (mut digits, mut exp) = split(&shortest);
    let n = digits.len();
    if n == 1 {
        // Java writes at least two digits: of the two-digit decimals that
        // read back, the closest to the value.
        let (two, two_exp) = split(&format(1));
        if reads_back(&format!("{}e{}", two, two_exp - 1)) {
            digits = two.trim_end_matches('0').to_string();
            if digits.is_empty() {
                digits.push('0');
            }
            exp = two_exp;
        }
        return (digits, exp);
    }
    let (more, more_exp) = split(&format(n));
    let (exact, exact_exp) = split(&format(n + 30));
    let tie = more.ends_with('5') && more_exp == exact_exp && exact[more.len()..].bytes().all(|b| b == b'0');
    if !tie {
        return (digits, exp);
    }
    let lower = more[..n].to_string();
    let mut upper: Vec<u8> = lower.bytes().collect();
    let mut upper_exp = more_exp;
    let mut at = n;
    loop {
        if at == 0 {
            upper.insert(0, b'1');
            upper.pop();
            upper_exp += 1;
            break;
        }
        at -= 1;
        if upper[at] == b'9' {
            upper[at] = b'0';
        } else {
            upper[at] += 1;
            break;
        }
    }
    let upper = String::from_utf8(upper).unwrap();
    let text = |digits: &str, exp: i32| format!("{}e{}", digits, exp - digits.len() as i32 + 1);
    let lower_ok = reads_back(&text(&lower, more_exp));
    let upper_ok = reads_back(&text(&upper, upper_exp));
    // Rust gave one of them; Java gives the even one if both read back.
    if lower_ok && upper_ok {
        if (lower.as_bytes()[n - 1] - b'0') % 2 == 0 {
            (lower, more_exp)
        } else {
            (upper, upper_exp)
        }
    } else {
        (digits, exp)
    }
}

pub fn double_to_string(d: f64) -> String {
    if d.is_nan() {
        return "NaN".into();
    }
    if d.is_infinite() {
        return if d < 0.0 { "-Infinity" } else { "Infinity" }.into();
    }
    if d == 0.0 {
        return if d.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let a = d.abs();
    let (digits, exp) = java_digits(format!("{:e}", a), |n| format!("{:.*e}", n, a), |text| text.parse::<f64>().unwrap() == a);
    layout(d < 0.0, &digits, exp)
}

pub fn float_to_string(f: f32) -> String {
    if f.is_nan() {
        return "NaN".into();
    }
    if f.is_infinite() {
        return if f < 0.0 { "-Infinity" } else { "Infinity" }.into();
    }
    if f == 0.0 {
        return if f.is_sign_negative() { "-0.0" } else { "0.0" }.into();
    }
    let a = f.abs();
    let (digits, exp) = java_digits(format!("{:e}", a), |n| format!("{:.*e}", n, a), |text| text.parse::<f32>().unwrap() == a);
    layout(f < 0.0, &digits, exp)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known() {
        assert_eq!(double_to_string(1.0), "1.0");
        assert_eq!(double_to_string(0.001), "0.001");
        assert_eq!(double_to_string(0.0001), "1.0E-4");
        assert_eq!(double_to_string(1.0e7), "1.0E7");
        assert_eq!(double_to_string(9999999.0), "9999999.0");
        assert_eq!(double_to_string(123456.789), "123456.789");
        assert_eq!(double_to_string(-0.25), "-0.25");
        assert_eq!(double_to_string(1.2345678e10), "1.2345678E10");
        assert_eq!(double_to_string(100.0), "100.0");
        assert_eq!(float_to_string(0.25), "0.25");
        assert_eq!(float_to_string(0.1), "0.1");
        assert_eq!(float_to_string(360.0), "360.0");
        assert_eq!(float_to_string(1.0e10), "1.0E10");
    }

    // Against Java's output for the bit patterns in a file of
    // "<double bits hex> <Double.toString>" and "<float bits hex> <Float.toString>"
    // lines (JFMT_CASES), when given.
    #[test]
    fn against_java() {
        let Ok(path) = std::env::var("JFMT_CASES") else { return };
        let text = std::fs::read_to_string(path).unwrap();
        let mut count = 0;
        for line in text.lines() {
            let p: Vec<&str> = line.split(' ').collect();
            let got = match p[0] {
                "d" => double_to_string(f64::from_bits(u64::from_str_radix(p[1], 16).unwrap())),
                "f" => float_to_string(f32::from_bits(u32::from_str_radix(p[1], 16).unwrap())),
                other => panic!("bad case {other}"),
            };
            assert_eq!(got, p[2], "{line}");
            count += 1;
        }
        assert!(count > 0);
    }
}
