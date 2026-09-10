//! Schema registry constants, M2-b result types, and fixtures.

use serde::{Deserialize, Serialize};

pub const SCHEMA_SUBMIT: &str = "canvas-cli/submit@1";
pub const SCHEMA_SUBMISSION: &str = "canvas-cli/submission@1";
pub const SCHEMA_RECEIPT: &str = "canvas-cli/receipt@1";
pub const SCHEMA_RECEIPTS: &str = "canvas-cli/receipts@1";
pub const SCHEMA_VERIFY: &str = "canvas-cli/verify@1";
pub const SCHEMA_RECONCILE: &str = "canvas-cli/reconcile@1";
pub const SCHEMA_ERROR: &str = "canvas-cli/error@1";
pub const SCHEMA_VERSION: &str = "canvas-cli/version@1";
pub const SCHEMA_CONFIG: &str = "canvas-cli/config@1";
pub const SCHEMA_IDENTITY: &str = "canvas-cli/identity@1";
pub const SCHEMA_DOCTOR: &str = "canvas-cli/doctor@1";

/// One registered schema and its example `result` fixture JSON.
#[derive(Debug, Clone, Copy)]
pub struct SchemaEntry {
    pub id: &'static str,
    pub fixture: &'static str,
}

/// Every registered schema with a fixture payload (M2-b focus + minimal stubs).
#[must_use]
pub fn all_schemas() -> &'static [SchemaEntry] {
    &[
        SchemaEntry {
            id: SCHEMA_SUBMIT,
            fixture: include_str!("schemas/submit.json"),
        },
        SchemaEntry {
            id: SCHEMA_SUBMISSION,
            fixture: include_str!("schemas/submission.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECEIPT,
            fixture: include_str!("schemas/receipt.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECEIPTS,
            fixture: include_str!("schemas/receipts.json"),
        },
        SchemaEntry {
            id: SCHEMA_VERIFY,
            fixture: include_str!("schemas/verify.json"),
        },
        SchemaEntry {
            id: SCHEMA_RECONCILE,
            fixture: include_str!("schemas/reconcile.json"),
        },
        SchemaEntry {
            id: SCHEMA_ERROR,
            fixture: include_str!("schemas/error.json"),
        },
        SchemaEntry {
            id: SCHEMA_VERSION,
            fixture: include_str!("schemas/version.json"),
        },
        SchemaEntry {
            id: SCHEMA_CONFIG,
            fixture: include_str!("schemas/config.json"),
        },
        SchemaEntry {
            id: SCHEMA_IDENTITY,
            fixture: include_str!("schemas/identity.json"),
        },
        SchemaEntry {
            id: SCHEMA_DOCTOR,
            fixture: include_str!("schemas/doctor.json"),
        },
    ]
}

/// Raw-output commands reject `--json` (SPEC §7).
#[must_use]
pub fn rejects_json(command_is_raw: bool) -> bool {
    command_is_raw
}

// --- M2-b typed result payloads (Appendix D) ---

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubmitCandidateJson {
    pub attempt: i64,
    pub submitted_at: Option<String>,
    #[serde(default)]
    pub submitted_at_local: Option<String>,
    pub attachment_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubmitFileJson {
    pub name: String,
    pub size: u64,
    pub sha256: String,
    #[serde(default)]
    pub canvas_file_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubmitTextJson {
    pub input_sha256: String,
    pub transform: String,
    pub sent_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubmitResult {
    pub outcome: String,
    pub state: String,
    pub journal_id: String,
    #[serde(default)]
    pub receipt_id: Option<String>,
    #[serde(default)]
    pub attribution: Option<String>,
    #[serde(default)]
    pub post_status: Option<i64>,
    #[serde(default)]
    pub response_kind: Option<String>,
    #[serde(default)]
    pub posted: Option<serde_json::Value>,
    #[serde(default)]
    pub server_match: Option<SubmitCandidateJson>,
    pub candidates: Vec<SubmitCandidateJson>,
    pub files: Vec<SubmitFileJson>,
    #[serde(default)]
    pub text: Option<SubmitTextJson>,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub error: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::output::envelope::{Envelope, IdentityRef, Outcome};

    #[test]
    fn every_registered_schema_has_a_parseable_fixture() {
        use crate::output::now::with_canvas_now;
        with_canvas_now("2026-09-09T17:05:12Z", || {
            let mut ids = std::collections::HashSet::new();
            for entry in all_schemas() {
                assert!(ids.insert(entry.id), "duplicate schema {}", entry.id);
                let result: serde_json::Value =
                    serde_json::from_str(entry.fixture).unwrap_or_else(|e| {
                        panic!("fixture for {} is not JSON: {e}", entry.id);
                    });
                let local = matches!(
                    entry.id,
                    SCHEMA_CONFIG | SCHEMA_IDENTITY | SCHEMA_VERSION | SCHEMA_DOCTOR | SCHEMA_ERROR
                );
                let mut env = Envelope::new(
                    entry.id,
                    if local { None } else { Some("default".into()) },
                    if local {
                        None
                    } else {
                        Some(IdentityRef {
                            origin: "https://example.instructure.com".into(),
                            user_id: "1".into(),
                            key: "example.instructure.com-1-deadbeef".into(),
                        })
                    },
                )
                .with_result(result);
                if entry.id == SCHEMA_ERROR {
                    env.outcome = Outcome::Error;
                    env.exit = 3;
                }
                let mut buf = Vec::new();
                env.write_json(&mut buf).unwrap();
                let parsed: serde_json::Value = serde_json::from_slice(&buf).unwrap();
                assert_eq!(parsed["schema"], entry.id);
                assert!(parsed.get("result").is_some(), "{}", entry.id);
            }
        });
    }

    #[test]
    fn m2b_fixtures_deserialize_to_typed_results() {
        let submit: SubmitResult =
            serde_json::from_str(include_str!("schemas/submit.json")).unwrap();
        assert_eq!(submit.state, "submitted");
        assert_eq!(submit.files[0].name, "essay.pdf");

        let verify: serde_json::Value =
            serde_json::from_str(include_str!("schemas/verify.json")).unwrap();
        assert_eq!(verify["outcome"], "verified");

        let reconcile: serde_json::Value =
            serde_json::from_str(include_str!("schemas/reconcile.json")).unwrap();
        assert_eq!(reconcile["outcome"], "ok");
    }

    #[test]
    fn rejects_json_documents_raw_commands() {
        assert!(rejects_json(true));
        assert!(!rejects_json(false));
    }
}
