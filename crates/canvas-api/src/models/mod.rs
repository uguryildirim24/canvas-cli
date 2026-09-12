//! Canvas API response models.

mod announcement;
mod assignment;
mod assignment_group;
mod calendar;
mod conversation;
mod course;
mod discussion;
mod enrollment;
mod file;
mod folder;
mod grading_period;
mod module;
mod new_quiz;
mod page;
mod planner;
mod quiz;
mod submission;
mod term;
mod user;
mod wiki_page;

pub use announcement::Announcement;
pub use assignment::{Assignment, ExternalToolTagAttributes, MissingSubmission};
pub use assignment_group::{AssignmentGroup, AssignmentGroupRules};
pub use calendar::CalendarEvent;
pub use conversation::{
    Conversation, ConversationAttachment, ConversationMessage, ConversationParticipant, UnreadCount,
};
pub use course::{Course, CourseEnrollment};
pub use discussion::{DiscussionEntry, DiscussionTopic, GroupTopicChild};
pub use enrollment::{Enrollment, EnrollmentGrades};
pub use file::File;
pub use folder::Folder;
pub use grading_period::{GradingPeriod, WrappedCollection};
pub use module::{Module, ModuleItem, ModuleItemContentDetails};
pub use new_quiz::{NewQuiz, NewQuizMultipleAttempts, NewQuizSettings};
pub use page::Page;
pub use planner::{Plannable, PlannerItem, PlannerOverride};
pub use quiz::{
    Quiz, QuizAnswer, QuizSubmission, QuizSubmissionQuestion, QuizSubmissionQuestionsDoc,
    QuizSubmissionsDoc,
};
pub use submission::{
    RubricAssessment, Submission, SubmissionAttachment, SubmissionComment, SubmissionHistoryEntry,
};
pub use term::Term;
pub use user::User;
pub use wiki_page::WikiPage;
