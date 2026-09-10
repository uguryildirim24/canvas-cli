//! Dataset refresh and sync (SPEC §10).

mod course;
mod course_totals;
mod courses;
mod enrollment_grades;
mod fields;
mod grading_periods;
mod outcome;
mod refresh;
mod terms;
mod wire;

#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod tests;

pub use course::{CourseDetailDataset, refresh_course};
pub use course_totals::{
    CourseTotalMode, CourseTotalsDataset, default_ttl_grades, totals_from_enrollment,
    upsert_course_totals, upsert_course_totals_entity,
};
pub use courses::{
    CoursesDataset, CoursesScope, course_to_entity, courses_fetch_paths, courses_path,
    courses_to_ingest_page, default_ttl_courses,
};
pub use enrollment_grades::{
    EnrollmentGradesDataset, PeriodKey, enrollment_grades_path, enrollment_to_entity,
    enrollments_to_ingest_page,
};
pub use grading_periods::{
    GradingPeriodsDataset, grading_period_to_entity, grading_periods_path,
    grading_periods_to_ingest_page,
};
pub use outcome::{FreshnessInfo, FreshnessSource, RefreshOutcome, SyncError};
pub use refresh::{refresh_courses, refresh_enrollment_grades, refresh_grading_periods};
pub use terms::{TermsDataset, default_ttl_terms, term_to_entity, upsert_term};
