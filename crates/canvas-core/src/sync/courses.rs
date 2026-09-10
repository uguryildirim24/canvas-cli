//! `courses` dataset (`active` | `all`).

use canvas_api::models::{Course, CourseEnrollment};
use jiff::{Span, Timestamp};
use rusqlite::{Transaction, params};
use serde_json::{Map, Value};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, IngestPage,
    apply_field_writes,
};

use super::course_totals::{totals_from_enrollment, upsert_course_totals_entity};
use super::fields::{
    parse_opt_bool, push_api_str, push_api_to_string, push_opt_bool, push_opt_i64, push_opt_str,
};
use super::terms::{term_to_entity, upsert_term};

/// Default TTL: `ttl_courses` = 6 hours.
#[must_use]
pub fn default_ttl_courses() -> Span {
    Span::new().hours(6)
}

/// Enrollment-state scopes for the courses dataset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoursesScope {
    /// `enrollment_state=active` only.
    Active,
    /// Three requests: `active`, `completed`, `invited_or_pending`.
    All,
}

impl CoursesScope {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::All => "all",
        }
    }

    /// Enrollment states fetched for this scope (one GET per state).
    #[must_use]
    pub fn enrollment_states(self) -> &'static [&'static str] {
        match self {
            Self::Active => &["active"],
            Self::All => &["active", "completed", "invited_or_pending"],
        }
    }
}

/// Courses list dataset.
#[derive(Debug, Clone)]
pub struct CoursesDataset {
    pub scope: CoursesScope,
    pub ttl: Span,
}

impl CoursesDataset {
    #[must_use]
    pub fn new(scope: CoursesScope, ttl: Span) -> Self {
        Self { scope, ttl }
    }
}

impl Dataset for CoursesDataset {
    fn name(&self) -> &'static str {
        "courses"
    }

    fn scope_key(&self) -> &str {
        self.scope.as_str()
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "course"
    }

    fn current_epoch(&self, state: &rusqlite::Connection) -> Result<i64, DbError> {
        // A list response can contain any course. Guard all totals epochs so a
        // concurrent submission cannot publish old totals as fresh.
        let totals: i64 = state.query_row(
            "SELECT COALESCE(SUM(epoch), 0) FROM scope_epoch WHERE scope LIKE 'course_totals:%'",
            [],
            |r| r.get(0),
        )?;
        crate::store::read_scope_epoch(state, &self.epoch_scope())?
            .checked_add(totals)
            .ok_or_else(|| DbError::Message("scope epoch overflow".into()))
    }

    fn finish_refresh(
        &self,
        tx: &Transaction<'_>,
        state: &rusqlite::Connection,
        pages: &[IngestPage],
    ) -> Result<(), IngestError> {
        publish_derived(tx, state, pages, self.scope_key())
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_course_bundle(tx, entity, fetched_at)
    }
}

/// Build the courses list path for one enrollment state.
#[must_use]
pub fn courses_path(enrollment_state: &str) -> String {
    format!(
        "/api/v1/courses?enrollment_type=student&enrollment_state={enrollment_state}\
         &include[]=term&include[]=total_scores&include[]=current_grading_period_scores\
         &include[]=favorites&per_page=100"
    )
}

/// Paths required for a courses refresh (one per enrollment state).
#[must_use]
pub fn courses_fetch_paths(scope: CoursesScope) -> Vec<String> {
    scope
        .enrollment_states()
        .iter()
        .map(|state| courses_path(state))
        .collect()
}

/// Convert one course page into an ingest page.
#[must_use]
pub fn courses_to_ingest_page(
    courses: &[Course],
    enrollment_state_hint: &str,
    fetched_at: Timestamp,
) -> IngestPage {
    IngestPage {
        fetched_at,
        entities: courses
            .iter()
            .map(|c| course_to_entity(c, enrollment_state_hint))
            .collect(),
    }
}

