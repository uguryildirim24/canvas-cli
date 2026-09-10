//! RFC 5545 ICS writer (§12.5).

use jiff::{Timestamp, civil::Date};

/// One calendar item for ICS export.
#[derive(Debug, Clone)]
pub struct CalendarItem {
    /// UID kind segment (`assignment`, `event`, …).
    pub kind: String,
    /// Canvas id.
    pub id: i64,
    /// Identity key for UID domain.
    pub identity_key: String,
    /// Course code for SUMMARY prefix (optional).
    pub course_code: Option<String>,
    /// Title.
    pub title: String,
    /// True when this is a deadline (`due_at`) rather than a timed event.
    pub is_deadline: bool,
    /// Due time for deadlines.
    pub due_at: Option<Timestamp>,
    /// Start for timed events.
    pub start_at: Option<Timestamp>,
    /// End for timed events (omit when absent; never for all-day).
    pub end_at: Option<Timestamp>,
    /// All-day flag.
    pub all_day: bool,
    /// Civil all-day date (required when `all_day`).
    pub all_day_date: Option<Date>,
    /// URL.
    pub url: Option<String>,
    /// DESCRIPTION body.
    pub description: Option<String>,
    /// Optional VALARM trigger before deadlines (e.g. `PT24H`).
    pub alarm: Option<String>,
}

/// Validated calendar text and one-day conversion warnings.
#[derive(Debug)]
pub struct IcsDocument {
    pub text: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum IcsError {
    #[error("calendar item {0} is missing its start or civil date")]
    MissingStart(i64),
    #[error("calendar item {0} has an invalid property value")]
    InvalidValue(i64),
}

/// Write validated RFC 5545 text. Callers must display returned warnings.
pub fn write_ics(items: &[CalendarItem], dtstamp: Timestamp) -> Result<IcsDocument, IcsError> {
    let mut out = String::new();
    let mut warnings = Vec::new();
    push_line(&mut out, "BEGIN:VCALENDAR");
    push_line(&mut out, "VERSION:2.0");
    push_line(&mut out, "PRODID:-//canvas-cli//EN");
    push_line(&mut out, "CALSCALE:GREGORIAN");
    for item in items {
        validate(item)?;
        if item.all_day && item.start_at.zip(item.end_at).is_some_and(|(a, b)| a != b) {
            warnings.push(format!(
                "{}: all-day event with a longer span is shown as one day in v1",
                item.title
            ));
        }
        write_vevent(&mut out, item, dtstamp);
    }
    push_line(&mut out, "END:VCALENDAR");
    Ok(IcsDocument {
        text: out,
        warnings,
    })
}

fn validate(item: &CalendarItem) -> Result<(), IcsError> {
    if if item.all_day {
        item.all_day_date.is_none()
    } else if item.is_deadline {
        item.due_at.is_none()
    } else {
        item.start_at.is_none()
    } {
        return Err(IcsError::MissingStart(item.id));
    }
    if item
        .url
        .as_ref()
        .is_some_and(|s| s.chars().any(char::is_control))
        || item.alarm.as_ref().is_some_and(|s| !valid_alarm(s))
    {
        return Err(IcsError::InvalidValue(item.id));
    }
    Ok(())
}

// RFC duration grammar for a positive offset before a deadline.
fn valid_alarm(s: &str) -> bool {
    let Some(mut rest) = s.strip_prefix('P') else {
        return false;
    };
    let mut any = false;
    let mut nonzero = false;
    for unit in ['W', 'D', 'T', 'H', 'M', 'S'] {
        if unit == 'T' {
            if let Some(tail) = rest.strip_prefix('T') {
                rest = tail;
                if rest.is_empty() {
                    return false;
                }
            } else {
                return rest.is_empty() && any && nonzero;
            }
            continue;
        }
        let n = rest.bytes().take_while(u8::is_ascii_digit).count();
        if n > 0 && rest[n..].starts_with(unit) {
            nonzero |= rest[..n].bytes().any(|c| c != b'0');
            rest = &rest[n + 1..];
            any = true;
            if unit == 'W' {
                return rest.is_empty() && nonzero;
            }
        }
    }
    rest.is_empty() && any && nonzero
}

fn write_vevent(out: &mut String, item: &CalendarItem, dtstamp: Timestamp) {
    push_line(out, "BEGIN:VEVENT");
    let uid = format!("canvas-{}-{}@{}", item.kind, item.id, item.identity_key);
    push_prop(out, "UID", &escape_text(&uid));
    push_prop(out, "DTSTAMP", &format_utc(dtstamp));

    if item.all_day {
        let date = item
            .all_day_date
            .expect("validated civil date")
            .to_string()
            .replace('-', "");
        push_line(out, &format!("DTSTART;VALUE=DATE:{date}"));
        // No DTEND for all-day (one civil day).
    } else if item.is_deadline {
        if let Some(due) = item.due_at {
            push_prop(out, "DTSTART", &format_utc(due));
            // No DURATION / DTEND for point deadlines.
        }
    } else {
        if let Some(start) = item.start_at {
            push_prop(out, "DTSTART", &format_utc(start));
        }
        if let Some(end) = item.end_at {
            push_prop(out, "DTEND", &format_utc(end));
        }
    }

    let summary = match &item.course_code {
        Some(code) => format!("[{code}] {}", item.title),
        None => item.title.clone(),
    };
    push_prop(out, "SUMMARY", &escape_text(&summary));
    if let Some(url) = &item.url {
        push_prop(out, "URL", url);
    }
    if let Some(desc) = &item.description {
        push_prop(out, "DESCRIPTION", &escape_text(desc));
    }

    if item.is_deadline
        && let Some(alarm) = &item.alarm
    {
        push_line(out, "BEGIN:VALARM");
        push_prop(out, "ACTION", "DISPLAY");
        push_prop(out, "DESCRIPTION", "Reminder");
        push_prop(out, "TRIGGER", &format!("-{alarm}"));
        push_line(out, "END:VALARM");
    }

    push_line(out, "END:VEVENT");
}

fn format_utc(ts: Timestamp) -> String {
    ts.strftime("%Y%m%dT%H%M%SZ").to_string()
}

/// RFC 5545 §3.3.11 text escaping.
#[must_use]
pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.replace("\r\n", "\n").replace('\r', "\n").chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            ';' => out.push_str("\\;"),
            ',' => out.push_str("\\,"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            _ => out.push(c),
        }
    }
    out
}

