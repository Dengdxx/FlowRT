use super::*;

pub(crate) fn parse_positive_usize(raw: &str) -> std::result::Result<usize, String> {
    match raw.parse::<usize>() {
        Ok(value) if value > 0 => Ok(value),
        _ => Err("must be a positive integer".to_string()),
    }
}

pub(crate) fn parse_positive_f64(raw: &str) -> std::result::Result<f64, String> {
    match raw.parse::<f64>() {
        Ok(value) if value.is_finite() && value > 0.0 => Ok(value),
        _ => Err("must be a positive finite number".to_string()),
    }
}

pub(crate) fn parse_record_duration(raw: &str) -> std::result::Result<Duration, String> {
    let (number, unit) = raw
        .strip_suffix("ms")
        .map(|number| (number, "ms"))
        .or_else(|| raw.strip_suffix('s').map(|number| (number, "s")))
        .or_else(|| raw.strip_suffix('m').map(|number| (number, "m")))
        .unwrap_or((raw, "s"));
    let value = number.parse::<u64>().map_err(|_| {
        "duration must be a positive integer with optional ms/s/m suffix".to_string()
    })?;
    if value == 0 {
        return Err("duration must be greater than zero".to_string());
    }
    match unit {
        "ms" => Ok(Duration::from_millis(value)),
        "s" => Ok(Duration::from_secs(value)),
        "m" => Ok(Duration::from_secs(value.saturating_mul(60))),
        _ => unreachable!(),
    }
}
