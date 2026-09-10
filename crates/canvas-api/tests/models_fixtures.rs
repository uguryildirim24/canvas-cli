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
