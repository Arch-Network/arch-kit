use crate::error::{CliError, Result};

pub(crate) fn format_amount(amount: u64, decimals: u8) -> String {
    if amount == 0 || decimals == 0 {
        return amount.to_string();
    }

    let digits = amount.to_string();
    let decimals = usize::from(decimals);
    let mut formatted = if digits.len() <= decimals {
        format!("0.{}{}", "0".repeat(decimals - digits.len()), digits)
    } else {
        let split = digits.len() - decimals;
        format!("{}.{}", &digits[..split], &digits[split..])
    };
    while formatted.ends_with('0') {
        formatted.pop();
    }
    if formatted.ends_with('.') {
        formatted.pop();
    }
    formatted
}

pub(crate) fn parse_amount(value: &str, decimals: u8) -> Result<u64> {
    let value = value.trim();
    let (whole, fraction) = match value.split_once('.') {
        Some((whole, fraction)) if !fraction.contains('.') => (whole, fraction),
        Some(_) => {
            return Err(invalid_amount(
                value,
                "must contain at most one decimal point",
            ));
        }
        None => (value, ""),
    };

    if whole.is_empty()
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fraction.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(invalid_amount(
            value,
            "must be a non-negative decimal number without exponent notation",
        ));
    }

    let decimals = usize::from(decimals);
    if fraction.len() > decimals {
        return Err(invalid_amount(
            value,
            &format!("has more than {decimals} fractional digits"),
        ));
    }

    let mut raw = String::with_capacity(whole.len() + decimals);
    raw.push_str(whole);
    raw.push_str(fraction);
    raw.extend(std::iter::repeat_n('0', decimals - fraction.len()));
    let raw = raw.trim_start_matches('0');
    let amount = if raw.is_empty() {
        0
    } else {
        raw.parse::<u64>()
            .map_err(|_| invalid_amount(value, "does not fit in a u64 token amount"))?
    };
    if amount == 0 {
        return Err(invalid_amount(value, "must be greater than zero"));
    }
    Ok(amount)
}

fn invalid_amount(value: &str, detail: &str) -> CliError {
    CliError::InvalidArgument(format!("token amount '{value}' {detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_token_amounts_without_floating_point() {
        assert_eq!(format_amount(0, 9), "0");
        assert_eq!(format_amount(42, 0), "42");
        assert_eq!(format_amount(42, 4), "0.0042");
        assert_eq!(format_amount(1_230_000, 6), "1.23");
        assert_eq!(format_amount(u64::MAX, 9), "18446744073.709551615");
    }

    #[test]
    fn parses_decimal_token_amounts_without_floating_point() {
        assert_eq!(parse_amount("12", 3).unwrap(), 12_000);
        assert_eq!(parse_amount("1.25", 6).unwrap(), 1_250_000);
        assert_eq!(parse_amount("0.000001", 6).unwrap(), 1);
        assert_eq!(parse_amount("42", 0).unwrap(), 42);
        assert_eq!(parse_amount("18446744073.709551615", 9).unwrap(), u64::MAX);
    }

    #[test]
    fn rejects_invalid_decimal_token_amounts() {
        for value in ["", "0", "0.0", "-1", "+1", ".5", "1e3", "1.2.3"] {
            assert!(parse_amount(value, 6).is_err(), "accepted {value}");
        }
        assert!(parse_amount("1.001", 2).is_err());
        assert!(parse_amount("1.0", 0).is_err());
        assert!(parse_amount("18446744073709551616", 0).is_err());
    }
}
