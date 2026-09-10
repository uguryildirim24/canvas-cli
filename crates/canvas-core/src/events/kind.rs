//! Event kinds (agent-UX REPORT §3.6).

/// One `canvas-cli/event@1` kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventKind {
    /// An entity joined a complete membership.
    AssignmentAdded,
    /// An allowlisted assignment field changed.
    AssignmentChanged,
    /// An entity left a complete membership.
    AssignmentRemoved,
    /// `due_at` changed.
    DueChanged,
    /// `score` or `grade` changed without publication evidence.
    GradeChanged,
    /// `posted_at` became non-null: the grade was published.
    GradePosted,
    /// An announcement joined a complete window membership.
    AnnouncementNew,
    /// An assignment joined the missing-submissions membership.
    MissingNew,
    /// A submission journal changed state (§12.2).
    SubmissionState,
    /// An operation journal changed state (M8-b, §12.2 discipline).
    OperationState,
    /// Registered for M8-a; this package writes no producer for it.
    InboxUnreadCount,
    /// The recorded observation could not be applied; rebuild the baseline.
    ResyncRequired,
}

impl EventKind {
    /// The wire name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AssignmentAdded => "assignment.added",
            Self::AssignmentChanged => "assignment.changed",
            Self::AssignmentRemoved => "assignment.removed",
            Self::DueChanged => "due.changed",
            Self::GradeChanged => "grade.changed",
            Self::GradePosted => "grade.posted",
            Self::AnnouncementNew => "announcement.new",
            Self::MissingNew => "missing.new",
            Self::SubmissionState => "submission.state",
            Self::OperationState => "operation.state",
            Self::InboxUnreadCount => "inbox.unread_count",
            Self::ResyncRequired => "resync_required",
        }
    }

    /// The kind a stored `events.kind` value names.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Self::all().iter().copied().find(|k| k.as_str() == raw)
    }

    /// Every registered kind, in a stable order.
    #[must_use]
    pub fn all() -> &'static [Self] {
        &[
            Self::AssignmentAdded,
            Self::AssignmentChanged,
            Self::AssignmentRemoved,
            Self::DueChanged,
            Self::GradeChanged,
            Self::GradePosted,
            Self::AnnouncementNew,
            Self::MissingNew,
            Self::SubmissionState,
            Self::OperationState,
            Self::InboxUnreadCount,
            Self::ResyncRequired,
        ]
    }

    /// The notification group this kind belongs to (`canvas notify`).
    #[must_use]
    pub fn group(self) -> &'static str {
        match self {
            Self::AssignmentAdded
            | Self::AssignmentChanged
            | Self::AssignmentRemoved
            | Self::DueChanged => "assignments",
            Self::GradeChanged | Self::GradePosted => "grades",
            Self::AnnouncementNew => "announcements",
            Self::MissingNew => "missing",
            Self::SubmissionState => "submission",
            Self::OperationState => "operation",
            Self::InboxUnreadCount => "inbox",
            Self::ResyncRequired => "resync",
        }
    }
}

impl std::fmt::Display for EventKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
