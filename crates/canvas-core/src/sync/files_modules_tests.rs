//! Unit tests for folders/files/modules sync (M3-a).

use std::fs;

use canvas_api::models::{File, Folder, Module, ModuleItem};
use canvas_api::{Client, Secret};
use jiff::{Span, Timestamp};
use serde_json::json;
use tempfile::TempDir;
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

use crate::identity::{IdentityDocument, Paths};
use crate::store::{
    Dataset, FieldGroup, IngestOpts, OpenIdentity, lookup_dataset, read_scope_epoch,
};

use super::files::{FilesDataset, default_ttl_files, file_to_entity};
use super::folders::FoldersDataset;
use super::modules::{
    ModulesDataset, count_item_fetch_requests, default_ttl_modules, module_to_entity,
    needs_items_fetch,
};
use super::outcome::{FreshnessSource, SyncError};
use super::refresh::{
    DenialAction, classify_listing_denial, listing_denial_status, refresh_files, refresh_folders,
};
use super::{discovery_plan_input, is_canvas_root};

fn ts(secs: i64) -> Timestamp {
    Timestamp::from_second(secs).unwrap()
}

fn setup() -> (TempDir, OpenIdentity) {
    let dir = TempDir::new().unwrap();
    let doc = IdentityDocument::new(
        "https://lasell.instructure.com",
        12345,
        "2026-01-01T00:00:00Z",
    );
    let paths = Paths::for_identity(dir.path(), &doc.key);
    fs::create_dir_all(&paths.identity_dir).unwrap();
    fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
    doc.write(&paths.identity_json()).unwrap();
    let open = OpenIdentity::open(&paths, &doc).unwrap();
    (dir, open)
}

fn ingest_ok(epoch_seen: i64) -> IngestOpts<'static> {
    IngestOpts {
        epoch_seen,
        complete: true,
        stale: false,
        error: None,
        window: None,
        contexts: None,
    }
}

fn client(server: &MockServer) -> Client {
    Client::new(
        server.uri().parse().unwrap(),
        Secret::new("test-token"),
        "canvas-cli/test",
    )
    .unwrap()
}

#[test]
fn inline_item_completeness_decision() {
    let absent = Module {
        id: 1,
        items_count: Some(2),
        items: None,
        ..Module::default()
    };
    assert!(needs_items_fetch(&absent));

    let null_like = Module {
        id: 2,
        items_count: Some(1),
        items: None,
        ..Module::default()
    };
    assert!(needs_items_fetch(&null_like));

    let shorter = Module {
        id: 3,
        items_count: Some(3),
        items: Some(vec![ModuleItem {
            id: 10,
            ..ModuleItem::default()
        }]),
        ..Module::default()
    };
    assert!(needs_items_fetch(&shorter));

    let complete = Module {
        id: 4,
        items_count: Some(1),
        items: Some(vec![ModuleItem {
            id: 11,
            ..ModuleItem::default()
        }]),
        ..Module::default()
    };
    assert!(!needs_items_fetch(&complete));

    let empty_ok = Module {
        id: 5,
        items_count: Some(0),
        items: Some(vec![]),
        ..Module::default()
    };
    assert!(!needs_items_fetch(&empty_ok));

    let modules = [absent, null_like, shorter, complete, empty_ok];
    assert_eq!(count_item_fetch_requests(&modules), 3);
}

#[test]
fn denial_classification_order() {
    use canvas_api::Error as Api;
    assert_eq!(
        classify_listing_denial(&Api::Unauthorized),
        DenialAction::Auth
    );
    assert_eq!(
        classify_listing_denial(&Api::RateLimited),
        DenialAction::Throttle
    );
    assert_eq!(
        classify_listing_denial(&Api::Forbidden {
            rate_limited: true,
            body: String::new(),
        }),
        DenialAction::Throttle
    );
    assert_eq!(
        classify_listing_denial(&Api::Forbidden {
            rate_limited: false,
            body: String::new(),
        }),
        DenialAction::RecordDenial { status: 403 }
    );
    assert_eq!(
        classify_listing_denial(&Api::Denied { status: 403 }),
        DenialAction::RecordDenial { status: 403 }
    );
    assert_eq!(
        classify_listing_denial(&Api::NotFound),
        DenialAction::RecordDenial { status: 404 }
    );
    assert_eq!(
        classify_listing_denial(&Api::Denied { status: 404 }),
        DenialAction::RecordDenial { status: 404 }
    );
    assert_eq!(
        classify_listing_denial(&Api::Network),
        DenialAction::Propagate
    );
    assert_eq!(listing_denial_status("unavailable:403"), Some(403));
    assert_eq!(listing_denial_status("unavailable:404"), Some(404));
    assert_eq!(listing_denial_status("other"), None);
}

