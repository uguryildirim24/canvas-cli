//! `enrollment_grades` dataset (`period:<id|none>`).

use canvas_api::models::Enrollment;
use jiff::{Span, Timestamp};
use rusqlite::Transaction;

use crate::store::{
    Dataset, EntityIngest, FieldGroup, IngestError, IngestPage, upsert_enrollment_grades,
};

use super::course_totals::default_ttl_grades;
use super::fields::{push_api_f64, push_api_str, push_opt_i64};

/// Grading-period qualifier for enrollment grades.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeriodKey {
    /// Whole-course / no grading period filter.
    None,
    /// Explicit Canvas grading period id.
    Id(i64),
}

impl PeriodKey {
    /// Scope key: `period:none` or `period:<id>`.
    #[must_use]
    pub fn scope_key(self) -> String {
        match self {
            Self::None => "period:none".to_owned(),
            Self::Id(id) => format!("period:{id}"),
        }
    }

    /// Period component stored in `enrollment_grades.period`.
    #[must_use]
    pub fn period_value(self) -> String {
        match self {
            Self::None => "none".to_owned(),
            Self::Id(id) => id.to_string(),
        }
    }
}

/// Enrollment grades dataset.
#[derive(Debug, Clone)]
pub struct EnrollmentGradesDataset {
    pub period: PeriodKey,
    pub ttl: Span,
    scope_key: String,
    period_value: String,
}

impl EnrollmentGradesDataset {
    #[must_use]
    pub fn new(period: PeriodKey, ttl: Span) -> Self {
        Self {
            scope_key: period.scope_key(),
            period_value: period.period_value(),
            period,
            ttl,
        }
    }

    #[must_use]
    pub fn with_default_ttl(period: PeriodKey) -> Self {
        Self::new(period, default_ttl_grades())
    }
}

impl Dataset for EnrollmentGradesDataset {
    fn name(&self) -> &'static str {
        "enrollment_grades"
    }

    fn scope_key(&self) -> &str {
        &self.scope_key
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "enrollment_grades"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        let enrollment_id = parse_enrollment_key(&entity.entity_key, &self.period_value)?;
        upsert_enrollment_grades(
            tx,
            enrollment_id,
            &self.period_value,
            fetched_at,
            &entity.fields,
        )
    }
}

/// Build the enrollments list path for a period scope.
#[must_use]
pub fn enrollment_grades_path(period: PeriodKey) -> String {
    let mut path = String::from(
        "/api/v1/users/self/enrollments?type[]=StudentEnrollment\
         &state[]=active&state[]=completed&per_page=100",
    );
    if let PeriodKey::Id(id) = period {
        use std::fmt::Write as _;
        let _ = write!(path, "&grading_period_id={id}");
    }
    path
}

/// Convert enrollment pages into an ingest page.
#[must_use]
pub fn enrollments_to_ingest_page(
    enrollments: &[Enrollment],
    period: PeriodKey,
    fetched_at: Timestamp,
) -> IngestPage {
    let period_value = period.period_value();
    IngestPage {
        fetched_at,
        entities: enrollments
            .iter()
            .map(|e| enrollment_to_entity(e, &period_value))
            .collect(),
    }
}

/// Convert one enrollment into an `enrollment_grades` entity.
#[must_use]
pub fn enrollment_to_entity(enrollment: &Enrollment, period_value: &str) -> EntityIngest {
    let mut fields = Vec::new();
    push_opt_i64(
        &mut fields,
        "course_id",
        FieldGroup::Core,
        enrollment.course_id,
    );
    if let Some(ref grades) = enrollment.grades {
        push_api_f64(
            &mut fields,
            "current_score",
            FieldGroup::Status,
            &grades.current_score,
        );
        push_api_f64(
            &mut fields,
            "final_score",
            FieldGroup::Status,
            &grades.final_score,
        );
        push_api_str(
            &mut fields,
            "current_grade",
            FieldGroup::Status,
            &grades.current_grade,
        );
        push_api_str(
            &mut fields,
            "final_grade",
            FieldGroup::Status,
            &grades.final_grade,
        );
    } else {
        push_api_f64(
            &mut fields,
            "current_score",
            FieldGroup::Status,
            &enrollment.computed_current_score,
        );
        push_api_f64(
            &mut fields,
            "final_score",
            FieldGroup::Status,
            &enrollment.computed_final_score,
        );
        push_api_str(
            &mut fields,
            "current_grade",
            FieldGroup::Status,
            &enrollment.computed_current_grade,
        );
        push_api_str(
            &mut fields,
            "final_grade",
            FieldGroup::Status,
            &enrollment.computed_final_grade,
        );
    }
    EntityIngest {
        entity_key: format!("{}|{period_value}", enrollment.id),
        fields,
    }
}

fn parse_enrollment_key(entity_key: &str, period_value: &str) -> Result<i64, IngestError> {
    let expected_suffix = format!("|{period_value}");
    let id_part = entity_key.strip_suffix(&expected_suffix).ok_or_else(|| {
        crate::store::DbError::Message(format!(
            "enrollment_grades key must end with |{period_value}"
        ))
    })?;
    let enrollment_id: i64 = id_part.parse().map_err(|e| {
        crate::store::DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("bad enrollment id: {e}"),
            ),
        )))
    })?;
    if entity_key != format!("{enrollment_id}|{period_value}") {
        return Err(crate::store::DbError::Message("entity key must be normalized".into()).into());
    }
    Ok(enrollment_id)
}
