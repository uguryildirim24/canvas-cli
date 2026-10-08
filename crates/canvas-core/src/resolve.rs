//! Course and assignment identifier resolution (SPEC §6).

use jiff::Timestamp;
use rusqlite::{Connection, OptionalExtension, params};

use crate::store::{DbError, StoreConns, load_fetch_log};

/// Command class that drives fetch-or-refuse behaviour for incomplete datasets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandClass {
    /// Identity-bound, local: never fetch.
    B,
    /// Identity-bound, cache-backed read: caller may fetch and retry.
    C,
    /// Network-required: caller may fetch and retry.
    D,
}

/// A course resolved from an identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedCourse {
    pub id: i64,
    pub code: Option<String>,
    pub name: Option<String>,
}

/// One candidate shown on ambiguous or empty course resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CourseCandidate {
    pub id: i64,
    pub code: Option<String>,
    pub name: Option<String>,
}

/// An assignment resolved from an identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAssignment {
    pub id: i64,
    pub course_id: i64,
    pub name: Option<String>,
}

/// One candidate shown on ambiguous or empty assignment resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignmentCandidate {
    pub id: i64,
    pub course_id: i64,
    pub name: Option<String>,
}

/// A quiz resolved from an identifier.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedQuiz {
    pub id: i64,
    pub course_id: i64,
    pub title: Option<String>,
}

/// One candidate shown on ambiguous or empty quiz resolution.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuizCandidate {
    pub id: i64,
    pub course_id: i64,
    pub title: Option<String>,
}

/// One row from the durable `alias` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasRow {
    pub name: String,
    pub target_kind: String,
    pub target_id: String,
    pub created_at: String,
}

/// Resolution failures (exit 6 unless noted by the caller).
#[derive(Debug, thiserror::Error)]
pub enum ResolveError {
    #[error("not found")]
    NotFound { candidates: Vec<CourseCandidate> },
    #[error("ambiguous")]
    Ambiguous { candidates: Vec<CourseCandidate> },
    #[error("assignment not found")]
    AssignmentNotFound {
        candidates: Vec<AssignmentCandidate>,
    },
    #[error("assignment ambiguous")]
    AssignmentAmbiguous {
        candidates: Vec<AssignmentCandidate>,
    },
    #[error("quiz not found")]
    QuizNotFound { candidates: Vec<QuizCandidate> },
    #[error("quiz ambiguous")]
    QuizAmbiguous { candidates: Vec<QuizCandidate> },
    #[error("origin mismatch")]
    OriginMismatch,
    #[error("course id mismatch: url {url_course}, arg {arg_course}")]
    CourseIdMismatch { url_course: i64, arg_course: i64 },
    #[error("use a numeric ID or a URL")]
    NeedIdOrUrl,
    #[error("incomplete dataset {dataset}:{scope}")]
    IncompleteDataset { dataset: String, scope: String },
    #[error(transparent)]
    Db(#[from] DbError),
}

impl From<rusqlite::Error> for ResolveError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Db(DbError::Sqlite(value))
    }
}

/// Resolve `<course>` per SPEC §6.
pub fn resolve_course(
    conns: &StoreConns,
    input: &str,
    identity_origin: &str,
    class: CommandClass,
) -> Result<ResolvedCourse, ResolveError> {
    if let Ok(id) = input.parse::<i64>() {
        return Ok(load_course(conns, id)?.unwrap_or(ResolvedCourse {
            id,
            code: None,
            name: None,
        }));
    }

    if let Some(id) = alias_course_id(&conns.state, input)? {
        return Ok(load_course(conns, id)?.unwrap_or(ResolvedCourse {
            id,
            code: None,
            name: None,
        }));
    }

    if let Some(id) = parse_course_url(input, identity_origin)? {
        return Ok(load_course(conns, id)?.unwrap_or(ResolvedCourse {
            id,
            code: None,
            name: None,
        }));
    }

    if input.contains("://") {
        return Err(ResolveError::NotFound { candidates: vec![] });
    }
    resolve_course_substring(conns, input, class)
}

