//! Canvas API response models.

mod announcement;
mod assignment;
mod assignment_group;
mod calendar;
mod course;
mod enrollment;
mod file;
mod folder;
mod grading_period;
mod module;
mod page;
mod planner;
mod submission;
mod term;
mod user;

pub use announcement::Announcement;
pub use assignment::{Assignment, ExternalToolTagAttributes, MissingSubmission};
pub use assignment_group::{AssignmentGroup, AssignmentGroupRules};
pub use calendar::CalendarEvent;
pub use course::{Course, CourseEnrollment};
pub use enrollment::{Enrollment, EnrollmentGrades};
pub use file::File;
pub use folder::Folder;
pub use grading_period::{GradingPeriod, WrappedCollection};
pub use module::{Module, ModuleItem, ModuleItemContentDetails};
pub use page::Page;
pub use planner::{Plannable, PlannerItem, PlannerOverride};
pub use submission::{
    RubricAssessment, Submission, SubmissionAttachment, SubmissionComment, SubmissionHistoryEntry,
};
pub use term::Term;
pub use user::User;
