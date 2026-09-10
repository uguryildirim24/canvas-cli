//! Preserve field presence while projecting API models onto allowlisted cache fields.

use super::{
    SyncError, assignment_to_entity, course_to_entity, enrollment_to_entity,
    grading_period_to_entity,
};
use crate::store::{EntityIngest, FieldGroup, FieldWrite};
use canvas_api::{
    Supplied,
    models::{Assignment, Course, Enrollment, GradingPeriod},
};
use serde::{Deserialize, Deserializer, de::DeserializeOwned};
use serde_json::{Value, json};

pub(super) struct Observed<T> {
    pub model: T,
    raw: Value,
}

impl<'de, T: DeserializeOwned> Deserialize<'de> for Observed<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let raw = Value::deserialize(d)?;
        if raw.get("id").is_none_or(Value::is_null) {
            return Err(serde::de::Error::custom("missing entity id"));
        }
        let model = serde_json::from_value(raw.clone()).map_err(serde::de::Error::custom)?;
        Ok(Self { model, raw })
    }
}

fn supplied(raw: &Value, name: &str) -> Supplied<Value> {
    match raw.get(name) {
        None => Supplied::Absent,
        Some(Value::Null) => Supplied::Null,
        Some(v) => Supplied::Value(v.clone()),
    }
}

fn observe(fields: &mut Vec<FieldWrite>, raw: &Value, name: &'static str, group: FieldGroup) {
    let value = match supplied(raw, name) {
        Supplied::Absent => return,
        Supplied::Null => None,
        Supplied::Value(Value::String(v)) => Some(v),
        Supplied::Value(v) => Some(v.to_string()),
    };
    fields.retain(|f| f.name != name);
    fields.push(FieldWrite { name, group, value });
}

impl Observed<Course> {
    #[allow(clippy::too_many_lines)]
    pub async fn entity(self, hint: &str) -> Result<EntityIngest, SyncError> {
        if self
            .raw
            .get("modules_count")
            .is_some_and(|v| !v.is_null() && v.as_u64().is_none())
            || self
                .raw
                .get("has_grading_periods")
                .is_some_and(|v| !v.is_null() && !v.is_boolean())
        {
            return Err(canvas_api::Error::Decode.into());
        }
        let mut entity = course_to_entity(&self.model, hint);
        let fields = &mut entity.fields;
        for (name, group) in [
            ("course_code", FieldGroup::Core),
            ("enrollment_state", FieldGroup::Status),
            ("is_favorite", FieldGroup::Detail),
            ("restricted", FieldGroup::Detail),
            ("time_zone", FieldGroup::Detail),
            ("modules_count", FieldGroup::Detail),
            ("has_grading_periods", FieldGroup::Detail),
        ] {
            observe(fields, &self.raw, name, group);
        }
        if self.raw.get("access_restricted_by_date").is_some()
            && self.raw.get("restricted").is_none()
        {
            observe(
                fields,
                &json!({"restricted": self.raw["access_restricted_by_date"]}),
                "restricted",
                FieldGroup::Detail,
            );
        }
        match supplied(&self.raw, "term") {
            Supplied::Null => observe(
                fields,
                &json!({"term_id": null}),
                "term_id",
                FieldGroup::Core,
            ),
            Supplied::Value(raw) => {
                if let Some(term) = &self.model.term {
                    let mut payload = serde_json::Map::new();
                    for (name, value) in [
                        ("name", term.name.clone()),
                        ("start_at", term.start_at.map(|v| v.to_string())),
                        ("end_at", term.end_at.map(|v| v.to_string())),
                    ] {
                        if raw.get(name).is_some() {
                            payload.insert(name.into(), value.map_or(Value::Null, Value::String));
                        }
                    }
                    fields.retain(|f| f.name != "term_payload");
                    fields.push(FieldWrite {
                        name: "term_payload",
                        group: FieldGroup::Detail,
                        value: Some(Value::Object(payload).to_string()),
                    });
                    if raw.get("id").is_some() {
                        observe(
                            fields,
                            &json!({"term_id": term.id}),
                            "term_id",
                            FieldGroup::Core,
                        );
                    }
                }
            }
            Supplied::Absent => {}
        }
        if self.raw.get("teachers").is_some() {
            let teachers: Vec<_> = self
                .model
                .teachers
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|t| json!({"id": t.id.to_string(), "name": t.name}))
                .collect();
            observe(
                fields,
                &json!({"teachers": teachers}),
                "teachers",
                FieldGroup::Detail,
            );
        }
        if self.raw.get("syllabus_body").is_some() {
            let markdown = match &self.model.syllabus_body {
                Some(html) => Some(
                    crate::markdown::html_to_markdown(html)
                        .await
                        .map_err(|_| canvas_api::Error::Decode)?,
                ),
                None => None,
            };
            observe(
                fields,
                &json!({"syllabus_markdown": markdown}),
                "syllabus_markdown",
                FieldGroup::Detail,
            );
        }
        // Period metadata has its own clocks; it must never borrow whole-course scores.
        if let Some(enrollment) = super::courses::student_enrollment(&self.model) {
            let index = self
                .model
                .enrollments
                .as_ref()
                .unwrap()
                .iter()
                .position(|e| std::ptr::eq(e, enrollment))
                .unwrap();
            let raw = &self.raw["enrollments"][index];
            if let Some(payload) = fields.iter_mut().find(|f| f.name == "totals_payload") {
                let mut totals: Value = serde_json::from_str(payload.value.as_deref().unwrap())
                    .map_err(|_| canvas_api::Error::Decode)?;
                let map = totals[1]["fields"].as_object_mut().unwrap();
                for (source, dest) in [
                    ("current_grading_period_id", "period_id"),
                    ("current_grading_period_title", "period_title"),
                ] {
                    if raw.get(source).is_some_and(Value::is_null) {
                        map.insert(dest.into(), Value::Null);
                    }
                }
                payload.value = Some(totals.to_string());
            }
        }
        Ok(entity)
    }
}

