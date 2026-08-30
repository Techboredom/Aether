use std::collections::BTreeMap;

use common::{ContainerStatusInfo, MyQuota};

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

/// Maps a pod phase to one of the dashboard's fixed status-badge classes.
pub fn phase_class(phase: &str) -> &'static str {
    match phase {
        "Running" | "Succeeded" => "good",
        "Pending" => "warning",
        "Failed" => "critical",
        _ => "serious",
    }
}

/// Picks the most relevant "why" reason to show next to a pod's status badge,
/// e.g. "CrashLoopBackOff" or "ImagePullBackOff" — the first non-running
/// container's reason, if any.
pub fn pod_reason(containers: &[ContainerStatusInfo]) -> Option<String> {
    containers
        .iter()
        .find(|c| c.state != "running" && c.reason.is_some())
        .and_then(|c| c.reason.clone())
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

/// Renders a template's accelerator type/count as "amd.com/gpu ×1", or "—" if none.
pub fn accelerator_summary(accelerator_type: &str, count: Option<i64>) -> String {
    if accelerator_type.trim().is_empty() {
        return "—".to_string();
    }
    format!("{accelerator_type} ×{}", count.unwrap_or(1))
}

/// Renders a "used / limit" summary line for the Launch tab and the Pods
/// tab's manage panel, e.g. "CPU limit: 1.5 / 4 cores · Memory limit: 2Gi /
/// 16Gi · GPUs: 0 / 2". A dimension with no configured limit is shown as
/// "used / unlimited".
pub fn quota_summary(quota: &MyQuota) -> String {
    let cpu_used = quota.used_cpu_millicores as f64 / 1000.0;
    let cpu = match &quota.limits.cpu_limit {
        Some(limit) => format!("{cpu_used:.2} / {limit} cores"),
        None => format!("{cpu_used:.2} cores / unlimited"),
    };
    let mem_used = bytes(Some(quota.used_memory_bytes));
    let mem = match &quota.limits.memory_limit {
        Some(limit) => format!("{mem_used} / {limit}"),
        None => format!("{mem_used} / unlimited"),
    };
    let gpu = match quota.limits.gpu_limit {
        Some(limit) => format!("{} / {limit}", quota.used_gpu_count),
        None => format!("{} / unlimited", quota.used_gpu_count),
    };
    format!("CPU limit: {cpu} · Memory limit: {mem} · GPUs: {gpu}")
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
