//! Human-friendly numbers for the dashboard

use std::time::Duration;

/// Latency with a unit that fits its size: 850µs, 12.34ms, 1.23s
pub fn latency(d: Duration) -> String {
    let us = d.as_micros();
    match us {
        0..=999 => format!("{us}µs"),
        1_000..=99_999 => format!("{:.2}ms", us as f64 / 1e3),
        100_000..=999_999 => format!("{:.1}ms", us as f64 / 1e3),
        _ => format!("{:.2}s", d.as_secs_f64()),
    }
}

/// At most five characters, for chart labels: 850µs, 1.2ms, 120ms, 3.4s
pub fn latency_short(us: u64) -> String {
    // Bounds sit where one decimal would round up to a two-digit integer
    match us {
        0..=999 => format!("{us}µs"),
        1_000..=9_949 => format!("{:.1}ms", us as f64 / 1e3),
        9_950..=999_499 => format!("{}ms", (us + 500) / 1_000),
        999_500..=9_949_999 => format!("{:.1}s", us as f64 / 1e6),
        _ => format!("{}s", (us + 500_000) / 1_000_000),
    }
}

/// 1234567 → "1,234,567"
pub fn count(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// Short form for rates and axis labels: 950, 12.3k, 1.2M
pub fn compact(v: f64) -> String {
    let v = v.max(0.0);
    if v < 1_000.0 {
        if v < 10.0 && v.fract() != 0.0 {
            format!("{v:.1}")
        } else {
            format!("{v:.0}")
        }
    } else if v < 1e6 {
        format!("{:.1}k", v / 1e3)
    } else if v < 1e9 {
        format!("{:.1}M", v / 1e6)
    } else {
        format!("{:.1}G", v / 1e9)
    }
}

/// Byte size in binary units: 512 B, 2.0 KiB, 1.5 MiB
pub fn bytes(v: f64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut v = v.max(0.0);
    let mut unit = 0;
    while v >= 1024.0 && unit < UNITS.len() - 1 {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{v:.0} B")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}

/// Elapsed time: 07.3s, 02:07.3, 1:02:07
pub fn clock(d: Duration) -> String {
    let secs = d.as_secs();
    let tenths = d.subsec_millis() / 100;
    match secs {
        0..=59 => format!("{secs:02}.{tenths}s"),
        60..=3599 => format!("{:02}:{:02}.{tenths}", secs / 60, secs % 60),
        _ => format!("{}:{:02}:{:02}", secs / 3600, secs % 3600 / 60, secs % 60),
    }
}

/// Elapsed time to the millisecond, for single requests: 07.312s, 02:07.312
pub fn clock_ms(d: Duration) -> String {
    let secs = d.as_secs();
    let ms = d.subsec_millis();
    match secs {
        0..=59 => format!("{secs:02}.{ms:03}s"),
        _ => format!("{:02}:{:02}.{ms:03}", secs / 60, secs % 60),
    }
}

/// Whole seconds for run lengths: 45s, 3m, 2m30s, 1h5m
pub fn span(d: Duration) -> String {
    let secs = d.as_secs();
    let (h, m, s) = (secs / 3600, secs % 3600 / 60, secs % 60);
    match (h, m, s) {
        (0, 0, s) => format!("{s}s"),
        (0, m, 0) => format!("{m}m"),
        (0, m, s) => format!("{m}m{s}s"),
        (h, 0, _) => format!("{h}h"),
        (h, m, _) => format!("{h}h{m}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_picks_a_unit() {
        let us = Duration::from_micros;
        assert_eq!(latency(us(850)), "850µs");
        assert_eq!(latency(us(12_345)), "12.35ms");
        assert_eq!(latency(us(250_000)), "250.0ms");
        assert_eq!(latency(us(1_500_000)), "1.50s");
    }

    #[test]
    fn short_latency_fits_five_chars() {
        for us in [
            0, 999, 1_000, 9_949, 9_999, 10_000, 999_499, 999_999, 1_000_000, 9_999_999, 59_000_000,
        ] {
            assert!(latency_short(us).chars().count() <= 5, "{us}");
        }
        assert_eq!(latency_short(1_250), "1.2ms");
        assert_eq!(latency_short(120_000), "120ms");
    }

    #[test]
    fn counts_get_separators() {
        assert_eq!(count(0), "0");
        assert_eq!(count(999), "999");
        assert_eq!(count(1_000), "1,000");
        assert_eq!(count(1_234_567), "1,234,567");
    }

    #[test]
    fn compact_and_bytes() {
        assert_eq!(compact(0.5), "0.5");
        assert_eq!(compact(950.0), "950");
        assert_eq!(compact(151_547.0), "151.5k");
        assert_eq!(compact(2_500_000.0), "2.5M");
        assert_eq!(bytes(512.0), "512 B");
        assert_eq!(bytes(2048.0), "2.0 KiB");
        assert_eq!(bytes(1.5 * 1024.0 * 1024.0), "1.5 MiB");
    }

    #[test]
    fn clock_and_span() {
        assert_eq!(clock(Duration::from_millis(7_300)), "07.3s");
        assert_eq!(clock(Duration::from_millis(127_300)), "02:07.3");
        assert_eq!(clock(Duration::from_secs(3_727)), "1:02:07");
        assert_eq!(clock_ms(Duration::from_millis(7_312)), "07.312s");
        assert_eq!(clock_ms(Duration::from_millis(127_005)), "02:07.005");
        assert_eq!(span(Duration::from_secs(45)), "45s");
        assert_eq!(span(Duration::from_secs(180)), "3m");
        assert_eq!(span(Duration::from_secs(150)), "2m30s");
        assert_eq!(span(Duration::from_secs(3_900)), "1h5m");
    }
}