/// Resolve `<assignment>` for a known course.
pub fn resolve_assignment(
    conns: &StoreConns,
    course_id: i64,
    input: &str,
    identity_origin: &str,
    class: CommandClass,
) -> Result<ResolvedAssignment, ResolveError> {
    if let Ok(id) = input.parse::<i64>() {
        return Ok(
            load_assignment(conns, id, course_id)?.unwrap_or(ResolvedAssignment {
                id,
                course_id,
                name: None,
            }),
        );
    }

    if let Some((url_course, assignment_id)) = parse_assignment_url(input, identity_origin)? {
        if url_course != course_id {
            return Err(ResolveError::CourseIdMismatch {
                url_course,
                arg_course: course_id,
            });
        }
        return Ok(load_assignment(conns, assignment_id, course_id)?.unwrap_or(
            ResolvedAssignment {
                id: assignment_id,
                course_id,
                name: None,
            },
        ));
    }

    if input.contains("://") {
        return Err(ResolveError::AssignmentNotFound { candidates: vec![] });
    }
    require_complete(conns, "assignments", &format!("course:{course_id}"), class)?;
    let rows = membership_assignments(conns, course_id)?;
    let needle = input.to_lowercase();
    let matches: Vec<AssignmentCandidate> = rows
        .into_iter()
        .filter(|a| {
            a.name
                .as_ref()
                .is_some_and(|n| n.to_lowercase().contains(&needle))
        })
        .collect();
    match matches.len() {
        1 => {
            let a = &matches[0];
            Ok(ResolvedAssignment {
                id: a.id,
                course_id: a.course_id,
                name: a.name.clone(),
            })
        }
        0 => Err(ResolveError::AssignmentNotFound {
            candidates: matches,
        }),
        _ => Err(ResolveError::AssignmentAmbiguous {
            candidates: matches,
        }),
    }
}

/// Resolve `<quiz>` for a known course.
///
/// A numeric id, a `/courses/:cid/quizzes/:qid` URL, or a title substring of
/// the cached `quizzes` listing. A title only resolves against a complete
/// listing, exactly as an assignment name resolves against its own.
pub fn resolve_quiz(
    conns: &StoreConns,
    course_id: i64,
    input: &str,
    identity_origin: &str,
    class: CommandClass,
) -> Result<ResolvedQuiz, ResolveError> {
    if let Ok(id) = input.parse::<i64>() {
        return Ok(load_quiz(conns, id, course_id)?.unwrap_or(ResolvedQuiz {
            id,
            course_id,
            title: None,
        }));
    }

    if let Some((url_course, quiz_id)) = parse_quiz_url(input, identity_origin)? {
        if url_course != course_id {
            return Err(ResolveError::CourseIdMismatch {
                url_course,
                arg_course: course_id,
            });
        }
        return Ok(
            load_quiz(conns, quiz_id, course_id)?.unwrap_or(ResolvedQuiz {
                id: quiz_id,
                course_id,
                title: None,
            }),
        );
    }

    if input.contains("://") {
        return Err(ResolveError::QuizNotFound { candidates: vec![] });
    }
    require_complete(conns, "quizzes", &format!("course:{course_id}"), class)?;
    let rows = membership_quizzes(conns, course_id)?;
    let needle = input.to_lowercase();
    let matches: Vec<QuizCandidate> = rows
        .into_iter()
        .filter(|q| {
            q.title
                .as_ref()
                .is_some_and(|t| t.to_lowercase().contains(&needle))
        })
        .collect();
    match matches.len() {
        1 => {
            let q = &matches[0];
            Ok(ResolvedQuiz {
                id: q.id,
                course_id: q.course_id,
                title: q.title.clone(),
            })
        }
        0 => Err(ResolveError::QuizNotFound {
            candidates: matches,
        }),
        _ => Err(ResolveError::QuizAmbiguous {
            candidates: matches,
        }),
    }
}