/// Convert a Canvas course into a course entity with side-effect fields.
#[must_use]
pub fn course_to_entity(course: &Course, enrollment_state_hint: &str) -> EntityIngest {
    let mut fields = Vec::new();
    push_api_str(&mut fields, "name", FieldGroup::Core, &course.name);
    push_opt_str(
        &mut fields,
        "course_code",
        FieldGroup::Core,
        course.course_code.as_deref(),
    );
    push_api_str(
        &mut fields,
        "workflow_state",
        FieldGroup::Status,
        &course.workflow_state,
    );
    push_api_to_string(&mut fields, "html_url", FieldGroup::Core, &course.html_url);

    let enrollment_state = course
        .enrollment_state
        .as_deref()
        .or_else(|| student_enrollment(course).and_then(|e| e.enrollment_state.as_deref()))
        .unwrap_or(enrollment_state_hint);
    if !enrollment_state.is_empty() {
        fields.push(FieldWrite {
            name: "enrollment_state",
            group: FieldGroup::Status,
            value: Some(enrollment_state.to_owned()),
        });
    }
    push_opt_bool(
        &mut fields,
        "is_favorite",
        FieldGroup::Detail,
        course.is_favorite,
    );
    push_opt_bool(
        &mut fields,
        "restricted",
        FieldGroup::Detail,
        course.restricted,
    );

    if let Some(ref term) = course.term {
        if let Some(id) = term.id {
            push_opt_i64(&mut fields, "term_id", FieldGroup::Core, Some(id));
        }
        if let Some(term_entity) = term_to_entity(term)
            && let Ok(json) = serde_json::to_string(&json_field_map(&term_entity.fields))
        {
            fields.push(FieldWrite {
                name: "term_payload",
                group: FieldGroup::Detail,
                value: Some(json),
            });
        }
    }

    if let Some(enrollment) = student_enrollment(course) {
        let totals = totals_from_enrollment(course.id, enrollment);
        if let Ok(json) = serde_json::to_string(&totals_payload(&totals)) {
            fields.push(FieldWrite {
                name: "totals_payload",
                group: FieldGroup::Status,
                value: Some(json),
            });
        }
    }

    EntityIngest {
        entity_key: course.id.to_string(),
        fields,
    }
}

pub(super) fn student_enrollment(course: &Course) -> Option<&CourseEnrollment> {
    let enrollments = course.enrollments.as_ref()?;
    enrollments.iter().find(|e| {
        e.enrollment_type.as_deref().is_some_and(|t| {
            t.eq_ignore_ascii_case("StudentEnrollment") || t.eq_ignore_ascii_case("student")
        }) || e.role.as_deref().is_some_and(|r| {
            r.eq_ignore_ascii_case("StudentEnrollment") || r.eq_ignore_ascii_case("Student")
        })
    })
}

fn json_field_map(fields: &[FieldWrite]) -> Map<String, Value> {
    let mut map = Map::new();
    for field in fields {
        map.insert(
            field.name.to_owned(),
            match &field.value {
                Some(v) => Value::String(v.clone()),
                None => Value::Null,
            },
        );
    }
    map
}

fn totals_payload(entities: &[EntityIngest]) -> Vec<Value> {
    entities
        .iter()
        .map(|e| {
            serde_json::json!({
                "entity_key": e.entity_key,
                "fields": json_field_map(&e.fields),
            })
        })
        .collect()
}

struct CourseExtras {
    enrollment_state: crate::store::Supplied<String>,
    is_favorite: crate::store::Supplied<bool>,
    restricted: crate::store::Supplied<bool>,
    term_id: crate::store::Supplied<i64>,
    term_payload: Option<String>,
    totals_payload: Option<String>,
    detail: Map<String, Value>,
}

