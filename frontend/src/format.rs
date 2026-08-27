use std::collections::BTreeMap;

pub fn millicores(m: Option<i64>) -> String {
    match m {
        None => "—".to_string(),
        Some(m) if m % 1000 == 0 => format!("{}", m / 1000),
        Some(m) => format!("{m}m"),
    }
}

pub fn bytes(b: Option<i64>) -> String {
    const UNITS: &[(&str, f64)] = &[
        ("Gi", 1024.0 * 1024.0 * 1024.0),
        ("Mi", 1024.0 * 1024.0),
        ("Ki", 1024.0),
    ];
    let Some(b) = b else { return "—".to_string() };
    let b = b as f64;
    for (unit, factor) in UNITS {
        if b >= *factor {
            return format!("{:.1}{unit}", b / factor);
        }
    }
    format!("{b}B")
}

pub fn accelerators(accelerators: &BTreeMap<String, i64>) -> String {
    if accelerators.is_empty() {
        return "—".to_string();
    }
    accelerators
        .iter()
        .map(|(name, count)| format!("{name}: {count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Renders a kubectl-style compact age string (e.g. "3d", "5h12m", "45s") from an
/// RFC 3339 timestamp, relative to the current time in the browser.
pub fn age(start_time: Option<&str>) -> String {
    let Some(start_time) = start_time else { return "—".to_string() };
    let started_ms = js_sys::Date::parse(start_time);
    if started_ms.is_nan() {
        return "—".to_string();
    }
    let now_ms = js_sys::Date::now();
    let secs = ((now_ms - started_ms) / 1000.0).max(0.0) as i64;

    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    let seconds = secs % 60;

    if days > 0 {
        format!("{days}d{hours}h")
    } else if hours > 0 {
        format!("{hours}h{minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m{seconds}s")
    } else {
        format!("{seconds}s")
    }
}
