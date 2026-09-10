//! Human renderer base: tables, dates, status colors (§7).

use anstyle::{AnsiColor, Color, Style};
use comfy_table::{ContentArrangement, Table, presets};
use jiff::{Timestamp, tz::TimeZone};

use crate::output::now::now_timestamp;

/// Status label kinds used by human renderers (§7 / §12.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    Missing,
    Overdue,
    DueSoon,
    Submitted,
    Locked,
    Closed,
    External,
    Pending,
    Unknown,
    LateGraded,
}

/// Build a borderless `comfy-table` with dynamic width and two-space padding.
#[must_use]
pub fn new_table() -> Table {
    let mut table = Table::new();
    table.load_style(presets::NOTHING);
    table.set_content_arrangement(ContentArrangement::Dynamic);
    if let Ok(raw) = std::env::var("COLUMNS")
        && let Ok(width) = raw.parse::<u16>()
        && width > 0
    {
        table.set_width(width);
    }
    table
}

/// Apply the SPEC two-space cell padding to every column.
pub fn apply_two_space_padding(table: &mut Table) {
    for column in table.column_iter_mut() {
        column.set_padding((2, 2));
    }
}

/// Format a timestamp in `zone` with a relative suffix.
///
/// Example: `Tue Sep 15, 11:59 PM (in 2d 4h)`.
#[must_use]
pub fn format_local_datetime(ts: Timestamp, zone: &TimeZone) -> String {
    format_local_datetime_at(ts, zone, now_timestamp())
}

/// Same as [`format_local_datetime`] with an explicit "now" (tests).
#[must_use]
pub fn format_local_datetime_at(ts: Timestamp, zone: &TimeZone, now: Timestamp) -> String {
    let absolute = format_local_instant(ts, zone);
    let relative = format_relative_suffix_at(ts, now);
    format!("{absolute} {relative}")
}

/// The §7 date without a relative suffix: `Tue Sep 15, 11:59 PM`.
///
/// The suffix reads a date as a deadline (`in 2d 4h` / `overdue 3h`), so it
/// belongs on due dates. Use this for a time something happened at.
#[must_use]
pub fn format_local_instant(ts: Timestamp, zone: &TimeZone) -> String {
    ts.to_zoned(zone.clone())
        .strftime("%a %b %-d, %-I:%M %p")
        .to_string()
        .replace("  ", " ")
}

/// Relative suffix only: `(in 2d 4h)` / `(overdue 3h)`.
#[must_use]
pub fn format_relative_suffix(ts: Timestamp) -> String {
    format_relative_suffix_at(ts, now_timestamp())
}

/// Relative suffix with an explicit "now".
#[must_use]
pub fn format_relative_suffix_at(ts: Timestamp, now: Timestamp) -> String {
    let delta = ts.as_second() - now.as_second();
    let overdue = delta < 0;
    let abs = delta.unsigned_abs();
    let days = abs / 86_400;
    let hours = (abs % 86_400) / 3_600;
    let mins = (abs % 3_600) / 60;
    let body = if days > 0 {
        if hours > 0 {
            format!("{days}d {hours}h")
        } else {
            format!("{days}d")
        }
    } else if hours > 0 {
        format!("{hours}h")
    } else if mins > 0 {
        format!("{mins}m")
    } else {
        "0m".to_string()
    };
    if overdue {
        format!("(overdue {body})")
    } else {
        format!("(in {body})")
    }
}

/// Map status fields to a short human label (§7).
#[must_use]
pub fn status_label(kind: StatusKind, detail: Option<&str>) -> String {
    match kind {
        StatusKind::Missing => "missing".to_string(),
        StatusKind::Overdue => "overdue".to_string(),
        StatusKind::DueSoon => "due soon".to_string(),
        StatusKind::Submitted => "submitted".to_string(),
        StatusKind::Locked => "locked".to_string(),
        StatusKind::Closed => "closed".to_string(),
        StatusKind::External => "external".to_string(),
        StatusKind::Pending => "pending".to_string(),
        StatusKind::Unknown => "unknown".to_string(),
        StatusKind::LateGraded => match detail {
            Some(d) => format!("late · graded {d}"),
            None => "late".to_string(),
        },
    }
}

/// Style for overdue items (red).
#[must_use]
pub fn style_overdue() -> Style {
    Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red)))
}

/// Style for missing items (red).
#[must_use]
pub fn style_missing() -> Style {
    Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red)))
}

/// Style for due-in-under-24h items (yellow).
#[must_use]
pub fn style_due_soon() -> Style {
    Style::new().fg_color(Some(Color::Ansi(AnsiColor::Yellow)))
}

/// Style for submitted items (green).
#[must_use]
pub fn style_submitted() -> Style {
    Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)))
}

/// Style for locked and closed items (dim).
#[must_use]
pub fn style_dim() -> Style {
    Style::new().dimmed()
}

/// Pick the §7 color for a status kind.
#[must_use]
pub fn apply_status_style(kind: StatusKind) -> Style {
    match kind {
        StatusKind::Missing | StatusKind::Overdue => style_overdue(),
        StatusKind::DueSoon => style_due_soon(),
        StatusKind::Submitted | StatusKind::LateGraded => style_submitted(),
        StatusKind::Locked | StatusKind::Closed => style_dim(),
        StatusKind::External | StatusKind::Pending | StatusKind::Unknown => Style::new(),
    }
}

/// Paint `text` with `style` when `use_color` is true.
#[must_use]
pub fn paint(text: &str, style: Style, use_color: bool) -> String {
    if use_color {
        format!("{style}{text}{style:#}")
    } else {
        text.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_suffix_future_and_overdue() {
        let now = "2026-09-09T12:00:00Z".parse::<Timestamp>().unwrap();
        let future = "2026-09-11T16:00:00Z".parse::<Timestamp>().unwrap();
        let past = "2026-09-09T09:00:00Z".parse::<Timestamp>().unwrap();
        assert_eq!(format_relative_suffix_at(future, now), "(in 2d 4h)");
        assert_eq!(format_relative_suffix_at(past, now), "(overdue 3h)");
    }

    #[test]
    fn local_datetime_includes_relative() {
        let now = "2026-09-09T12:00:00Z".parse::<Timestamp>().unwrap();
        let ts = "2026-09-09T15:00:00Z".parse::<Timestamp>().unwrap();
        let formatted = format_local_datetime_at(ts, &TimeZone::UTC, now);
        assert!(formatted.contains("(in 3h)"), "{formatted}");
        assert!(
            formatted.contains("PM") || formatted.contains("AM"),
            "{formatted}"
        );
    }

    #[test]
    fn table_is_borderless_with_padding() {
        let mut table = new_table();
        table.set_header(vec!["A", "B"]);
        table.add_row(vec!["1", "2"]);
        apply_two_space_padding(&mut table);
        let rendered = table.to_string();
        assert!(!rendered.contains('│'));
        assert!(!rendered.contains('─'));
    }

    #[test]
    fn status_styles_and_labels() {
        assert_eq!(status_label(StatusKind::Missing, None), "missing");
        assert_eq!(
            status_label(StatusKind::LateGraded, Some("45/50")),
            "late · graded 45/50"
        );
        let painted = paint("missing", style_missing(), true);
        assert!(painted.contains("missing"));
        assert_ne!(painted, "missing");
        assert_eq!(paint("missing", style_missing(), false), "missing");
    }
}