fn push_prop(out: &mut String, name: &str, value: &str) {
    push_line(out, &format!("{name}:{value}"));
}

fn push_line(out: &mut String, line: &str) {
    for folded in fold_line(line) {
        out.push_str(&folded);
        out.push_str("\r\n");
    }
}

/// Fold to 75 octets with CRLF and a leading space on continuations.
#[must_use]
pub fn fold_line(line: &str) -> Vec<String> {
    let bytes = line.as_bytes();
    if bytes.len() <= 75 {
        return vec![line.to_string()];
    }
    let mut lines = Vec::new();
    let mut start = 0;
    let mut first = true;
    while start < bytes.len() {
        let budget = if first { 75 } else { 74 }; // leading space on continuation
        let mut end = (start + budget).min(bytes.len());
        while end > start && !line.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            // Pathological: single char longer than budget — advance one char.
            end = ((start + 1)..=bytes.len())
                .find(|&i| line.is_char_boundary(i))
                .unwrap_or(bytes.len());
        }
        if first {
            lines.push(line[start..end].to_string());
            first = false;
        } else {
            lines.push(format!(" {}", &line[start..end]));
        }
        start = end;
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    #[test]
    fn escaping_and_folding() {
        assert_eq!(escape_text("a;b,c\\d\ne"), "a\\;b\\,c\\\\d\\ne");
        let long = "X".repeat(100);
        let folded = fold_line(&format!("DESCRIPTION:{long}"));
        assert!(folded.len() > 1);
        assert!(folded[1].starts_with(' '));
        for line in &folded {
            assert!(line.len() <= 75);
        }
    }

    #[test]
    fn all_day_no_dtend_west_of_utc() {
        // Civil date west of UTC must not shift via Timestamp.
        let item = CalendarItem {
            kind: "event".into(),
            id: 1,
            identity_key: "host-1".into(),
            course_code: Some("CS".into()),
            title: "Holiday".into(),
            is_deadline: false,
            due_at: None,
            start_at: Some("2026-03-08T05:00:00Z".parse().unwrap()),
            end_at: Some("2026-03-08T05:00:00Z".parse().unwrap()),
            all_day: true,
            all_day_date: Some(date(2026, 3, 8)),
            url: None,
            description: None,
            alarm: None,
        };
        let ics = write_ics(&[item], "2026-01-01T00:00:00Z".parse().unwrap())
            .unwrap()
            .text;
        assert!(ics.contains("DTSTART;VALUE=DATE:20260308"));
        assert!(!ics.contains("DTEND"));
    }

    #[test]
    fn all_day_across_dst_equal_start_end() {
        let item = CalendarItem {
            kind: "event".into(),
            id: 2,
            identity_key: "host-1".into(),
            course_code: None,
            title: "DST".into(),
            is_deadline: false,
            due_at: None,
            // 23-hour span across spring DST — still one civil day.
            start_at: Some("2026-03-08T05:00:00Z".parse().unwrap()),
            end_at: Some("2026-03-09T04:00:00Z".parse().unwrap()),
            all_day: true,
            all_day_date: Some(date(2026, 3, 8)),
            url: None,
            description: Some("note; with, special\\chars".into()),
            alarm: None,
        };
        let ics = write_ics(&[item], "2026-01-01T00:00:00Z".parse().unwrap())
            .unwrap()
            .text;
        assert!(ics.contains("DTSTART;VALUE=DATE:20260308"));
        assert!(!ics.lines().any(|l| l.starts_with("DTEND")));
        assert!(ics.contains("DESCRIPTION:note\\; with\\, special\\\\chars"));
    }

    #[test]
    fn deadline_with_alarm() {
        let item = CalendarItem {
            kind: "assignment".into(),
            id: 9,
            identity_key: "host-1".into(),
            course_code: Some("CS".into()),
            title: "HW".into(),
            is_deadline: true,
            due_at: Some("2026-09-09T23:59:00Z".parse().unwrap()),
            start_at: None,
            end_at: None,
            all_day: false,
            all_day_date: None,
            url: Some("https://example.test/a".into()),
            description: None,
            alarm: Some("PT24H".into()),
        };
        let ics = write_ics(&[item], "2026-01-01T00:00:00Z".parse().unwrap())
            .unwrap()
            .text;
        assert!(ics.contains("DTSTAMP:20260101T000000Z\r\n"));
        assert!(ics.contains("DTSTART:20260909T235900Z\r\n"));
        assert!(ics.contains("BEGIN:VALARM"));
        assert!(ics.contains("TRIGGER:-PT24H"));
        assert!(ics.contains("SUMMARY:[CS] HW"));
    }
}

#[cfg(test)]
mod validation_tests {
    use super::*;

    #[test]
    fn unicode_folding_roundtrips_and_alarm_grammar() {
        let text = format!("DESCRIPTION:{}", "😀é".repeat(60));
        let lines = fold_line(&text);
        assert!(lines.iter().all(|s| s.len() <= 75));
        assert_eq!(lines.join("\r\n").replace("\r\n ", ""), text);
        for good in ["PT24H", "P1D", "P2W", "P1DT30M", "PT1H30M"] {
            assert!(valid_alarm(good));
        }
        for bad in [
            "",
            "PT",
            "PT0S",
            "-PT1H",
            "PT1H\r\nEND:VEVENT",
            "P1W2D",
            "P1H",
        ] {
            assert!(!valid_alarm(bad));
        }
        assert_eq!(escape_text("a\rb\r\nc"), "a\\nb\\nc");
    }
}
