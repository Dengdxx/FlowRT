use anyhow::Result;

pub(super) fn validate_pascal_name(raw: &str, label: &str) -> Result<String> {
    if is_pascal_case(raw) {
        Ok(raw.to_string())
    } else {
        anyhow::bail!("{label} `{raw}` must be PascalCase")
    }
}

pub(super) fn normalize_snake_name(raw: &str, label: &str) -> Result<String> {
    if raw.is_empty() {
        anyhow::bail!("{label} must not be empty");
    }
    if raw.starts_with("flowrt") || raw.starts_with("FlowRT") || raw.starts_with("Flowrt") {
        anyhow::bail!("{label} `{raw}` uses reserved flowrt prefix");
    }
    if is_snake_case(raw) {
        return Ok(raw.to_string());
    }
    let snake = pascal_or_camel_to_snake(raw)?;
    validate_snake_case(&snake, label)?;
    Ok(snake)
}

pub(super) fn validate_snake_case(raw: &str, label: &str) -> Result<()> {
    if is_snake_case(raw) {
        Ok(())
    } else {
        anyhow::bail!("{label} `{raw}` must be snake_case")
    }
}

fn is_pascal_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_uppercase()
        && chars.all(|ch| ch.is_ascii_alphanumeric())
        && name.chars().any(|ch| ch.is_ascii_lowercase())
}

fn is_snake_case(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    let mut previous_underscore = false;
    for ch in chars {
        match ch {
            '_' if !previous_underscore => previous_underscore = true,
            '_' => return false,
            'a'..='z' | '0'..='9' => previous_underscore = false,
            _ => return false,
        }
    }
    !previous_underscore
}

fn pascal_or_camel_to_snake(raw: &str) -> Result<String> {
    if !raw
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
    {
        anyhow::bail!("name `{raw}` may only contain ASCII letters, digits, or `_`");
    }
    let mut output = String::new();
    let mut previous_lower_or_digit = false;
    for ch in raw.chars() {
        if ch == '_' {
            if !output.ends_with('_') && !output.is_empty() {
                output.push('_');
            }
            previous_lower_or_digit = false;
        } else if ch.is_ascii_uppercase() {
            if previous_lower_or_digit && !output.ends_with('_') {
                output.push('_');
            }
            output.push(ch.to_ascii_lowercase());
            previous_lower_or_digit = false;
        } else {
            output.push(ch);
            previous_lower_or_digit = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        }
    }
    Ok(output.trim_matches('_').to_string())
}
