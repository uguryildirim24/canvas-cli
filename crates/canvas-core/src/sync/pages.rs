//! `pages` and `page` datasets (§10, M8-a).
//!
//! The listing never asks for `include[]=body`: a course can hold hundreds of
//! pages, and one body per page is a large fetch that a listing does not need.
//! A body arrives only from the `page` detail route, and the per-field write
//! rule (§10) merges it into the same row.

use canvas_api::models::WikiPage;
use jiff::{Span, Timestamp};
use rusqlite::{Transaction, params};
use serde_json::{Map, Value};

use crate::store::{
    Dataset, DbError, EntityIngest, FieldGroup, FieldWrite, IngestError, IngestPage,
    apply_field_writes,
};

use super::fields::{push_opt_bool, push_opt_str, push_opt_ts};
use super::folders::{merge_extra, parse_entity_id, touch_observed, validate_fields};

/// Default TTL: `ttl_pages` = 1 hour.
#[must_use]
pub fn default_ttl_pages() -> Span {
    Span::new().hours(1)
}

/// Pages listing for one course.
#[derive(Debug, Clone)]
pub struct PagesDataset {
    pub course_id: i64,
    pub ttl: Span,
    scope_key: String,
}

impl PagesDataset {
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
        Self::new(course_id, default_ttl_pages())
    }
}

impl Dataset for PagesDataset {
    fn name(&self) -> &'static str {
        "pages"
    }

    fn scope_key(&self) -> &str {
        &self.scope_key
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "page"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_page(tx, self.course_id, entity, fetched_at)
    }
}

/// One page fetched by slug or id, without replacing the listing membership.
///
/// The scope keeps the operand the caller used, because Canvas accepts both a
/// slug and a `page_id` and the two are different cache keys until the fetch
/// tells us which page they name.
#[derive(Debug, Clone)]
pub struct PageDetailDataset {
    pub course_id: i64,
    pub operand: String,
    ttl: Span,
    scope: String,
}

impl PageDetailDataset {
    #[must_use]
    pub fn new(course_id: i64, operand: &str, ttl: Span) -> Self {
        Self {
            course_id,
            operand: operand.to_owned(),
            ttl,
            scope: format!("page:{course_id}:{operand}"),
        }
    }
}

impl Dataset for PageDetailDataset {
    fn name(&self) -> &'static str {
        "page"
    }

    fn scope_key(&self) -> &str {
        &self.scope
    }

    fn ttl(&self) -> Span {
        self.ttl
    }

    fn entity_kind(&self) -> &'static str {
        "page"
    }

    fn upsert_entity(
        &self,
        tx: &Transaction<'_>,
        entity: &EntityIngest,
        fetched_at: Timestamp,
    ) -> Result<(), IngestError> {
        upsert_page(tx, self.course_id, entity, fetched_at)
    }
}

/// Listing path for course pages, sorted by title and without bodies.
#[must_use]
pub fn pages_path(course_id: i64) -> String {
    format!("/api/v1/courses/{course_id}/pages?sort=title")
}

/// Detail path for one page, addressed by slug or by `page_id`.
#[must_use]
pub fn page_path(course_id: i64, operand: &str) -> String {
    format!(
        "/api/v1/courses/{course_id}/pages/{}",
        encode_path_segment(operand)
    )
}

/// Percent-encode one path segment, keeping the unreserved set intact.
#[must_use]
pub fn encode_path_segment(raw: &str) -> String {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push('%');
            out.push(char::from(HEX[usize::from(byte >> 4)]));
            out.push(char::from(HEX[usize::from(byte & 0x0f)]));
        }
    }
    out
}

/// Convert page items into an ingest page.
///
/// A page without `page_id` cannot be keyed, so the whole fetch is a decode
/// failure rather than a listing that silently drops a row.
pub fn pages_to_ingest_page(
    items: &[WikiPage],
    course_id: i64,
    fetched_at: Timestamp,
) -> Result<IngestPage, canvas_api::Error> {
    let mut entities = Vec::with_capacity(items.len());
    for item in items {
        entities.push(page_to_entity(item, course_id)?);
    }
    Ok(IngestPage {
        fetched_at,
        entities,
    })
}

