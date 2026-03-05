pub const WORKSPACE_ALLOWED: &str =
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789._-";

pub fn normalize_state_name(raw: &str) -> String {
    raw.trim().to_lowercase()
}

pub fn normalize_label(raw: &str) -> String {
    raw.trim().to_lowercase()
}

pub fn sanitize_workspace_key(raw: &str) -> String {
    let sanitized: String = raw
        .trim()
        .chars()
        .map(|character| {
            if WORKSPACE_ALLOWED.contains(character) {
                character
            } else {
                '_'
            }
        })
        .collect();

    if sanitized.is_empty() {
        "issue".to_string()
    } else {
        sanitized
    }
}

pub fn parse_comma_separated(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|segment| !segment.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}