fn upsert_course_bundle(
    tx: &Transaction<'_>,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_course_id(&entity.entity_key)?;
    let (tracked, extras) = split_course_fields(&entity.fields)?;
    validate_tracked(&tracked)?;
    tx.execute(
        "INSERT INTO courses (id) VALUES (?1) ON CONFLICT(id) DO NOTHING",
        params![id],
    )?;
    let real_fields: Vec<_> = entity
        .fields
        .iter()
        .filter(|f| !matches!(f.name, "term_payload" | "totals_payload"))
        .cloned()
        .collect();
    let applied = apply_field_writes(tx, "course", &entity.entity_key, fetched_at, &real_fields)?;
    let winning: Vec<_> = entity
        .fields
        .iter()
        .filter(|f| {
            applied.fields.contains(&f.name) || matches!(f.name, "term_payload" | "totals_payload")
        })
        .cloned()
        .collect();
    let (_, winning_extras) = split_course_fields(&winning)?;
    apply_tracked_columns(tx, id, &tracked, &applied.fields)?;
    match winning_extras.term_id {
        crate::store::Supplied::Absent => {}
        crate::store::Supplied::Null => {
            tx.execute(
                "UPDATE courses SET term_id = NULL WHERE id = ?1",
                params![id],
            )?;
        }
        crate::store::Supplied::Value(tid) => {
            tx.execute(
                "UPDATE courses SET term_id = ?1 WHERE id = ?2",
                params![tid, id],
            )?;
        }
    }
    write_course_data_json(tx, id, &winning_extras)?;
    touch_course_observed(
        tx,
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    write_term_side_effect(tx, &extras, fetched_at)?;
    write_totals_side_effect(tx, &extras, fetched_at)?;
    Ok(())
}

fn parse_course_id(entity_key: &str) -> Result<i64, IngestError> {
    let id: i64 = entity_key.parse().map_err(|e| {
        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("bad course id: {e}"),
            ),
        )))
    })?;
    if entity_key != id.to_string() {
        return Err(DbError::Message("entity key must be normalized".into()).into());
    }
    Ok(id)
}

