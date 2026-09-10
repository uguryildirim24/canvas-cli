//! Dataset refresh and sync (SPEC §10).

mod assignment_detail;
mod assignments;
mod course;
mod course_totals;
mod courses;
mod discovery;
mod enrollment_grades;
mod fields;
mod files;
mod folders;
mod grading_periods;
mod missing;
mod modules;
mod outcome;
mod planner;
mod refresh;
mod submission;
mod terms;
mod wire;

#[cfg(test)]
mod files_modules_tests;
#[cfg(test)]
mod review_tests;
#[cfg(test)]
mod tests;

pub use assignments::{
    AssignmentsDataset, assignment_detail_path, assignment_to_entity, assignments_path,
    assignments_to_ingest_page, default_ttl_assignments, upsert_assignment,
};
pub use course::{CourseDetailDataset, refresh_course};
pub use course_totals::{
    CourseTotalMode, CourseTotalsDataset, default_ttl_grades, totals_from_enrollment,
    upsert_course_totals, upsert_course_totals_entity,
};
pub use courses::{
    CoursesDataset, CoursesScope, course_to_entity, courses_fetch_paths, courses_path,
    courses_to_ingest_page, default_ttl_courses,
};
pub use discovery::{discovery_plan_input, is_canvas_root};
pub use enrollment_grades::{
    EnrollmentGradesDataset, PeriodKey, enrollment_grades_path, enrollment_to_entity,
    enrollments_to_ingest_page,
};
pub use files::{
    FilesDataset, default_ttl_files, file_to_entity, files_path, files_to_ingest_page,
};
pub use folders::{FoldersDataset, folder_to_entity, folders_path, folders_to_ingest_page};
pub use grading_periods::{
    GradingPeriodsDataset, grading_period_to_entity, grading_periods_path,
    grading_periods_to_ingest_page,
};
pub use missing::{MissingDataset, default_ttl_missing, missing_path, missing_to_ingest_page};
pub use modules::{
    ModulesDataset, count_item_fetch_requests, default_ttl_modules, module_item_to_entity,
    module_items_path, module_to_entity, modules_path, modules_to_ingest_page, needs_items_fetch,
};
pub use outcome::{FreshnessInfo, FreshnessSource, RefreshOutcome, SyncError};
pub use planner::{
    PlannerDataset, PlannerWindow, default_ttl_planner, planner_item_to_entity, planner_path,
    planner_to_ingest_page,
};
pub use refresh::{
    DenialAction, classify_listing_denial, listing_denial_status, refresh_assignments,
    refresh_courses, refresh_enrollment_grades, refresh_files, refresh_folders,
    refresh_grading_periods, refresh_missing, refresh_modules, refresh_planner, refresh_submission,
};
pub use submission::{
    SubmissionDataset, submission_path, submission_to_entity, submission_to_ingest_page,
};
pub use terms::{TermsDataset, default_ttl_terms, term_to_entity, upsert_term};

pub use assignment_detail::{AssignmentDetailDataset, refresh_assignment};

#[cfg(test)]
mod m1c_review_tests;
