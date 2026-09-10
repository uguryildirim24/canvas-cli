//! Shared assignment / todo buckets (§12.1).

use jiff::Timestamp;

use super::merge::TodoItem;

/// Local bucket filter shared by `todo` and `assignments`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssignmentBucket {
    Open,
    Upcoming,
    Overdue,
    Past,
    Undated,
    Unsubmitted,
    Ungraded,
    Future,
    All,
}

impl AssignmentBucket {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Upcoming => "upcoming",
            Self::Overdue => "overdue",
            Self::Past => "past",
            Self::Undated => "undated",
            Self::Unsubmitted => "unsubmitted",
            Self::Ungraded => "ungraded",
            Self::Future => "future",
            Self::All => "all",
        }
    }

    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Some(match raw {
            "open" => Self::Open,
            "upcoming" => Self::Upcoming,
            "overdue" => Self::Overdue,
            "past" => Self::Past,
            "undated" => Self::Undated,
            "unsubmitted" => Self::Unsubmitted,
            "ungraded" => Self::Ungraded,
            "future" => Self::Future,
            "all" => Self::All,
            _ => return None,
        })
    }
}

/// Whether an item belongs in `bucket` at `now`.
#[must_use]
pub fn in_bucket(item: &TodoItem, bucket: AssignmentBucket, now: Timestamp) -> bool {
    let submitted = item.status.submitted.unwrap_or(false);
    let excused = item.status.excused.unwrap_or(false);
    let graded = item.status.graded.unwrap_or(false);
    match bucket {
        AssignmentBucket::All => true,
        AssignmentBucket::Undated => item.due_at.is_none(),
        AssignmentBucket::Upcoming => item.due_at.is_some_and(|d| d >= now),
        AssignmentBucket::Overdue => item.due_at.is_some_and(|d| d < now) && !submitted && !excused,
        AssignmentBucket::Past => item.due_at.is_some_and(|d| d < now),
        AssignmentBucket::Unsubmitted => !submitted && item.availability.submittable != Some(false),
        AssignmentBucket::Ungraded => submitted && !graded,
        AssignmentBucket::Future => item.availability.unlock_at.is_some_and(|u| u > now),
        AssignmentBucket::Open => {
            in_bucket(item, AssignmentBucket::Upcoming, now)
                || in_bucket(item, AssignmentBucket::Overdue, now)
                || in_bucket(item, AssignmentBucket::Undated, now)
        }
    }
}