impl Observed<Enrollment> {
    pub fn entity(self, period: &str) -> EntityIngest {
        let mut entity = enrollment_to_entity(&self.model, period);
        observe(&mut entity.fields, &self.raw, "course_id", FieldGroup::Core);
        if self.raw.get("grades").is_some_and(Value::is_null) {
            for name in [
                "current_score",
                "final_score",
                "current_grade",
                "final_grade",
            ] {
                observe(
                    &mut entity.fields,
                    &json!({name: null}),
                    name,
                    FieldGroup::Status,
                );
            }
        }
        entity
    }
}

impl Observed<GradingPeriod> {
    pub fn entity(self, course_id: i64) -> EntityIngest {
        let mut entity = grading_period_to_entity(&self.model, course_id);
        // Typed timestamps were normalized by the API model; nulls still need writes.
        for name in [
            "title",
            "start_date",
            "end_date",
            "close_date",
            "weight",
            "is_closed",
        ] {
            if self.raw.get(name).is_some_and(Value::is_null) {
                let group = match name {
                    "close_date" | "weight" => FieldGroup::Detail,
                    "is_closed" => FieldGroup::Status,
                    _ => FieldGroup::Core,
                };
                observe(&mut entity.fields, &self.raw, name, group);
            }
        }
        entity
    }
}

impl Observed<Assignment> {
    /// Project allowlisted fields, preserving explicit null for optional fields.
    pub fn entity(self, course_hint: Option<i64>) -> EntityIngest {
        let mut entity = assignment_to_entity(&self.model, course_hint);
        for name in ["locked_for_user", "lock_explanation", "group_category_id"] {
            if self.raw.get(name).is_some_and(Value::is_null) {
                observe(&mut entity.fields, &self.raw, name, FieldGroup::Detail);
            }
        }
        // Canvas exposes the tool's display name separately from capability URLs.
        if let Some(tool) = self.raw.get("external_tool_tag_attributes")
            && (tool.is_null() || tool.get("name").is_some())
        {
            observe(
                &mut entity.fields,
                &json!({"external_tool_name": tool.get("name")}),
                "external_tool_name",
                FieldGroup::Detail,
            );
        }
        if let Some(sub) = self.raw.get("submission") {
            for name in ["extra_attempts", "posted_at"] {
                if sub.get(name).is_some_and(Value::is_null) {
                    observe(&mut entity.fields, sub, name, FieldGroup::Status);
                }
            }
            if sub.is_null() {
                for name in [
                    "submitted",
                    "graded",
                    "score",
                    "grade",
                    "late",
                    "missing",
                    "excused",
                    "workflow_state",
                    "attempt",
                    "submitted_at",
                    "posted_at",
                    "extra_attempts",
                ] {
                    entity.fields.retain(|f| f.name != name);
                    entity.fields.push(FieldWrite {
                        name,
                        group: FieldGroup::Status,
                        value: None,
                    });
                }
            }
        }
        entity
    }
}
