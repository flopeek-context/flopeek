//! Diagnostic metadata validation.

#[allow(unused_imports)]
use super::*;

pub(super) fn validate_string_list(name: &str, values: &[String]) -> Result<(), String> {
    validate_list(name, values, MAX_LIST_ITEMS)?;
    for value in values {
        validate_text(name, value)?;
    }
    Ok(())
}

pub(super) fn validate_list<T>(name: &str, values: &[T], max: usize) -> Result<(), String> {
    if values.len() > max {
        return Err(format!("{name} exceeds the bound of {max} items."));
    }
    Ok(())
}

pub(super) fn validate_text(name: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > MAX_TEXT_BYTES || value.contains('\0') {
        return Err(format!("{name} must be non-empty and bounded."));
    }
    Ok(())
}

pub(super) fn validate_id(name: &str, value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > MAX_ID_BYTES
        || value
            .bytes()
            .any(|byte| !(byte.is_ascii_alphanumeric() || b"._:-".contains(&byte)))
    {
        return Err(format!("{name} must be a bounded stable identifier."));
    }
    Ok(())
}

pub(super) fn validate_choice(name: &str, value: &str, allowed: &[&str]) -> Result<(), String> {
    if allowed.contains(&value) {
        Ok(())
    } else {
        Err(format!("Unsupported {name} {value:?}."))
    }
}

pub(super) fn validate_revision(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 128
        || value.chars().any(char::is_whitespace)
        || value.contains([';', '|', '&', '`', '\0'])
    {
        return Err("Git revision must be a bounded single token.".to_string());
    }
    Ok(())
}

pub(super) fn is_typescript_path(path: &str) -> bool {
    path.ends_with(".ts") || path.ends_with(".tsx")
}