/// One row of a course's cached `quizzes` listing.
fn membership_quizzes(
    conns: &StoreConns,
    course_id: i64,
) -> Result<Vec<QuizCandidate>, ResolveError> {
    let scope = format!("course:{course_id}");
    let mut stmt = conns.cache.prepare(
        "SELECT q.id, q.course_id, q.title
         FROM membership m
         INNER JOIN quizzes q ON q.id = CAST(m.entity_id AS INTEGER)
         WHERE m.dataset = 'quizzes' AND m.scope = ?1 AND m.entity_kind = 'quiz'
         ORDER BY m.position ASC, q.id ASC",
    )?;
    let rows = stmt
        .query_map(params![scope], |r| {
            Ok(QuizCandidate {
                id: r.get(0)?,
                course_id: r.get::<_, Option<i64>>(1)?.unwrap_or(course_id),
                title: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// One quiz row, when the cache holds it.
fn load_quiz(
    conns: &StoreConns,
    id: i64,
    course_id: i64,
) -> Result<Option<ResolvedQuiz>, ResolveError> {
    let row: Option<ResolvedQuiz> = conns
        .cache
        .query_row(
            "SELECT id, course_id, title FROM quizzes WHERE id = ?1",
            params![id],
            |r| {
                Ok(ResolvedQuiz {
                    id: r.get(0)?,
                    course_id: r.get::<_, Option<i64>>(1)?.unwrap_or(course_id),
                    title: r.get(2)?,
                })
            },
        )
        .optional()?;
    if let Some(ref quiz) = row
        && quiz.course_id != course_id
    {
        return Err(ResolveError::CourseIdMismatch {
            url_course: quiz.course_id,
            arg_course: course_id,
        });
    }
    Ok(row)
}

/// Split a `/courses/:cid/quizzes/:qid` URL, when the input is one on this origin.
fn parse_quiz_url(input: &str, identity_origin: &str) -> Result<Option<(i64, i64)>, ResolveError> {
    let Some(url) = canvas_url(input, identity_origin)? else {
        return Ok(None);
    };
    let parts: Vec<_> = url.path_segments().into_iter().flatten().collect();
    Ok(parts
        .windows(4)
        .find(|p| p[0] == "courses" && p[2] == "quizzes")
        .and_then(|p| Some((p[1].parse().ok()?, p[3].parse().ok()?))))
}

/// Insert or replace a course alias for the active identity.
pub fn alias_set(state: &Connection, name: &str, course_id: i64) -> Result<(), ResolveError> {
    let created_at = Timestamp::now().to_string();
    let tx = rusqlite::Transaction::new_unchecked(state, rusqlite::TransactionBehavior::Immediate)?;
    tx.execute(
        "INSERT INTO alias (name, target_kind, target_id, created_at)
         VALUES (?1, 'course', ?2, ?3)
         ON CONFLICT(name) DO UPDATE SET
            target_kind = excluded.target_kind,
            target_id = excluded.target_id,
            created_at = excluded.created_at",
        params![name, course_id.to_string(), created_at],
    )?;
    tx.commit()?;
    Ok(())
}

/// List all aliases for the active identity.
pub fn alias_list(state: &Connection) -> Result<Vec<AliasRow>, ResolveError> {
    let mut stmt = state
        .prepare("SELECT name, target_kind, target_id, created_at FROM alias ORDER BY name ASC")?;
    let rows = stmt
        .query_map([], |r| {
            Ok(AliasRow {
                name: r.get(0)?,
                target_kind: r.get(1)?,
                target_id: r.get(2)?,
                created_at: r.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Remove an alias by name. Returns `true` when a row was deleted.
pub fn alias_remove(state: &Connection, name: &str) -> Result<bool, ResolveError> {
    let tx = rusqlite::Transaction::new_unchecked(state, rusqlite::TransactionBehavior::Immediate)?;
    let n = tx.execute("DELETE FROM alias WHERE name = ?1", params![name])?;
    tx.commit()?;
    Ok(n > 0)
}

fn alias_course_id(state: &Connection, name: &str) -> Result<Option<i64>, ResolveError> {
    let row: Option<(String, String)> = state
        .query_row(
            "SELECT target_kind, target_id FROM alias WHERE name = ?1",
            params![name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((kind, id)) = row else {
        return Ok(None);
    };
    if kind != "course" {
        return Ok(None);
    }
    let id: i64 = id.parse().map_err(|_| {
        ResolveError::Db(DbError::Message(format!(
            "alias {name:?} has non-numeric course id"
        )))
    })?;
    Ok(Some(id))
}

fn resolve_course_substring(
    conns: &StoreConns,
    input: &str,
    class: CommandClass,
) -> Result<ResolvedCourse, ResolveError> {
    require_complete(conns, "courses", "active", class)?;
    let active = membership_courses(conns, "active")?;
    match select_course_match(input, &active) {
        MatchOutcome::Unique(c) => return Ok(c),
        MatchOutcome::Ambiguous(cands) => {
            return Err(ResolveError::Ambiguous { candidates: cands });
        }
        MatchOutcome::None => {}
    }

    require_complete(conns, "courses", "all", class)?;
    let all = membership_courses(conns, "all")?;
    let mut by_id = std::collections::BTreeMap::new();
    for c in active.into_iter().chain(all) {
        by_id.entry(c.id).or_insert(c);
    }
    let union: Vec<_> = by_id.into_values().collect();
    match select_course_match(input, &union) {
        MatchOutcome::Unique(c) => Ok(c),
        MatchOutcome::None => Err(ResolveError::NotFound { candidates: union }),
        MatchOutcome::Ambiguous(cands) => Err(ResolveError::Ambiguous { candidates: cands }),
    }
}

enum MatchOutcome {
    Unique(ResolvedCourse),
    None,
    Ambiguous(Vec<CourseCandidate>),
}

fn select_course_match(input: &str, courses: &[CourseCandidate]) -> MatchOutcome {
    let needle = input.to_lowercase();
    let code_hits: Vec<&CourseCandidate> = courses
        .iter()
        .filter(|c| {
            c.code
                .as_ref()
                .is_some_and(|code| code.to_lowercase().contains(&needle))
        })
        .collect();
    if code_hits.len() == 1 {
        return MatchOutcome::Unique(to_resolved(code_hits[0]));
    }
    if code_hits.len() > 1 {
        return MatchOutcome::Ambiguous(code_hits.into_iter().cloned().collect());
    }

    let name_hits: Vec<&CourseCandidate> = courses
        .iter()
        .filter(|c| {
            c.name
                .as_ref()
                .is_some_and(|name| name.to_lowercase().contains(&needle))
        })
        .collect();
    match name_hits.len() {
        0 => MatchOutcome::None,
        1 => MatchOutcome::Unique(to_resolved(name_hits[0])),
        _ => MatchOutcome::Ambiguous(name_hits.into_iter().cloned().collect()),
    }
}

fn to_resolved(c: &CourseCandidate) -> ResolvedCourse {
    ResolvedCourse {
        id: c.id,
        code: c.code.clone(),
        name: c.name.clone(),
    }
}

fn require_complete(
    conns: &StoreConns,
    dataset: &str,
    scope: &str,
    class: CommandClass,
) -> Result<(), ResolveError> {
    let complete = load_fetch_log(&conns.cache, dataset, scope)?.is_some_and(|r| r.complete);
    if complete {
        return Ok(());
    }
    match class {
        CommandClass::B => Err(ResolveError::NeedIdOrUrl),
        CommandClass::C | CommandClass::D => Err(ResolveError::IncompleteDataset {
            dataset: dataset.to_string(),
            scope: scope.to_string(),
        }),
    }
}

fn membership_courses(
    conns: &StoreConns,
    scope: &str,
) -> Result<Vec<CourseCandidate>, ResolveError> {
    let mut stmt = conns.cache.prepare(
        "SELECT c.id, c.course_code, c.name
         FROM membership m
         INNER JOIN courses c ON c.id = CAST(m.entity_id AS INTEGER)
         WHERE m.dataset = 'courses' AND m.scope = ?1 AND m.entity_kind = 'course'
         ORDER BY m.position ASC, c.id ASC",
    )?;
    let rows = stmt
        .query_map(params![scope], |r| {
            Ok(CourseCandidate {
                id: r.get(0)?,
                code: r.get(1)?,
                name: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn membership_assignments(
    conns: &StoreConns,
    course_id: i64,
) -> Result<Vec<AssignmentCandidate>, ResolveError> {
    let scope = format!("course:{course_id}");
    let mut stmt = conns.cache.prepare(
        "SELECT a.id, a.course_id, a.name
         FROM membership m
         INNER JOIN assignments a ON a.id = CAST(m.entity_id AS INTEGER)
         WHERE m.dataset = 'assignments' AND m.scope = ?1 AND m.entity_kind = 'assignment'
         ORDER BY m.position ASC, a.id ASC",
    )?;
    let rows = stmt
        .query_map(params![scope], |r| {
            Ok(AssignmentCandidate {
                id: r.get(0)?,
                course_id: r.get::<_, Option<i64>>(1)?.unwrap_or(course_id),
                name: r.get(2)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

fn load_course(conns: &StoreConns, id: i64) -> Result<Option<ResolvedCourse>, ResolveError> {
    let row = conns
        .cache
        .query_row(
            "SELECT id, course_code, name FROM courses WHERE id = ?1",
            params![id],
            |r| {
                Ok(ResolvedCourse {
                    id: r.get(0)?,
                    code: r.get(1)?,
                    name: r.get(2)?,
                })
            },
        )
        .optional()?;
    Ok(row)
}

fn load_assignment(
    conns: &StoreConns,
    id: i64,
    course_id: i64,
) -> Result<Option<ResolvedAssignment>, ResolveError> {
    let row: Option<ResolvedAssignment> = conns
        .cache
        .query_row(
            "SELECT id, course_id, name FROM assignments WHERE id = ?1",
            params![id],
            |r| {
                Ok(ResolvedAssignment {
                    id: r.get(0)?,
                    course_id: r.get::<_, Option<i64>>(1)?.unwrap_or(course_id),
                    name: r.get(2)?,
                })
            },
        )
        .optional()?;
    if let Some(ref assignment) = row
        && assignment.course_id != course_id
    {
        return Err(ResolveError::CourseIdMismatch {
            url_course: assignment.course_id,
            arg_course: course_id,
        });
    }
    Ok(row)
}

/// Parse and validate a browser URL against the identity origin, rejecting userinfo.
pub fn canvas_url(
    input: &str,
    identity_origin: &str,
) -> Result<Option<reqwest::Url>, ResolveError> {
    let Ok(url) = reqwest::Url::parse(input) else {
        return Ok(None);
    };
    if !matches!(url.scheme(), "http" | "https") {
        return Ok(None);
    }
    let identity =
        reqwest::Url::parse(identity_origin).map_err(|_| ResolveError::OriginMismatch)?;
    if url.origin() != identity.origin() || !url.username().is_empty() || url.password().is_some() {
        return Err(ResolveError::OriginMismatch);
    }
    Ok(Some(url))
}

fn parse_course_url(input: &str, identity_origin: &str) -> Result<Option<i64>, ResolveError> {
    let Some(url) = canvas_url(input, identity_origin)? else {
        return Ok(None);
    };
    let parts: Vec<_> = url.path_segments().into_iter().flatten().collect();
    Ok(parts
        .windows(2)
        .find(|p| p[0] == "courses")
        .and_then(|p| p[1].parse().ok()))
}

fn parse_assignment_url(
    input: &str,
    identity_origin: &str,
) -> Result<Option<(i64, i64)>, ResolveError> {
    let Some(url) = canvas_url(input, identity_origin)? else {
        return Ok(None);
    };
    let parts: Vec<_> = url.path_segments().into_iter().flatten().collect();
    Ok(parts
        .windows(4)
        .find(|p| p[0] == "courses" && p[2] == "assignments")
        .and_then(|p| Some((p[1].parse().ok()?, p[3].parse().ok()?))))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    use tempfile::TempDir;

    use crate::identity::{IdentityDocument, Paths};
    use crate::store::OpenIdentity;

    const ORIGIN: &str = "https://canvas.example.test";

    fn setup() -> (TempDir, OpenIdentity) {
        let dir = TempDir::new().unwrap();
        let doc = IdentityDocument::new(ORIGIN, 62001, "2026-01-01T00:00:00Z");
        let paths = Paths::for_identity(dir.path(), &doc.key);
        fs::create_dir_all(&paths.identity_dir).unwrap();
        fs::create_dir_all(paths.lock_path.parent().unwrap()).unwrap();
        doc.write(&paths.identity_json()).unwrap();
        let open = OpenIdentity::open(&paths, &doc).unwrap();
        (dir, open)
    }

    fn seed_course(conns: &mut StoreConns, id: i64, code: &str, name: &str) {
        conns
            .cache
            .execute(
                "INSERT INTO courses (id, course_code, name, data_json) VALUES (?1, ?2, ?3, '{}')",
                params![id, code, name],
            )
            .unwrap();
    }

    fn mark_complete(conns: &mut StoreConns, dataset: &str, scope: &str, complete: bool) {
        conns
            .cache
            .execute(
                "INSERT INTO fetch_log (
                    dataset, scope, fetched_at, complete, count, stale, error, epoch_seen
                 ) VALUES (?1, ?2, '2026-01-01T00:00:00Z', ?3, 0, 0, NULL, 0)
                 ON CONFLICT(dataset, scope) DO UPDATE SET complete = excluded.complete",
                params![dataset, scope, i64::from(complete)],
            )
            .unwrap();
    }

    fn add_membership(
        conns: &mut StoreConns,
        dataset: &str,
        scope: &str,
        kind: &str,
        id: i64,
        position: i64,
    ) {
        conns
            .cache
            .execute(
                "INSERT INTO membership (dataset, scope, entity_kind, entity_id, position)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![dataset, scope, kind, id.to_string(), position],
            )
            .unwrap();
    }

    #[test]
    fn numeric_id_resolves_without_membership() {
        let (_dir, open) = setup();
        open.store
            .call_blocking(|conns| {
                seed_course(conns, 42, "CS101", "Intro");
                let got = resolve_course(conns, "42", ORIGIN, CommandClass::B).unwrap();
                assert_eq!(got.id, 42);
                assert_eq!(got.code.as_deref(), Some("CS101"));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn url_resolves_with_matching_origin() {
        let (_dir, open) = setup();
        open.store
            .call_blocking(|conns| {
                let got = resolve_course(
                    conns,
                    "https://canvas.example.test/courses/99",
                    ORIGIN,
                    CommandClass::B,
                )
                .unwrap();
                assert_eq!(got.id, 99);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn url_origin_mismatch() {
        let (_dir, open) = setup();
        open.store
            .call_blocking(|conns| {
                let err = resolve_course(
                    conns,
                    "https://other.instructure.com/courses/99",
                    ORIGIN,
                    CommandClass::B,
                )
                .unwrap_err();
                assert!(matches!(err, ResolveError::OriginMismatch));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn alias_resolves_course() {
        let (_dir, open) = setup();
        open.store
            .call_blocking(|conns| {
                seed_course(conns, 7, "CHEM", "Chemistry");
                alias_set(&conns.state, "chem", 7).unwrap();
                let got = resolve_course(conns, "chem", ORIGIN, CommandClass::B).unwrap();
                assert_eq!(got.id, 7);
                assert_eq!(got.name.as_deref(), Some("Chemistry"));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn alias_helpers_list_and_remove() {
        let (_dir, open) = setup();
        open.store
            .call_blocking(|conns| {
                alias_set(&conns.state, "a", 1).unwrap();
                alias_set(&conns.state, "b", 2).unwrap();
                let list = alias_list(&conns.state).unwrap();
                assert_eq!(list.len(), 2);
                assert_eq!(list[0].name, "a");
                assert!(alias_remove(&conns.state, "a").unwrap());
                assert!(!alias_remove(&conns.state, "a").unwrap());
                assert_eq!(alias_list(&conns.state).unwrap().len(), 1);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn substring_ambiguity_over_active() {
        let (_dir, open) = setup();
        open.store
            .call_blocking(|conns| {
                seed_course(conns, 1, "CS101", "Intro CS");
                seed_course(conns, 2, "CS102", "Intro CS Lab");
                add_membership(conns, "courses", "active", "course", 1, 0);
                add_membership(conns, "courses", "active", "course", 2, 1);
                mark_complete(conns, "courses", "active", true);

                let err = resolve_course(conns, "intro", ORIGIN, CommandClass::C).unwrap_err();
                match err {
                    ResolveError::Ambiguous { candidates } => {
                        assert_eq!(candidates.len(), 2);
                    }
                    other => panic!("expected Ambiguous, got {other:?}"),
                }
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn incompleteness_class_c_and_b() {
        let (_dir, open) = setup();
        open.store
            .call_blocking(|conns| {
                seed_course(conns, 1, "CS101", "Intro");
                add_membership(conns, "courses", "active", "course", 1, 0);
                // No fetch_log row → absent.

                let err_c = resolve_course(conns, "intro", ORIGIN, CommandClass::C).unwrap_err();
                match err_c {
                    ResolveError::IncompleteDataset { dataset, scope } => {
                        assert_eq!(dataset, "courses");
                        assert_eq!(scope, "active");
                    }
                    other => panic!("expected IncompleteDataset, got {other:?}"),
                }

                let err_b = resolve_course(conns, "intro", ORIGIN, CommandClass::B).unwrap_err();
                assert!(matches!(err_b, ResolveError::NeedIdOrUrl));
                assert_eq!(err_b.to_string(), "use a numeric ID or a URL");
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn incomplete_fetch_log_row_same_as_absent() {
        let (_dir, open) = setup();
        open.store
            .call_blocking(|conns| {
                mark_complete(conns, "courses", "active", false);
                let err = resolve_course(conns, "x", ORIGIN, CommandClass::D).unwrap_err();
                assert!(matches!(
                    err,
                    ResolveError::IncompleteDataset {
                        ref dataset,
                        ref scope
                    } if dataset == "courses" && scope == "active"
                ));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn unique_substring_on_active() {
        let (_dir, open) = setup();
        open.store
            .call_blocking(|conns| {
                seed_course(conns, 1, "CS101", "Intro");
                seed_course(conns, 2, "MATH", "Calculus");
                add_membership(conns, "courses", "active", "course", 1, 0);
                add_membership(conns, "courses", "active", "course", 2, 1);
                mark_complete(conns, "courses", "active", true);

                let got = resolve_course(conns, "cs101", ORIGIN, CommandClass::C).unwrap();
                assert_eq!(got.id, 1);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn fallback_to_union_when_active_misses() {
        let (_dir, open) = setup();
        open.store
            .call_blocking(|conns| {
                seed_course(conns, 1, "CS101", "Intro");
                seed_course(conns, 9, "HIST", "World History");
                add_membership(conns, "courses", "active", "course", 1, 0);
                add_membership(conns, "courses", "all", "course", 1, 0);
                add_membership(conns, "courses", "all", "course", 9, 1);
                mark_complete(conns, "courses", "active", true);
                mark_complete(conns, "courses", "all", true);

                let got = resolve_course(conns, "history", ORIGIN, CommandClass::C).unwrap();
                assert_eq!(got.id, 9);
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn never_matches_unreferenced_entity_rows() {
        let (_dir, open) = setup();
        open.store
            .call_blocking(|conns| {
                seed_course(conns, 1, "CS101", "Intro");
                seed_course(conns, 99, "ORPHAN", "Orphan Course");
                add_membership(conns, "courses", "active", "course", 1, 0);
                mark_complete(conns, "courses", "active", true);
                mark_complete(conns, "courses", "all", true);

                let err = resolve_course(conns, "orphan", ORIGIN, CommandClass::C).unwrap_err();
                assert!(matches!(err, ResolveError::NotFound { .. }));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn assignment_numeric_url_and_substring() {
        let (_dir, open) = setup();
        open.store
            .call_blocking(|conns| {
                conns
                    .cache
                    .execute(
                        "INSERT INTO assignments (id, course_id, name, data_json)
                         VALUES (10, 5, 'Synthetic Exercise One', '{}')",
                        [],
                    )
                    .unwrap();
                conns
                    .cache
                    .execute(
                        "INSERT INTO assignments (id, course_id, name, data_json)
                         VALUES (11, 5, 'Synthetic Exercise Two', '{}')",
                        [],
                    )
                    .unwrap();
                add_membership(conns, "assignments", "course:5", "assignment", 10, 0);
                add_membership(conns, "assignments", "course:5", "assignment", 11, 1);
                mark_complete(conns, "assignments", "course:5", true);

                let by_id = resolve_assignment(conns, 5, "10", ORIGIN, CommandClass::B).unwrap();
                assert_eq!(by_id.id, 10);

                let by_url = resolve_assignment(
                    conns,
                    5,
                    "https://canvas.example.test/courses/5/assignments/11",
                    ORIGIN,
                    CommandClass::B,
                )
                .unwrap();
                assert_eq!(by_url.id, 11);

                let by_name =
                    resolve_assignment(conns, 5, "exercise one", ORIGIN, CommandClass::C).unwrap();
                assert_eq!(by_name.id, 10);

                let mismatch = resolve_assignment(
                    conns,
                    5,
                    "https://canvas.example.test/courses/9/assignments/11",
                    ORIGIN,
                    CommandClass::B,
                )
                .unwrap_err();
                assert!(matches!(
                    mismatch,
                    ResolveError::CourseIdMismatch {
                        url_course: 9,
                        arg_course: 5
                    }
                ));
                Ok(())
            })
            .unwrap();
    }

    #[test]
    fn assignment_incomplete_class_b() {
        let (_dir, open) = setup();
        open.store
            .call_blocking(|conns| {
                let err =
                    resolve_assignment(conns, 5, "homework", ORIGIN, CommandClass::B).unwrap_err();
                assert!(matches!(err, ResolveError::NeedIdOrUrl));
                Ok(())
            })
            .unwrap();
    }
    #[test]
    fn canonical_urls_and_assignment_course_binding() {
        assert_eq!(
            parse_course_url("HTTPS://CANVAS.EXAMPLE.TEST:443/courses/42?x=1", ORIGIN).unwrap(),
            Some(42)
        );
        assert!(matches!(
            parse_course_url("https://user@canvas.example.test/courses/42", ORIGIN),
            Err(ResolveError::OriginMismatch)
        ));
        assert_eq!(
            parse_assignment_url(
                "https://canvas.example.test/courses/1/files/2/assignments/3",
                ORIGIN
            )
            .unwrap(),
            None
        );
        let (_dir, open) = setup();
        open.store
            .call_blocking(|conns| {
                conns
                    .cache
                    .execute("INSERT INTO assignments (id, course_id) VALUES (10, 9)", [])?;
                for input in ["10", "https://canvas.example.test/courses/5/assignments/10"] {
                    assert!(matches!(
                        resolve_assignment(conns, 5, input, ORIGIN, CommandClass::B),
                        Err(ResolveError::CourseIdMismatch {
                            url_course: 9,
                            arg_course: 5
                        })
                    ));
                }
                Ok(())
            })
            .unwrap();
    }
    #[test]
    fn malformed_target_urls_never_request_name_datasets() {
        let (_dir, open) = setup();
        open.store
            .call_blocking(|conns| {
                for class in [CommandClass::B, CommandClass::C, CommandClass::D] {
                    assert!(matches!(
                        resolve_assignment(
                            conns,
                            1,
                            "https://canvas.example.test/courses/1/files/2",
                            ORIGIN,
                            class
                        ),
                        Err(ResolveError::AssignmentNotFound { .. })
                    ));
                    assert!(matches!(
                        resolve_course(conns, "https://canvas.example.test/files/2", ORIGIN, class),
                        Err(ResolveError::NotFound { .. })
                    ));
                }
                Ok(())
            })
            .unwrap();
    }
}
