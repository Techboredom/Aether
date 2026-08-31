use crate::error::ApiError;

fn bad(field: &str, reason: &str) -> ApiError {
    ApiError::BadRequest(format!("{field}: {reason}"))
}

/// A Kubernetes DNS-1123 label: lowercase alphanumeric or `-`, must start and
/// end with an alphanumeric character, max 63 chars. Required for anything
/// that becomes a Kubernetes object name (Deployment/Service/Pod name).
pub fn k8s_name(field: &str, value: &str) -> Result<(), ApiError> {
    if value.is_empty() || value.len() > 63 {
        return Err(bad(field, "must be 1-63 characters"));
    }
    let bytes = value.as_bytes();
    let is_alphanumeric = |b: u8| b.is_ascii_lowercase() || b.is_ascii_digit();
    if !is_alphanumeric(bytes[0]) || !is_alphanumeric(bytes[bytes.len() - 1]) {
        return Err(bad(field, "must start and end with a lowercase letter or digit"));
    }
    if !bytes.iter().all(|&b| is_alphanumeric(b) || b == b'-') {
        return Err(bad(field, "must be lowercase alphanumeric characters or '-' (a DNS label)"));
    }
    Ok(())
}

/// A display label (template name, username): just guards against empty/huge/
/// control-character input, not a strict k8s naming scheme.
pub fn label(field: &str, value: &str, max_len: usize) -> Result<(), ApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(bad(field, "must not be empty"));
    }
    if trimmed.chars().count() > max_len {
        return Err(bad(field, &format!("must be at most {max_len} characters")));
    }
    if trimmed.chars().any(|c| c.is_control()) {
        return Err(bad(field, "must not contain control characters"));
    }
    Ok(())
}

/// An image reference: non-empty, no whitespace/control characters, capped length.
/// Full OCI reference grammar isn't validated — malformed-but-well-formed-looking
/// values are still caught by the Kubernetes API server at creation time.
pub fn image_ref(value: &str) -> Result<(), ApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(bad("image", "must not be empty"));
    }
    if trimmed.len() > 512 {
        return Err(bad("image", "must be at most 512 characters"));
    }
    if trimmed.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(bad("image", "must not contain whitespace or control characters"));
    }
    Ok(())
}

pub fn container_port(port: i32) -> Result<(), ApiError> {
    if !(1..=65535).contains(&port) {
        return Err(bad("container_port", "must be between 1 and 65535"));
    }
    Ok(())
}

/// A Kubernetes resource `Quantity` string (CPU/memory), e.g. "500m", "2", "512Mi".
/// A light format check (digits, optional decimal point, optional known unit
/// suffix) — the Kubernetes API server is the authority on full validity.
pub fn quantity(field: &str, value: &str) -> Result<(), ApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(());
    }
    if trimmed.len() > 20 {
        return Err(bad(field, "must be at most 20 characters"));
    }
    const SUFFIXES: &[&str] = &["Ki", "Mi", "Gi", "Ti", "Pi", "Ei", "m", "k", "M", "G", "T", "P", "E"];
    let numeric_part = SUFFIXES.iter().find_map(|s| trimmed.strip_suffix(s)).unwrap_or(trimmed);
    let valid = !numeric_part.is_empty()
        && numeric_part.chars().all(|c| c.is_ascii_digit() || c == '.')
        && numeric_part.matches('.').count() <= 1;
    if !valid {
        return Err(bad(field, "must look like a Kubernetes quantity, e.g. \"500m\", \"2\", \"512Mi\""));
    }
    Ok(())
}

/// An environment variable name: `[A-Za-z_][A-Za-z0-9_]*`, capped length.
pub fn env_key(value: &str) -> Result<(), ApiError> {
    if value.len() > 128 {
        return Err(bad("env key", "must be at most 128 characters"));
    }
    let mut chars = value.chars();
    let Some(first) = chars.next() else {
        return Err(bad("env key", "must not be empty"));
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(bad("env key", "must start with a letter or underscore"));
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(bad("env key", "must contain only letters, digits, and underscores"));
    }
    Ok(())
}

/// Caps the number and length of a list of freeform strings (env values,
/// command args) to prevent pathological input, e.g. thousands of huge args.
pub fn bounded_list(field: &str, items: &[impl AsRef<str>], max_items: usize, max_len: usize) -> Result<(), ApiError> {
    if items.len() > max_items {
        return Err(bad(field, &format!("must have at most {max_items} entries")));
    }
    if items.iter().any(|item| item.as_ref().len() > max_len) {
        return Err(bad(field, &format!("each entry must be at most {max_len} characters")));
    }
    Ok(())
}

/// Also doubles as a Kubernetes label *value* (the `aether.io/owner` label
/// tracking who launched a Deployment), hence the stricter start/end rule
/// beyond just "printable and short".
pub fn username(value: &str) -> Result<(), ApiError> {
    if value.len() < 3 || value.len() > 32 {
        return Err(bad("username", "must be 3-32 characters"));
    }
    if !value.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.') {
        return Err(bad("username", "must contain only letters, digits, '.', '_', or '-'"));
    }
    let bytes = value.as_bytes();
    if !bytes[0].is_ascii_alphanumeric() || !bytes[bytes.len() - 1].is_ascii_alphanumeric() {
        return Err(bad("username", "must start and end with a letter or digit"));
    }
    Ok(())
}

/// A Kubernetes node label in `key=value` form, e.g. `node-type=cpu` or
/// `nvidia.com/gpu.product=H100`. The key follows the label-key grammar (an
/// optional DNS-subdomain prefix, `/`, then a DNS-1123-ish name segment); the
/// value follows the label-value grammar (same character set as the name
/// segment, may be empty). This is a practical subset of the full spec, not
/// exhaustive — the Kubernetes API server is the final authority when the
/// nodeSelector is actually applied.
pub fn node_label(value: &str) -> Result<(), ApiError> {
    let bad_label = || bad("node_label", "must look like \"key=value\", e.g. \"node-type=cpu\"");

    fn is_segment(s: &str) -> bool {
        if s.is_empty() || s.len() > 63 {
            return false;
        }
        let bytes = s.as_bytes();
        let is_alnum = |b: u8| b.is_ascii_alphanumeric();
        is_alnum(bytes[0])
            && is_alnum(bytes[bytes.len() - 1])
            && bytes.iter().all(|&b| is_alnum(b) || b == b'-' || b == b'_' || b == b'.')
    }

    let trimmed = value.trim();
    if trimmed.len() > 317 {
        return Err(bad_label());
    }
    let Some((key, val)) = trimmed.split_once('=') else {
        return Err(bad_label());
    };
    let key_ok = match key.split_once('/') {
        Some((prefix, name)) => {
            !prefix.is_empty() && prefix.len() <= 253 && prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.') && is_segment(name)
        }
        None => is_segment(key),
    };
    if !key_ok || (!val.is_empty() && !is_segment(val)) {
        return Err(bad_label());
    }
    Ok(())
}

pub fn password(value: &str) -> Result<(), ApiError> {
    if value.len() < 8 {
        return Err(bad("password", "must be at least 8 characters"));
    }
    if value.len() > 256 {
        return Err(bad("password", "must be at most 256 characters"));
    }
    Ok(())
}
