//! Journal state enums.

use std::fmt;
use std::str::FromStr;

/// Journal lifecycle state (§12.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum State {
    /// Row published; uploads not started.
    Planned,
    /// Uploads in progress.
    Uploading,
    /// All file IDs recorded; POST not sent.
    Uploaded,
    /// Submission POST in flight or unclassified.
    Posting,
    /// Observed successful POST.
    Submitted,
    /// Matched via history (unproven attribution).
    Matched,
    /// Upload failed.
    UploadIncomplete,
    /// Files ready but POST never sent / assumed.
    UploadedNotSubmitted,
    /// POST outcome not observed.
    OutcomeUnknown,
    /// Refused after the row existed.
    Refused,
}

impl State {
    /// Snake-case DB / JSON form.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::Uploading => "uploading",
            Self::Uploaded => "uploaded",
            Self::Posting => "posting",
            Self::Submitted => "submitted",
            Self::Matched => "matched",
            Self::UploadIncomplete => "upload_incomplete",
            Self::UploadedNotSubmitted => "uploaded_not_submitted",
            Self::OutcomeUnknown => "outcome_unknown",
            Self::Refused => "refused",
        }
    }

    /// Terminal states never report a live owner.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Submitted
                | Self::Matched
                | Self::UploadIncomplete
                | Self::UploadedNotSubmitted
                | Self::OutcomeUnknown
                | Self::Refused
        )
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for State {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "planned" => Self::Planned,
            "uploading" => Self::Uploading,
            "uploaded" => Self::Uploaded,
            "posting" => Self::Posting,
            "submitted" => Self::Submitted,
            "matched" => Self::Matched,
            "upload_incomplete" => Self::UploadIncomplete,
            "uploaded_not_submitted" => Self::UploadedNotSubmitted,
            "outcome_unknown" => Self::OutcomeUnknown,
            "refused" => Self::Refused,
            _ => return Err(()),
        })
    }
}

/// `response_kind` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponseKind {
    /// Canvas-shaped error body.
    CanvasError,
    /// Other HTTP response.
    Other,
    /// Timeout / dropped connection.
    None,
}

impl ResponseKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::CanvasError => "canvas-error",
            Self::Other => "other",
            Self::None => "none",
        }
    }
}

impl FromStr for ResponseKind {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "canvas-error" => Self::CanvasError,
            "other" => Self::Other,
            "none" => Self::None,
            _ => return Err(()),
        })
    }
}

/// `not_submitted_evidence` column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NotSubmittedEvidence {
    /// Recovered from `uploaded` with owner absent.
    NeverSent,
    /// User assumed after 30 minutes.
    Assumed,
}

impl NotSubmittedEvidence {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NeverSent => "never_sent",
            Self::Assumed => "assumed",
        }
    }
}

impl FromStr for NotSubmittedEvidence {
    type Err = ();
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(match s {
            "never_sent" => Self::NeverSent,
            "assumed" => Self::Assumed,
            _ => return Err(()),
        })
    }
}

/// Owner probe result for JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnerStatus {
    /// Non-blocking exclusive lock would block.
    Live,
    /// Lock acquired (owner gone) or immediately free.
    Absent,
    /// Terminal journal — probe not meaningful.
    NotApplicable,
}

impl OwnerStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Absent => "absent",
            Self::NotApplicable => "n/a",
        }
    }
}