#[test]
fn hidden_and_locked_remain_separate_in_data_json() {
    let (_dir, open) = setup();
    let file = File {
        id: 9,
        display_name: Some("a.pdf".into()),
        folder_id: Some(1),
        hidden: Some(true),
        locked: Some(false),
        locked_for_user: Some(true),
        ..File::default()
    };
    let entity = file_to_entity(&file, 42);
    let hidden = entity.fields.iter().find(|f| f.name == "hidden").unwrap();
    let locked = entity.fields.iter().find(|f| f.name == "locked").unwrap();
    let locked_for_user = entity
        .fields
        .iter()
        .find(|f| f.name == "locked_for_user")
        .unwrap();
    assert_eq!(hidden.value.as_deref(), Some("true"));
    assert_eq!(locked.value.as_deref(), Some("false"));
    assert_eq!(locked_for_user.value.as_deref(), Some("true"));
    assert_eq!(hidden.group, FieldGroup::Status);
    assert_eq!(locked.group, FieldGroup::Status);

    let ds = FilesDataset::new(42, default_ttl_files());
    open.store
        .call_blocking({
            let page = crate::store::IngestPage {
                fetched_at: ts(100),
                entities: vec![entity],
            };
            let ds = ds.clone();
            move |conns| {
                let epoch = read_scope_epoch(&conns.state, &ds.epoch_scope())?;
                ds.ingest(&[page], &ingest_ok(epoch), conns)
                    .map_err(|e| crate::store::DbError::Message(e.to_string()))?;
                let data: String =
                    conns
                        .cache
                        .query_row("SELECT data_json FROM files WHERE id = 9", [], |r| r.get(0))?;
                let v: serde_json::Value = serde_json::from_str(&data).unwrap();
                assert_eq!(v["hidden"], json!("true"));
                assert_eq!(v["locked"], json!("false"));
                assert_eq!(v["locked_for_user"], json!("true"));
                Ok(())
            }
        })
        .unwrap();
}

#[tokio::test]
async fn offline_serves_recorded_listing_denial() {
    let server = MockServer::start().await;
    let (_dir, open) = setup();
    let ds = FoldersDataset::new(7, Span::new().hours(1));
    open.store
        .call_blocking({
            let ds = ds.clone();
            move |conns| {
                let epoch = read_scope_epoch(&conns.state, &ds.epoch_scope())?;
                ds.ingest(
                    &[crate::store::IngestPage {
                        fetched_at: ts(50),
                        entities: vec![],
                    }],
                    &IngestOpts {
                        epoch_seen: epoch,
                        complete: true,
                        stale: false,
                        error: Some("unavailable:403"),
                        window: None,
                        contexts: None,
                    },
                    conns,
                )
                .map_err(|e| crate::store::DbError::Message(e.to_string()))?;
                Ok(())
            }
        })
        .unwrap();

    let out = refresh_folders(
        &client(&server),
        &open.store,
        7,
        Span::new().hours(1),
        ts(100),
        false,
        true,
    )
    .await
    .unwrap();
    assert_eq!(out.freshness.source, FreshnessSource::Cache);
    assert!(out.freshness.complete);
    assert!(out.freshness.stale);
    assert_eq!(out.error.as_deref(), Some("unavailable:403"));
    assert_eq!(
        listing_denial_status(out.error.as_deref().unwrap()),
        Some(403)
    );

    let hit = open
        .store
        .call_blocking({
            let ds = ds.clone();
            move |conns| lookup_dataset(conns, &ds, ts(60), None)
        })
        .unwrap();
    assert!(matches!(hit, crate::store::LookupResult::Hit(_)));
}

#[tokio::test]
async fn folders_403_records_complete_denial() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/3/folders"))
        .respond_with(ResponseTemplate::new(403).set_body_string("nope"))
        .mount(&server)
        .await;
    let (_dir, open) = setup();
    let out = refresh_folders(
        &client(&server),
        &open.store,
        3,
        Span::new().hours(1),
        ts(100),
        true,
        false,
    )
    .await
    .unwrap();
    assert_eq!(out.freshness.source, FreshnessSource::Network);
    assert!(out.freshness.complete);
    assert!(!out.freshness.stale);
    assert_eq!(out.freshness.count, 0);
    assert_eq!(out.error.as_deref(), Some("unavailable:403"));
}

#[tokio::test]
async fn files_401_propagates_auth() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/courses/3/files"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;
    let (_dir, open) = setup();
    let err = refresh_files(
        &client(&server),
        &open.store,
        3,
        default_ttl_files(),
        ts(100),
        true,
        false,
    )
    .await
    .unwrap_err();
    assert!(matches!(
        err,
        SyncError::Api(canvas_api::Error::Unauthorized)
    ));
}

#[test]
fn discovery_maps_membership_and_root_folder() {
    let (_dir, open) = setup();
    seed_discovery_course(&open);
    open.store
        .call_blocking(|conns| {
            let plan = discovery_plan_input(conns, 5, "CS101")?;
            assert_eq!(plan.folders.len(), 2);
            assert!(plan.folders.iter().any(|f| f.is_root));
            assert_eq!(plan.files.len(), 1);
            assert_eq!(plan.modules.len(), 1);
            assert_eq!(plan.modules[0].items.len(), 1);
            assert_eq!(plan.modules[0].items[0].content_id, Some(50));
            Ok(())
        })
        .unwrap();
    assert!(is_canvas_root(None, Some("anything")));
    assert!(is_canvas_root(Some(9), Some("course files")));
    assert!(!is_canvas_root(Some(9), Some("course files/Slides")));
}

