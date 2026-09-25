//! `quizzes` and `quiz` datasets (§12.7, M10-a).
//!
//! The listing and the detail route answer the same shape, so one table holds
//! both and the per-field write rule (§10) merges a detail fetch into the row
//! the listing wrote. Only Classic Quizzes are covered; New Quizzes runs as
//! an LTI tool and stays a browser handoff (§1).

use canvas_api::models::Quiz;
use jiff::{Span, Timestamp};
use rusqlite::{Transaction, params};
use serde_json::{Map, Value};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, IngestPage,
    apply_field_writes,
};

use super::fields::{push_opt_bool, push_opt_f64, push_opt_i64, push_opt_str, push_opt_ts};
use super::folders::{merge_extra, parse_entity_id, touch_observed, validate_fields};

/// Default TTL: `ttl_quizzes` = 1 hour.
#[must_use]
pub fn default_ttl_quizzes() -> Span {
    Span::new().hours(1)
}

/// Quizzes listing for one course.
#[derive(Debug, Clone)]
pub struct QuizzesDataset {
    pub course_id: i64,
    pub ttl: Span,
    scope_key: String,
}

impl QuizzesDataset {
    #[must_use]
    pub fn new(course_id: i64, ttl: Span) -> Self {
        Self {
            course_id,
            ttl,
            scope_key: format!("course:{course_id}"),
        }
    }

    #[must_use]
    pub fn with_default_ttl(course_id: i64) -> Self {
        Self::new(course_id, default_ttl_quizzes())
    }
}

impl Dataset for QuizzesDataset {
    fn name(&self) -> &'static str {
        "quizzes"
    }

    fn scope_key(&self) -> &str {
        &self.scope_key
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "quiz"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_quiz(tx, self.course_id, entity, fetched_at)
    }
}

/// One quiz fetched by id, without replacing the listing membership.
#[derive(Debug, Clone)]
pub struct QuizDetailDataset {
    pub course_id: i64,
    pub quiz_id: i64,
    ttl: Span,
    scope: String,
}

impl QuizDetailDataset {
    #[must_use]
    pub fn new(course_id: i64, quiz_id: i64, ttl: Span) -> Self {
        Self {
            course_id,
            quiz_id,
            ttl,
            scope: format!("quiz:{course_id}:{quiz_id}"),
        }
    }
}

impl Dataset for QuizDetailDataset {
    fn name(&self) -> &'static str {
        "quiz"
    }

    fn scope_key(&self) -> &str {
        &self.scope
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "quiz"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_quiz(tx, self.course_id, entity, fetched_at)
    }
}

/// Listing path for the quizzes of one course.
#[must_use]
pub fn quizzes_path(course_id: i64) -> String {
    format!("/api/v1/courses/{course_id}/quizzes")
}

/// Detail path for one quiz.
#[must_use]
pub fn quiz_path(course_id: i64, quiz_id: i64) -> String {
    format!("/api/v1/courses/{course_id}/quizzes/{quiz_id}")
}

/// Convert quiz items into an ingest page.
pub fn quizzes_to_ingest_page(items: &[Quiz], course_id: i64, fetched_at: Timestamp) -> IngestPage {
    let mut entities = Vec::with_capacity(items.len());
    for item in items {
        entities.push(quiz_to_entity(item, course_id));
    }
    IngestPage {
        fetched_at,
        entities,
    }
}

/// Convert one quiz into an entity ingest row.
///
/// The fields a listing filter or a resolution needs are columns; the rest
/// travels in `data_json`, which the read side decodes back into a `Quiz`.
pub fn quiz_to_entity(item: &Quiz, course_id: i64) -> EntityIngest {
    let mut fields = vec![FieldWrite {
        name: "course_id",
        group: FieldGroup::Core,
        value: Some(course_id.to_string()),
    }];
    push_opt_str(
        &mut fields,
        "title",
        FieldGroup::Core,
        item.title.as_deref(),
    );
    push_opt_str(
        &mut fields,
        "description",
        FieldGroup::Core,
        item.description.as_deref(),
    );
    push_opt_ts(&mut fields, "due_at", FieldGroup::Core, item.due_at);
    push_opt_ts(&mut fields, "unlock_at", FieldGroup::Core, item.unlock_at);
    push_opt_ts(&mut fields, "lock_at", FieldGroup::Core, item.lock_at);
    push_status_fields(&mut fields, item);
    EntityIngest {
        entity_key: item.id.to_string(),
        fields,
    }
}

