//! Fixture deserialization for every v1 model.

use canvas_api::Supplied;
use canvas_api::models::{
    Announcement, Assignment, AssignmentGroup, CalendarEvent, Course, Enrollment, File, Folder,
    GradingPeriod, MissingSubmission, Module, ModuleItem, PlannerItem, Submission, Term, User,
    WrappedCollection,
};
use canvas_api::serde_util::with_origin;
use reqwest::Url;

fn origin() -> Url {
    Url::parse("https://canvas.example.test").unwrap()
}

fn load<T: serde::de::DeserializeOwned>(json: &str) -> T {
    with_origin(&origin(), || {
        serde_json::from_str(json).expect("deserialize")
    })
}

#[test]
fn fixtures_deserialize() {
    let _: User = load(include_str!("fixtures/user.json"));
    let _: Course = load(include_str!("fixtures/course.json"));
    let _: Term = load(include_str!("fixtures/term.json"));
    let _: Enrollment = load(include_str!("fixtures/enrollment.json"));
    let _: GradingPeriod = load(include_str!("fixtures/grading_period.json"));
    let _: AssignmentGroup = load(include_str!("fixtures/assignment_group.json"));
    let _: Assignment = load(include_str!("fixtures/assignment.json"));
    let _: Submission = load(include_str!("fixtures/submission.json"));
    let _: PlannerItem = load(include_str!("fixtures/planner.json"));
    let _: MissingSubmission = load(include_str!("fixtures/missing.json"));
    let _: Folder = load(include_str!("fixtures/folder.json"));
    let _: File = load(include_str!("fixtures/file.json"));
    let _: Module = load(include_str!("fixtures/module.json"));
    let _: ModuleItem = load(include_str!("fixtures/module_item.json"));
    let _: Announcement = load(include_str!("fixtures/announcement.json"));
    let _: CalendarEvent = load(include_str!("fixtures/calendar.json"));
}

#[test]
fn grading_periods_wrapped_pages() {
    let page1: WrappedCollection<GradingPeriod> =
        load(include_str!("fixtures/grading_periods_page1.json"));
    assert_eq!(page1.items.len(), 2);
    assert_eq!(page1.items[0].id, 7);

    let page2: WrappedCollection<GradingPeriod> =
        load(include_str!("fixtures/grading_periods_page2.json"));
    assert_eq!(page2.items.len(), 2);
    assert_eq!(page2.items[0].id, 9);
}

#[test]
fn supplied_can_submit_states() {
    let absent: Assignment = load(include_str!("fixtures/supplied_absent.json"));
    assert_eq!(absent.can_submit, Supplied::Absent);

    let null: Assignment = load(include_str!("fixtures/supplied_null.json"));
    assert_eq!(null.can_submit, Supplied::Null);

    let value: Assignment = load(include_str!("fixtures/supplied_value.json"));
    assert_eq!(value.can_submit, Supplied::Value(false));
}

#[test]
fn tracked_core_detail_status_and_scoped_grades_keep_all_three_states() {
    let absent: Assignment = load(include_str!("fixtures/tracked_absent.json"));
    let null: Assignment = load(include_str!("fixtures/tracked_null.json"));
    let value: Assignment = load(include_str!("fixtures/tracked_value.json"));
    macro_rules! states {
        ($a:expr, $n:expr, $v:expr; $($field:ident),+ $(,)?) => { $(
            assert!(matches!($a.$field, Supplied::Absent), stringify!($field));
            assert!(matches!($n.$field, Supplied::Null), stringify!($field));
            assert!(matches!($v.$field, Supplied::Value(_)), stringify!($field));
        )+ };
    }
    states!(absent, null, value; name, description, due_at, unlock_at, lock_at,
        points_possible, html_url, submission_types, allowed_extensions, allowed_attempts, rubric, can_submit);
    let a = absent.submission.unwrap();
    let n = null.submission.unwrap();
    let v = value.submission.unwrap();
    states!(a, n, v; attempt, submitted_at, graded_at, score, grade, late, missing, excused, workflow_state);
    assert_eq!(
        v.submitted_at.as_value().unwrap().to_string(),
        "2026-09-19T09:00:00Z"
    );
    assert_eq!(
        value.due_at.as_value().unwrap().to_string(),
        "2026-09-20T04:00:00Z"
    );
    assert_eq!(
        value.html_url.as_value().unwrap().as_str(),
        "https://canvas.example.test/courses/100/assignments/9"
    );
    let a = absent.course.unwrap().enrollments.unwrap();
    let n = null.course.unwrap().enrollments.unwrap();
    let v = value.course.unwrap().enrollments.unwrap();
    states!(a[0], n[0], v[0]; computed_current_score, current_period_computed_final_grade);
}

