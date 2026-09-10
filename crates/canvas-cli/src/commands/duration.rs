//! `DURATION` operands: `--since` (§12.6) and `--alarm` (§12.5).

use jiff::Span;

/// Parse `<N><unit>` with unit `m`, `h`, `d`, or `w` (for example `24h`).
///
/// Days and weeks become hours, so the span stays exact and needs no
/// reference date to measure.
#[must_use]
pub fn parse_duration(raw: &str) -> Option<Span> {
    let (amount, unit) = split_unit(raw)?;
    match unit {
        'm' => Span::new().try_minutes(amount).ok(),
        'h' => Span::new().try_hours(amount).ok(),
        'd' => Span::new().try_hours(amount.checked_mul(24)?).ok(),
        'w' => Span::new().try_hours(amount.checked_mul(24 * 7)?).ok(),
        _ => None,
    }
}

/// Split `<N><unit>` into a positive amount and its unit character.
///
/// The unit is taken as a whole `char`, so an operand ending in a multi-byte
/// character is rejected instead of splitting inside it.
fn split_unit(raw: &str) -> Option<(i64, char)> {
    let mut chars = raw.trim().chars();
    let unit = chars.next_back()?;
    let amount: i64 = chars.as_str().parse().ok()?;
    (amount > 0).then_some((amount, unit))
}

/// Whole civil days a duration spans, rounded up, at least one.
#[must_use]
pub fn duration_in_days(span: Span) -> Option<u32> {
    let minutes = span.total(jiff::Unit::Minute).ok()?;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let days = (minutes / (24.0 * 60.0)).ceil() as i64;
    u32::try_from(days.max(1)).ok()
}

/// Convert an `--alarm` operand into an RFC 5545 duration (`24h` → `PT24H`).
///
/// An RFC form is passed through; `write_ics` validates the grammar.
#[must_use]
pub fn rfc_duration(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.starts_with('P') {
        return Some(raw.to_owned());
    }
    let (amount, unit) = split_unit(raw)?;
    match unit {
        'm' => Some(format!("PT{amount}M")),
        'h' => Some(format!("PT{amount}H")),
        'd' => Some(format!("P{amount}D")),
        'w' => Some(format!("P{amount}W")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations_parse_and_convert() {
        let hours = |raw: &str| parse_duration(raw).map(|s| s.get_hours());
        assert_eq!(hours("14d"), Some(14 * 24));
        assert_eq!(hours("6h"), Some(6));
        assert_eq!(hours("2w"), Some(2 * 7 * 24));
        assert_eq!(parse_duration("90m").map(|s| s.get_minutes()), Some(90));
        // A unit that is not one byte must be rejected, not split into.
        for bad in [
            "",
            "d",
            "0d",
            "-1d",
            "14",
            "14y",
            "1.5d",
            "14 d",
            "7\u{e9}",
            "7\u{1f600}",
            "\u{e9}",
        ] {
            assert!(parse_duration(bad).is_none(), "{bad} must be rejected");
            assert!(rfc_duration(bad).is_none(), "{bad} must be rejected");
        }

        assert_eq!(duration_in_days(parse_duration("14d").unwrap()), Some(14));
        assert_eq!(
            duration_in_days(parse_duration("6h").unwrap()),
            Some(1),
            "part of a day still needs one civil day of coverage"
        );
        assert_eq!(duration_in_days(parse_duration("36h").unwrap()), Some(2));

        assert_eq!(rfc_duration("24h").as_deref(), Some("PT24H"));
        assert_eq!(rfc_duration("30m").as_deref(), Some("PT30M"));
        assert_eq!(rfc_duration("1d").as_deref(), Some("P1D"));
        assert_eq!(rfc_duration("2w").as_deref(), Some("P2W"));
        assert_eq!(rfc_duration("PT1H30M").as_deref(), Some("PT1H30M"));
        assert!(rfc_duration("tomorrow").is_none());
    }
}