/// The status half of a quiz row: everything the read side decodes back.
fn push_status_fields(fields: &mut Vec<FieldWrite>, item: &Quiz) {
    push_opt_bool(fields, "published", FieldGroup::Status, item.published);
    push_opt_bool(
        fields,
        "locked_for_user",
        FieldGroup::Status,
        item.locked_for_user,
    );
    push_opt_bool(
        fields,
        "unlocked_for_user",
        FieldGroup::Status,
        item.unlocked_for_user,
    );
    push_opt_bool(
        fields,
        "one_question_at_a_time",
        FieldGroup::Status,
        item.one_question_at_a_time,
    );
    push_opt_bool(
        fields,
        "cant_go_back",
        FieldGroup::Status,
        item.cant_go_back,
    );
    push_opt_bool(
        fields,
        "require_lockdown_browser",
        FieldGroup::Status,
        item.require_lockdown_browser,
    );
    push_opt_str(
        fields,
        "ip_filter",
        FieldGroup::Status,
        item.ip_filter.as_deref(),
    );
    push_opt_str(
        fields,
        "lock_explanation",
        FieldGroup::Status,
        item.lock_explanation.as_deref(),
    );
    push_opt_str(
        fields,
        "quiz_type",
        FieldGroup::Status,
        item.quiz_type.as_deref(),
    );
    push_opt_i64(
        fields,
        "time_limit",
        FieldGroup::Status,
        item.time_limit
            .and_then(|minutes| i64::try_from(minutes).ok()),
    );
    push_opt_i64(
        fields,
        "allowed_attempts",
        FieldGroup::Status,
        item.allowed_attempts,
    );
    push_opt_i64(
        fields,
        "question_count",
        FieldGroup::Status,
        item.question_count.and_then(|n| i64::try_from(n).ok()),
    );
    push_opt_i64(
        fields,
        "assignment_id",
        FieldGroup::Status,
        item.assignment_id,
    );
    push_opt_f64(
        fields,
        "points_possible",
        FieldGroup::Status,
        item.points_possible,
    );
    push_opt_str(
        fields,
        "html_url",
        FieldGroup::Status,
        item.html_url.as_deref(),
    );
}

const COLUMN_FIELDS: &[&str] = &[
    "course_id",
    "title",
    "description",
    "due_at",
    "unlock_at",
    "lock_at",
];

const EXTRA_FIELDS: &[&str] = &[
    "published",
    "locked_for_user",
    "unlocked_for_user",
    "one_question_at_a_time",
    "cant_go_back",
    "require_lockdown_browser",
    "ip_filter",
    "lock_explanation",
    "quiz_type",
    "time_limit",
    "allowed_attempts",
    "question_count",
    "assignment_id",
    "points_possible",
    "html_url",
];

fn upsert_quiz(
    tx: &Transaction<'_>,
    dataset_course_id: i64,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_entity_id("quiz", &entity.entity_key)?;
    let mut columns = Vec::new();
    let mut extra = Map::new();
    for field in &entity.fields {
        if COLUMN_FIELDS.contains(&field.name) {
            columns.push(field.clone());
        } else if EXTRA_FIELDS.contains(&field.name) {
            extra.insert(
                field.name.to_owned(),
                match &field.value {
                    Some(v) => Value::String(v.clone()),
                    None => Value::Null,
                },
            );
        } else {
            return Err(DbError::Message(format!("unsupported quiz field: {}", field.name)).into());
        }
    }
    validate_fields(&columns, COLUMN_FIELDS)?;
    tx.execute(
        "INSERT INTO quizzes (id, course_id) VALUES (?1, ?2) ON CONFLICT(id) DO NOTHING",
        params![id, dataset_course_id],
    )?;
    let applied = apply_field_writes(tx, "quiz", &entity.entity_key, fetched_at, &entity.fields)?;
    for field in &columns {
        if !applied.fields.contains(&field.name) {
            continue;
        }
        if field.name == "course_id" {
            let value: Option<i64> = field
                .value
                .as_ref()
                .map(|s| s.parse())
                .transpose()
                .map_err(|e: std::num::ParseIntError| {
                    DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                })?;
            tx.execute(
                "UPDATE quizzes SET course_id = ?1 WHERE id = ?2",
                params![value, id],
            )?;
        } else {
            tx.execute(
                &format!("UPDATE quizzes SET {} = ?1 WHERE id = ?2", field.name),
                params![field.value, id],
            )?;
        }
    }
    let extra = extra
        .into_iter()
        .filter(|(name, _)| applied.fields.contains(&name.as_str()))
        .collect();
    merge_extra(tx, "quizzes", id, extra)?;
    touch_observed(
        tx,
        "quizzes",
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    Ok(())
}

/// Refresh the `quizzes` listing for one course.
#[allow(clippy::too_many_arguments)]
pub async fn refresh_quizzes(
    client: &canvas_api::Client,
    store: &crate::store::Store,
    course_id: i64,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<super::RefreshOutcome, super::SyncError> {
    use futures_util::StreamExt;

    use super::refresh::{FetchBundle, refresh_dataset};

    let dataset = QuizzesDataset::new(course_id, ttl);
    refresh_dataset(
        client,
        store,
        &dataset,
        now,
        fresh,
        offline,
        None,
        None,
        || async {
            let path = quizzes_path(course_id);
            let mut items: Vec<Quiz> = Vec::new();
            let mut stream = std::pin::pin!(client.get_all::<Quiz>(&path));
            while let Some(page) = stream.next().await {
                items.extend(page?.items);
            }
            Ok(FetchBundle {
                pages: vec![quizzes_to_ingest_page(&items, course_id, now)],
            })
        },
    )
    .await
}

/// Refresh one quiz by id.
#[allow(clippy::too_many_arguments)]
pub async fn refresh_quiz(
    client: &canvas_api::Client,
    store: &crate::store::Store,
    course_id: i64,
    quiz_id: i64,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<super::RefreshOutcome, super::SyncError> {
    use super::refresh::{FetchBundle, refresh_dataset};

    let dataset = QuizDetailDataset::new(course_id, quiz_id, ttl);
    refresh_dataset(
        client,
        store,
        &dataset,
        now,
        fresh,
        offline,
        None,
        None,
        || async {
            let item: Quiz = client.get(&quiz_path(course_id, quiz_id)).await?;
            Ok(FetchBundle {
                pages: vec![IngestPage {
                    fetched_at: now,
                    entities: vec![quiz_to_entity(&item, course_id)],
                }],
            })
        },
    )
    .await
}