#[test]
fn nested_ids_timestamps_urls_and_module_fallback_are_normalized() {
    use canvas_api::models::{CourseEnrollment, ExternalToolTagAttributes};
    let group: AssignmentGroup = load(
        r#"{"id":"3","rules":{"never_drop":["9",10]},"assignments":[{"id":"9","due_at":"2026-09-20T00:00:00-04:00"}]}"#,
    );
    assert_eq!(group.rules.unwrap().never_drop.unwrap(), vec![9, 10]);
    assert_eq!(group.assignments.unwrap()[0].id, 9);
    let enrollment: CourseEnrollment = load(r#"{"current_grading_period_id":"7"}"#);
    assert_eq!(enrollment.current_grading_period_id, Some(7));
    let tool: ExternalToolTagAttributes = load(r#"{"content_id":"8","url":"/tools/8"}"#);
    assert_eq!(tool.content_id, Some(8));
    assert_eq!(
        tool.url.unwrap().as_str(),
        "https://canvas.example.test/tools/8"
    );
    let module: Module = load(
        r#"{"id":"30","prerequisite_module_ids":["28",29],"completed_at":"2026-09-20T00:00:00-04:00","items_url":"/modules/30/items"}"#,
    );
    assert_eq!(module.prerequisite_module_ids.unwrap(), vec![28, 29]);
    assert_eq!(
        module.completed_at.unwrap().to_string(),
        "2026-09-20T04:00:00Z"
    );
    assert_eq!(
        module.items_url.unwrap().as_str(),
        "https://canvas.example.test/modules/30/items"
    );
    assert!(module.items.is_none());
    let module: Module = load(r#"{"id":30,"items":null}"#);
    assert!(module.items.is_none());
    let item: ModuleItem = load(
        r#"{"id":"31","content_details":{"due_at":"2026-09-20T00:00:00-04:00"},"url":"/modules/30/items/31"}"#,
    );
    assert_eq!(
        item.content_details
            .unwrap()
            .due_at
            .as_value()
            .unwrap()
            .to_string(),
        "2026-09-20T04:00:00Z"
    );
    assert_eq!(
        item.url.unwrap().as_str(),
        "https://canvas.example.test/modules/30/items/31"
    );
    let calendar: CalendarEvent = load(r#"{"id":50,"all_day":true,"all_day_date":"2026-09-20"}"#);
    assert_eq!(calendar.all_day_date.unwrap().to_string(), "2026-09-20");
}

#[test]
fn origin_context_restores_on_nested_panic_without_leaking_url_in_errors() {
    use canvas_api::serde_util::deserialize_url;
    #[derive(serde::Deserialize)]
    struct Relative(#[serde(deserialize_with = "deserialize_url")] Url);
    with_origin(&origin(), || {
        let _ = std::panic::catch_unwind(|| {
            with_origin(&Url::parse("https://other.test").unwrap(), || {
                panic!("test unwind")
            });
        });
        let value: Relative = serde_json::from_str(r#""/restored""#).unwrap();
        assert_eq!(value.0.host_str(), Some("canvas.example.test"));
    });
    let err = serde_json::from_str::<Relative>(r#""/relative?token=SECRET""#)
        .err()
        .unwrap();
    assert!(!err.to_string().contains("SECRET"));
}
