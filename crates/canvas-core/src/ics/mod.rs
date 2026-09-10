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

/// Write a `VCALENDAR` document for `items`. `dtstamp` is the generation time.
#[must_use]
pub fn write_ics(items: &[CalendarItem], dtstamp: Timestamp) -> String {
    let mut out = String::new();
    push_line(&mut out, "BEGIN:VCALENDAR");
    push_line(&mut out, "VERSION:2.0");
    push_line(&mut out, "PRODID:-//canvas-cli//EN");
    push_line(&mut out, "CALSCALE:GREGORIAN");
    for item in items {
        write_vevent(&mut out, item, dtstamp);
    }
    push_line(&mut out, "END:VCALENDAR");
    out
}

fn write_vevent(out: &mut String, item: &CalendarItem, dtstamp: Timestamp) {
    push_line(out, "BEGIN:VEVENT");
    let uid = format!("canvas-{}-{}@{}", item.kind, item.id, item.identity_key);
    push_prop(out, "UID", &uid);
    push_prop(out, "DTSTAMP", &format_utc(dtstamp));

    if item.all_day {
        let date = item
            .all_day_date
            .map_or_else(|| "19700101".into(), |d| d.to_string().replace('-', ""));
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
    // YYYYMMDDTHHMMSSZ
    let s = ts.to_string(); // RFC 3339
    let cleaned: String = s.chars().filter(char::is_ascii_digit).collect();
    // timestamp string like 2026-09-09T12:00:00Z → digits 20260909120000
    if cleaned.len() >= 14 {
        format!("{}Z", &cleaned[..14])
    } else {
        format!("{cleaned}Z")
    }
}

/// RFC 5545 §3.3.11 text escaping.
#[must_use]
pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
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
        let ics = write_ics(&[item], "2026-01-01T00:00:00Z".parse().unwrap());
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
        let ics = write_ics(&[item], "2026-01-01T00:00:00Z".parse().unwrap());
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
        let ics = write_ics(&[item], "2026-01-01T00:00:00Z".parse().unwrap());
        assert!(ics.contains("BEGIN:VALARM"));
        assert!(ics.contains("TRIGGER:-PT24H"));
        assert!(ics.contains("SUMMARY:[CS] HW"));
    }
}