/// Convert one page into an entity ingest row.
///
/// `body` is `Detail`: the listing leaves it absent, so a cached body from an
/// earlier `page` fetch survives a listing refresh.
pub fn page_to_entity(item: &WikiPage, course_id: i64) -> Result<EntityIngest, canvas_api::Error> {
    let page_id = item.page_id.ok_or(canvas_api::Error::Decode)?;
    let mut fields = vec![FieldWrite {
        name: "course_id",
        group: FieldGroup::Core,
        value: Some(course_id.to_string()),
    }];
    push_opt_str(&mut fields, "url", FieldGroup::Core, item.url.as_deref());
    push_opt_str(
        &mut fields,
        "title",
        FieldGroup::Core,
        item.title.as_deref(),
    );
    push_opt_ts(&mut fields, "updated_at", FieldGroup::Core, item.updated_at);
    push_opt_ts(
        &mut fields,
        "created_at",
        FieldGroup::Detail,
        item.created_at,
    );
    push_opt_bool(&mut fields, "published", FieldGroup::Status, item.published);
    push_opt_bool(&mut fields, "front_page", FieldGroup::Core, item.front_page);
    push_opt_bool(
        &mut fields,
        "hide_from_students",
        FieldGroup::Status,
        item.hide_from_students,
    );
    push_opt_str(
        &mut fields,
        "editing_roles",
        FieldGroup::Detail,
        item.editing_roles.as_deref(),
    );
    push_opt_bool(
        &mut fields,
        "locked_for_user",
        FieldGroup::Status,
        item.locked_for_user,
    );
    push_opt_str(
        &mut fields,
        "body",
        FieldGroup::Detail,
        item.body.as_deref(),
    );
    if let Some(url) = item.html_url.as_ref() {
        fields.push(FieldWrite {
            name: "html_url",
            group: FieldGroup::Detail,
            value: Some(super::modules::without_capability(url)),
        });
    }
    Ok(EntityIngest {
        entity_key: page_id.to_string(),
        fields,
    })
}

const COLUMN_FIELDS: &[&str] = &["course_id", "url", "title", "body", "updated_at"];

const EXTRA_FIELDS: &[&str] = &[
    "created_at",
    "published",
    "front_page",
    "hide_from_students",
    "editing_roles",
    "locked_for_user",
    "html_url",
];

fn upsert_page(
    tx: &Transaction<'_>,
    dataset_course_id: i64,
    entity: &EntityIngest,
    fetched_at: Timestamp,
) -> Result<(), IngestError> {
    let id = parse_entity_id("page", &entity.entity_key)?;
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
            return Err(DbError::Message(format!("unsupported page field: {}", field.name)).into());
        }
    }
    validate_fields(&columns, COLUMN_FIELDS)?;
    tx.execute(
        "INSERT INTO pages (id, course_id) VALUES (?1, ?2) ON CONFLICT(id) DO NOTHING",
        params![id, dataset_course_id],
    )?;
    let applied = apply_field_writes(tx, "page", &entity.entity_key, fetched_at, &entity.fields)?;
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
                "UPDATE pages SET course_id = ?1 WHERE id = ?2",
                params![value, id],
            )?;
        } else {
            tx.execute(
                &format!("UPDATE pages SET {} = ?1 WHERE id = ?2", field.name),
                params![field.value, id],
            )?;
        }
    }
    let extra = extra
        .into_iter()
        .filter(|(name, _)| applied.fields.contains(&name.as_str()))
        .collect();
    merge_extra(tx, "pages", id, extra)?;
    touch_observed(
        tx,
        "pages",
        id,
        fetched_at,
        applied.core,
        applied.detail,
        applied.status,
    )?;
    Ok(())
}

/// Refresh the `pages` listing for one course (listing denial classification).
pub async fn refresh_pages(
    client: &canvas_api::Client,
    store: &crate::store::Store,
    course_id: i64,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<super::RefreshOutcome, super::SyncError> {
    use futures_util::StreamExt;

    use super::refresh::{FetchBundle, refresh_listing_with_denial};

    let dataset = PagesDataset::new(course_id, ttl);
    refresh_listing_with_denial(client, store, &dataset, now, fresh, offline, || async {
        let path = pages_path(course_id);
        let mut items: Vec<WikiPage> = Vec::new();
        let mut stream = std::pin::pin!(client.get_all::<WikiPage>(&path));
        while let Some(page) = stream.next().await {
            items.extend(page?.items);
        }
        Ok(FetchBundle {
            pages: vec![
                pages_to_ingest_page(&items, course_id, now).map_err(super::SyncError::from)?,
            ],
        })
    })
    .await
}

/// Refresh one page by slug or id.
#[allow(clippy::too_many_arguments)]
pub async fn refresh_page(
    client: &canvas_api::Client,
    store: &crate::store::Store,
    course_id: i64,
    operand: &str,
    ttl: Span,
    now: Timestamp,
    fresh: bool,
    offline: bool,
) -> Result<super::RefreshOutcome, super::SyncError> {
    use super::refresh::{FetchBundle, refresh_dataset};

    let dataset = PageDetailDataset::new(course_id, operand, ttl);
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
            let item: WikiPage = client.get(&page_path(course_id, operand)).await?;
            Ok(FetchBundle {
                pages: vec![IngestPage {
                    fetched_at: now,
                    entities: vec![page_to_entity(&item, course_id)?],
                }],
            })
        },
    )
    .await
}
