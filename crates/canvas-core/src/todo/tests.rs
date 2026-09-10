//! Todo merge unit tests (§16).

use std::collections::BTreeMap;

use jiff::{Timestamp, civil::Date};

use super::buckets::{AssignmentBucket, in_bucket};
use super::merge::{
    MissingSourceRow, PlannerSourceRow, TodoFilters, TodoKind, TodoWindow, build_todo,
    map_plannable_kind,
};

fn ts(s: &str) -> Timestamp {
    s.parse().unwrap()
}

#[test]
fn kind_mapping_covers_spec_aliases() {
    assert_eq!(map_plannable_kind("assignment").0, TodoKind::Assignment);
    assert_eq!(map_plannable_kind("quiz").0, TodoKind::Quiz);
    assert_eq!(
        map_plannable_kind("discussion_topic").0,
        TodoKind::Discussion
    );
    assert_eq!(map_plannable_kind("sub_assignment").0, TodoKind::Checkpoint);
    assert_eq!(
        map_plannable_kind("peer_review_sub_assignment").0,
        TodoKind::PeerReview
    );
    assert_eq!(
        map_plannable_kind("assessment_request").0,
        TodoKind::PeerReview
    );
    assert_eq!(map_plannable_kind("weird_type").0, TodoKind::Unknown);
}

#[test]
fn missing_wins_and_dismissed_missing_stays_visible() {
    let planner = vec![PlannerSourceRow {
        id: "assignment:1".into(),
        plannable_id: Some(1),
        plannable_type: Some("assignment".into()),
        course_id: Some(10),
        title: Some("HW1".into()),
        data_json: r#"{"due_at":"2026-09-01T12:00:00Z","dismissed":true,"marked_complete":false}"#
            .into(),
    }];
    let missing = vec![MissingSourceRow {
        id: 1,
        course_id: Some(10),
        name: Some("HW1".into()),
        due_at: Some("2026-09-01T12:00:00Z".into()),
        points_possible: Some(10.0),
        html_url: None,
        submitted: Some(0),
        graded: Some(0),
        score: None,
        late: Some(0),
        missing: Some(1),
        excused: Some(0),
        can_submit: None,
        unlock_at: None,
        lock_at: None,
        data_json: "{}".into(),
    }];
    let mut codes = BTreeMap::new();
    codes.insert(10, "CHEM".into());
    let (items, counts) = build_todo(
        &planner,
        &missing,
        &codes,
        &BTreeMap::new(),
        ts("2026-09-09T12:00:00Z"),
        Date::new(2026, 9, 9).unwrap(),
        &TodoWindow {
            start: Date::new(2026, 9, 8).unwrap(),
            end: Date::new(2026, 9, 22).unwrap(),
            days: 14,
        },
        &TodoFilters::default(),
        jiff::Span::new().minutes(30),
        &BTreeMap::new(),
    );
    assert_eq!(items.len(), 1);
    assert!(items[0].status.missing);
    assert!(items[0].dismissed);
    assert_eq!(counts.missing, 1);
}

#[test]
fn open_bucket_is_union_of_upcoming_overdue_undated() {
    let overdue = super::merge::TodoItem {
        key: "a".into(),
        kind: TodoKind::Assignment,
        raw_type: "assignment".into(),
        id: 1,
        assignment_id: Some(1),
        parent_assignment_id: None,
        course_id: None,
        course_code: None,
        title: "o".into(),
        due_at: Some(ts("2026-09-01T00:00:00Z")),
        scheduled_at: None,
        points_possible: None,
        status: super::merge::TodoStatus::default(),
        availability: super::merge::TodoAvailability::default(),
        marked_complete: false,
        dismissed: false,
        html_url: None,
    };
    let now = ts("2026-09-09T00:00:00Z");
    assert!(in_bucket(&overdue, AssignmentBucket::Overdue, now));
    assert!(in_bucket(&overdue, AssignmentBucket::Open, now));
    assert!(!in_bucket(&overdue, AssignmentBucket::Upcoming, now));
}

#[test]
fn unknown_kind_is_retained() {
    let planner = vec![PlannerSourceRow {
        id: "custom:9".into(),
        plannable_id: Some(9),
        plannable_type: Some("custom_widget".into()),
        course_id: None,
        title: Some("Widget".into()),
        data_json: "{}".into(),
    }];
    let (items, _) = build_todo(
        &planner,
        &[],
        &BTreeMap::new(),
        &BTreeMap::new(),
        ts("2026-09-09T12:00:00Z"),
        Date::new(2026, 9, 9).unwrap(),
        &TodoWindow {
            start: Date::new(2026, 9, 8).unwrap(),
            end: Date::new(2026, 9, 22).unwrap(),
            days: 14,
        },
        &TodoFilters {
            all: true,
            ..TodoFilters::default()
        },
        jiff::Span::new().minutes(30),
        &BTreeMap::new(),
    );
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].kind, TodoKind::Unknown);
    assert_eq!(items[0].raw_type, "custom_widget");
}