fn seed_discovery_course(open: &OpenIdentity) {
    let folders_ds = FoldersDataset::new(5, default_ttl_files());
    let files_ds = FilesDataset::new(5, default_ttl_files());
    let modules_ds = ModulesDataset::new(5, default_ttl_modules());
    let folder = Folder {
        id: 1,
        name: canvas_api::Supplied::Value("course files".into()),
        full_name: Some("course files".into()),
        parent_folder_id: None,
        ..Folder::default()
    };
    let child = Folder {
        id: 2,
        name: canvas_api::Supplied::Value("Slides".into()),
        full_name: Some("course files/Slides".into()),
        parent_folder_id: Some(1),
        ..Folder::default()
    };
    let file = File {
        id: 50,
        display_name: Some("lec.pdf".into()),
        folder_id: Some(2),
        size: Some(10),
        ..File::default()
    };
    let module = Module {
        id: 8,
        name: canvas_api::Supplied::Value("Week 1".into()),
        position: Some(1),
        state: Some("unlocked".into()),
        items_count: Some(1),
        ..Module::default()
    };
    let item = ModuleItem {
        id: 80,
        title: Some("slides".into()),
        position: Some(1),
        item_type: Some("File".into()),
        content_id: Some(50),
        content_details: Some(canvas_api::models::ModuleItemContentDetails {
            locked_for_user: Some(false),
            lock_explanation: None,
            ..Default::default()
        }),
        ..ModuleItem::default()
    };
    open.store
        .call_blocking({
            let folders_ds = folders_ds.clone();
            let files_ds = files_ds.clone();
            let modules_ds = modules_ds.clone();
            move |conns| {
                let epoch = read_scope_epoch(&conns.state, &folders_ds.epoch_scope())?;
                folders_ds
                    .ingest(
                        &[crate::store::IngestPage {
                            fetched_at: ts(100),
                            entities: vec![
                                super::folder_to_entity(&folder, 5),
                                super::folder_to_entity(&child, 5),
                            ],
                        }],
                        &ingest_ok(epoch),
                        conns,
                    )
                    .map_err(|e| crate::store::DbError::Message(e.to_string()))?;
                let epoch = read_scope_epoch(&conns.state, &files_ds.epoch_scope())?;
                files_ds
                    .ingest(
                        &[crate::store::IngestPage {
                            fetched_at: ts(100),
                            entities: vec![file_to_entity(&file, 5)],
                        }],
                        &ingest_ok(epoch),
                        conns,
                    )
                    .map_err(|e| crate::store::DbError::Message(e.to_string()))?;
                let epoch = read_scope_epoch(&conns.state, &modules_ds.epoch_scope())?;
                modules_ds
                    .ingest(
                        &[crate::store::IngestPage {
                            fetched_at: ts(100),
                            entities: vec![module_to_entity(&module, 5, true, &[item])],
                        }],
                        &ingest_ok(epoch),
                        conns,
                    )
                    .map_err(|e| crate::store::DbError::Message(e.to_string()))?;
                Ok(())
            }
        })
        .unwrap();
}

#[test]
fn default_ttls_are_one_hour() {
    assert_eq!(
        default_ttl_files().fieldwise(),
        Span::new().hours(1).fieldwise()
    );
    assert_eq!(
        default_ttl_modules().fieldwise(),
        Span::new().hours(1).fieldwise()
    );
}

#[test]
fn module_refresh_replaces_item_membership_without_deleting_entities() {
    let (_dir, open) = setup();
    seed_discovery_course(&open);
    open.store
        .call_blocking(|conns| {
            let ds = ModulesDataset::with_default_ttl(5);
            let module = Module {
                id: 8,
                items_count: Some(0),
                ..Module::default()
            };
            ds.ingest(
                &[crate::store::IngestPage {
                    fetched_at: ts(200),
                    entities: vec![module_to_entity(&module, 5, true, &[])],
                }],
                &ingest_ok(0),
                conns,
            )
            .unwrap();
            assert!(
                discovery_plan_input(conns, 5, "CS101")?.modules[0]
                    .items
                    .is_empty()
            );
            let retained: i64 = conns.cache.query_row(
                "SELECT count(*) FROM module_items WHERE id=80",
                [],
                |r| r.get(0),
            )?;
            assert_eq!(retained, 1);
            // A late old response can enrich fields, but cannot resurrect membership.
            let item = ModuleItem {
                id: 80,
                content_id: Some(50),
                ..ModuleItem::default()
            };
            ds.ingest(
                &[crate::store::IngestPage {
                    fetched_at: ts(150),
                    entities: vec![module_to_entity(&module, 5, true, &[item])],
                }],
                &ingest_ok(0),
                conns,
            )
            .unwrap();
            assert!(
                discovery_plan_input(conns, 5, "CS101")?.modules[0]
                    .items
                    .is_empty()
            );
            Ok(())
        })
        .unwrap();
}