fn split_course_fields(
    fields: &[FieldWrite],
) -> Result<(Vec<FieldWrite>, CourseExtras), IngestError> {
    use crate::store::Supplied;
    let mut tracked = Vec::new();
    let mut extras = CourseExtras {
        enrollment_state: Supplied::Absent,
        is_favorite: Supplied::Absent,
        restricted: Supplied::Absent,
        term_id: Supplied::Absent,
        term_payload: None,
        totals_payload: None,
        detail: Map::new(),
    };
    for field in fields {
        match field.name {
            "name" | "course_code" | "workflow_state" | "html_url" => tracked.push(field.clone()),
            "enrollment_state" => {
                extras.enrollment_state = match &field.value {
                    Some(v) => Supplied::Value(v.clone()),
                    None => Supplied::Null,
                };
            }
            "is_favorite" => {
                extras.is_favorite = match parse_opt_bool(field.value.as_ref()) {
                    Some(v) => Supplied::Value(v),
                    None => Supplied::Null,
                };
            }
            "restricted" => {
                extras.restricted = match parse_opt_bool(field.value.as_ref()) {
                    Some(v) => Supplied::Value(v),
                    None => Supplied::Null,
                };
            }
            "term_id" => {
                extras.term_id = match &field.value {
                    Some(s) => Supplied::Value(s.parse().map_err(|e| {
                        DbError::Sqlite(rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
                    })?),
                    None => Supplied::Null,
                };
            }
            "syllabus_markdown"
            | "teachers"
            | "time_zone"
            | "modules_count"
            | "has_grading_periods" => {
                let value = if let Some(raw) = &field.value {
                    if matches!(
                        field.name,
                        "teachers" | "modules_count" | "has_grading_periods"
                    ) {
                        serde_json::from_str(raw)
                            .map_err(|_| DbError::Message("invalid course detail field".into()))?
                    } else {
                        Value::String(raw.clone())
                    }
                } else {
                    Value::Null
                };
                extras.detail.insert(field.name.into(), value);
            }
            "term_payload" => extras.term_payload.clone_from(&field.value),
            "totals_payload" => extras.totals_payload.clone_from(&field.value),
            other => {
                return Err(DbError::Message(format!("unsupported course field: {other}")).into());
            }
        }
    }
    Ok((tracked, extras))
}

fn apply_tracked_columns(
    tx: &Transaction<'_>,
    id: i64,
    tracked: &[FieldWrite],
    applied: &[&str],
) -> Result<(), IngestError> {
    for field in tracked {
        if !applied.contains(&field.name) {
            continue;
        }
        match field.name {
            "name" | "course_code" | "workflow_state" | "html_url" => {
                tx.execute(
                    &format!("UPDATE courses SET {} = ?1 WHERE id = ?2", field.name),
                    params![field.value, id],
                )?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn write_course_data_json(
    tx: &Transaction<'_>,
    id: i64,
    extras: &CourseExtras,
) -> Result<(), IngestError> {
    use crate::store::Supplied;
    let mut data = load_course_data_json(tx, id)?;
    let obj = data
        .as_object_mut()
        .ok_or_else(|| DbError::Message("courses data_json must be an object".into()))?;
    match &extras.enrollment_state {
        Supplied::Absent => {}
        Supplied::Null => {
            obj.insert("enrollment_state".into(), Value::Null);
        }
        Supplied::Value(s) => {
            obj.insert("enrollment_state".into(), Value::String(s.clone()));
        }
    }
    match extras.is_favorite {
        Supplied::Absent => {}
        Supplied::Null => {
            obj.insert("is_favorite".into(), Value::Null);
        }
        Supplied::Value(b) => {
            obj.insert("is_favorite".into(), Value::Bool(b));
        }
    }
    match extras.restricted {
        Supplied::Absent => {}
        Supplied::Null => {
            obj.insert("restricted".into(), Value::Null);
        }
        Supplied::Value(b) => {
            obj.insert("restricted".into(), Value::Bool(b));
        }
    }
    obj.extend(extras.detail.clone());
    tx.execute(
        "UPDATE courses SET data_json = ?1 WHERE id = ?2",
        params![data.to_string(), id],
    )?;
    Ok(())
}

fn touch_course_observed(
    tx: &Transaction<'_>,
    id: i64,
    fetched_at: Timestamp,
    core: bool,
    detail: bool,
    status: bool,
) -> Result<(), IngestError> {
    let ts = fetched_at.to_string();
    if core {
        tx.execute(
            "UPDATE courses SET observed_at_core = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    if detail {
        tx.execute(
            "UPDATE courses SET observed_at_detail = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    if status {
        tx.execute(
            "UPDATE courses SET observed_at_status = ?1 WHERE id = ?2",
            params![ts, id],
        )?;
    }
    Ok(())
}

fn write_term_side_effect(
    tx: &Transaction<'_>,
    extras: &CourseExtras,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    use crate::store::Supplied;
    let (Some(payload), Supplied::Value(tid)) = (&extras.term_payload, &extras.term_id) else {
        return Ok(());
    };
    let map: Map<String, Value> = serde_json::from_str(payload).map_err(|e| {
        DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))
    })?;
    let mut term_fields = Vec::new();
    for (name, value) in map {
        let static_name: &'static str = match name.as_str() {
            "name" => "name",
            "start_at" => "start_at",
            "end_at" => "end_at",
            _ => continue,
        };
        term_fields.push(FieldWrite {
            name: static_name,
            group: FieldGroup::Core,
            value: match value {
                Value::Null => None,
                Value::String(s) => Some(s),
                other => Some(other.to_string()),
            },
        });
    }
    upsert_term(
        tx,
        &EntityIngest {
            entity_key: tid.to_string(),
            fields: term_fields,
        },
        fetched_at,
    )
}

fn write_totals_side_effect(
    tx: &Transaction<'_>,
    extras: &CourseExtras,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let Some(payload) = extras.totals_payload.as_ref() else {
        return Ok(());
    };
    let items: Vec<Value> = serde_json::from_str(payload).map_err(|e| {
        DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))
    })?;
    for item in items {
        let key = item
            .get("entity_key")
            .and_then(Value::as_str)
            .ok_or_else(|| DbError::Message("totals payload missing entity_key".into()))?;
        let field_map = item
            .get("fields")
            .and_then(Value::as_object)
            .ok_or_else(|| DbError::Message("totals payload missing fields".into()))?;
        let mut fields = Vec::new();
        for (name, value) in field_map {
            let static_name: &'static str = match name.as_str() {
                "current_score" => "current_score",
                "final_score" => "final_score",
                "current_grade" => "current_grade",
                "final_grade" => "final_grade",
                "period_id" => "period_id",
                "period_title" => "period_title",
                _ => continue,
            };
            let group = match static_name {
                "period_id" | "period_title" => FieldGroup::Detail,
                _ => FieldGroup::Status,
            };
            fields.push(FieldWrite {
                name: static_name,
                group,
                value: match value {
                    Value::Null => None,
                    Value::String(s) => Some(s.clone()),
                    other => Some(other.to_string()),
                },
            });
        }
        upsert_course_totals_entity(
            tx,
            &EntityIngest {
                entity_key: key.to_owned(),
                fields,
            },
            fetched_at,
        )?;
    }
    Ok(())
}

fn load_course_data_json(tx: &Transaction<'_>, id: i64) -> Result<Value, IngestError> {
    let raw: String = tx.query_row(
        "SELECT data_json FROM courses WHERE id = ?1",
        params![id],
        |r| r.get(0),
    )?;
    serde_json::from_str(&raw).map_err(|e| {
        DbError::Sqlite(rusqlite::Error::FromSqlConversionFailure(
            0,
            rusqlite::types::Type::Text,
            Box::new(e),
        ))
        .into()
    })
}

fn validate_tracked(fields: &[FieldWrite]) -> Result<(), IngestError> {
    let allowed = ["name", "course_code", "workflow_state", "html_url"];
    let mut seen = std::collections::HashSet::new();
    for field in fields {
        if !allowed.contains(&field.name) || !seen.insert(field.name) {
            return Err(DbError::Message("unsupported or duplicate course field".into()).into());
        }
    }
    Ok(())
}

/// Coverage for embedded terms and totals is committed atomically with courses.
pub(super) fn publish_derived(
    tx: &Transaction<'_>,
    state: &rusqlite::Connection,
    pages: &[IngestPage],
    scope: &str,
) -> Result<(), IngestError> {
    let Some(page) = pages.last() else {
        return Ok(());
    };
    let mut terms = std::collections::BTreeSet::new();
    let mut courses = std::collections::BTreeSet::new();
    for entity in pages.iter().flat_map(|p| &p.entities) {
        for field in &entity.fields {
            if field.name == "term_id"
                && let Some(id) = &field.value
            {
                terms.insert(id.clone());
            }
        }
        courses.insert(entity.entity_key.clone());
    }
    publish_coverage(
        tx,
        state,
        "terms",
        scope,
        "term",
        &terms.into_iter().collect::<Vec<_>>(),
        page.fetched_at,
    )?;
    for id in courses {
        // Unknown scores still have nullable rows; absent fields keep their own age.
        for mode in ["all", "current"] {
            tx.execute("INSERT INTO course_totals (course_id, mode) VALUES (?1, ?2) ON CONFLICT DO NOTHING", params![id, mode])?;
        }
        publish_coverage(
            tx,
            state,
            "course_totals",
            &format!("course:{id}"),
            "course_totals",
            &[format!("{id}|all"), format!("{id}|current")],
            page.fetched_at,
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn publish_coverage(
    tx: &Transaction<'_>,
    state: &rusqlite::Connection,
    dataset: &str,
    scope: &str,
    kind: &str,
    ids: &[String],
    at: Timestamp,
) -> Result<(), IngestError> {
    tx.execute(
        "DELETE FROM membership WHERE dataset=?1 AND scope=?2",
        params![dataset, scope],
    )?;
    for (position, id) in ids.iter().enumerate() {
        tx.execute("INSERT INTO membership (dataset,scope,entity_kind,entity_id,position) VALUES (?1,?2,?3,?4,?5)", params![dataset, scope, kind, id, i64::try_from(position).unwrap_or(i64::MAX)])?;
    }
    let epoch = crate::store::read_scope_epoch(state, &format!("{dataset}:{scope}"))?;
    tx.execute("INSERT INTO fetch_log (dataset,scope,fetched_at,complete,count,stale,epoch_seen) VALUES (?1,?2,?3,1,?4,0,?5) ON CONFLICT(dataset,scope) DO UPDATE SET fetched_at=excluded.fetched_at, complete=1,count=excluded.count,stale=0,error=NULL,epoch_seen=excluded.epoch_seen", params![dataset, scope, at.to_string(), i64::try_from(ids.len()).unwrap_or(i64::MAX), epoch])?;
    Ok(())
}
