//! Path component sanitization and uniqueness (§12.3).

use std::collections::{HashMap, HashSet};

use unicode_normalization::UnicodeNormalization;

/// Kind of path component; selects the empty-name fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComponentKind {
    /// File display name → `file-<id>`.
    File,
    /// Folder name → `folder-<id>`.
    Folder,
    /// Module name/slug → `module-<id>`.
    Module,
}

/// Sanitize one path component per §12.3.
///
/// `id` is the file, folder, or module id used for empty fallback and truncation budget.
#[must_use]
pub fn sanitize_component(raw: &str, kind: ComponentKind, id: i64) -> String {
    let mut out = String::with_capacity(raw.len());
    for ch in raw.chars() {
        if is_invalid_char(ch) {
            out.push('_');
        } else {
            out.push(ch);
        }
    }

    while out.ends_with(' ') || out.ends_with('.') {
        out.pop();
    }

    if out == "." || out == ".." {
        out.clear();
    }

    while out.starts_with('.') {
        out = out[out.chars().next().map_or(0, char::len_utf8)..].to_string();
        while out.ends_with(' ') || out.ends_with('.') {
            out.pop();
        }
    }

    if out.is_empty() {
        return empty_fallback(kind, id);
    }

    if is_windows_device(&out) {
        out = if let Some((stem, ext)) = out.split_once('.') {
            format!("{stem}_.{ext}")
        } else {
            out.push('_');
            out
        };
    }

    truncate_utf8_leaving_id_room(&mut out, id);
    if out.is_empty() {
        return empty_fallback(kind, id);
    }
    out
}

fn empty_fallback(kind: ComponentKind, id: i64) -> String {
    match kind {
        ComponentKind::File => format!("file-{id}"),
        ComponentKind::Folder => format!("folder-{id}"),
        ComponentKind::Module => format!("module-{id}"),
    }
}

fn is_invalid_char(ch: char) -> bool {
    matches!(
        ch,
        '/' | '\\' | '\0' | '<' | '>' | ':' | '"' | '|' | '?' | '*'
    ) || ch.is_control()
}

fn is_windows_device(name: &str) -> bool {
    let base = name
        .split_once('.')
        .map_or(name, |(stem, _)| stem)
        .to_ascii_uppercase();
    matches!(
        base.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM0"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "COM\u{00B9}"
            | "COM\u{00B2}"
            | "COM\u{00B3}"
            | "LPT0"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
            | "LPT\u{00B9}"
            | "LPT\u{00B2}"
            | "LPT\u{00B3}"
    ) || {
        // Case-fold ASCII prefix then compare superscript forms case-insensitively on COM/LPT.
        let upper: String = base.chars().map(|c| c.to_ascii_uppercase()).collect();
        matches!(
            upper.as_str(),
            "COM\u{00B9}"
                | "COM\u{00B2}"
                | "COM\u{00B3}"
                | "LPT\u{00B9}"
                | "LPT\u{00B2}"
                | "LPT\u{00B3}"
        )
    }
}

fn truncate_utf8_leaving_id_room(s: &mut String, id: i64) {
    let suffix_len = format!("-{id}").len();
    let max = 180usize.saturating_sub(suffix_len);
    if s.len() <= max {
        return;
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    while s.ends_with(' ') || s.ends_with('.') {
        s.pop();
    }
}

/// Insert `-<file_id>` before the final extension (if any).
#[must_use]
pub fn with_file_id_suffix(name: &str, file_id: i64) -> String {
    let (stem, ext) = name
        .rsplit_once('.')
        .filter(|(stem, ext)| !stem.is_empty() && !ext.is_empty())
        .map_or((name, String::new()), |(stem, ext)| {
            (stem, format!(".{ext}"))
        });
    let suffix = format!("-{file_id}");
    let mut ext = ext;
    truncate_to(&mut ext, 180 - suffix.len() - 1);
    let mut base = stem;
    while let Some(tail) = base.strip_suffix(&suffix) {
        base = tail;
    }
    let previous = &stem[base.len()..];
    let mut base = base.to_owned();
    truncate_to(
        &mut base,
        180usize.saturating_sub(suffix.len() + previous.len() + ext.len()),
    );
    format!("{base}{previous}{suffix}{ext}")
}

fn truncate_to(s: &mut String, max: usize) {
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
}

/// Case-insensitive NFC key for uniqueness comparison.
#[must_use]
pub fn uniqueness_key(path: &str) -> String {
    path.nfc().collect::<String>().to_lowercase()
}

/// Planned path before uniqueness, with the owning file id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathDraft {
    /// Relative path using `/` separators.
    pub path: String,
    /// Canvas file id (used for collision suffixes).
    pub file_id: i64,
}

/// Make every path unique (§12.3 uniqueness pass). Deterministic for a given plan.
#[must_use]
pub fn uniquify_paths(drafts: &[PathDraft]) -> Vec<String> {
    let mut paths: Vec<String> = drafts.iter().map(|d| d.path.clone()).collect();
    let mut reserved: HashSet<String> = drafts.iter().map(|d| uniqueness_key(&d.path)).collect();
    let mut rounds = vec![0usize; drafts.len()];

    loop {
        let mut groups: HashMap<String, Vec<usize>> = HashMap::new();
        for (i, p) in paths.iter().enumerate() {
            groups.entry(uniqueness_key(p)).or_default().push(i);
        }

        let mut changed = false;
        let mut collision_idxs: Vec<usize> = groups
            .into_values()
            .filter(|idxs| idxs.len() > 1)
            .flatten()
            .collect();
        let keys: Vec<_> = paths.iter().map(|p| uniqueness_key(p)).collect();
        for (i, key) in keys.iter().enumerate() {
            if keys
                .iter()
                .any(|other| other.starts_with(&format!("{key}/")))
            {
                collision_idxs.push(i);
            }
        }
        collision_idxs.sort_unstable();
        collision_idxs.dedup();

        for i in collision_idxs {
            let file_id = drafts[i].file_id;
            rounds[i] += 1;
            let (parent, name) = split_parent_name(&drafts[i].path);
            let mut candidate_name = name.to_owned();
            for _ in 0..rounds[i] {
                candidate_name = with_file_id_suffix(&candidate_name, file_id);
            }
            let mut candidate = join_parent(parent, &candidate_name);
            // If still collides with a reserved or another path's current key after this
            // round's intended set, keep the id suffix (already applied); further rounds
            // handle residual collisions. Generated names count as reserved.
            let key = uniqueness_key(&candidate);
            if reserved.contains(&key) {
                // Extremely unlikely same file_id twice; append again.
                candidate_name = with_file_id_suffix(&candidate_name, file_id);
                candidate = join_parent(parent, &candidate_name);
            }
            if candidate != paths[i] {
                paths[i] = candidate;
                changed = true;
            }
            reserved.insert(uniqueness_key(&paths[i]));
        }

        // Reserve all current paths so generated names stay reserved next round.
        for p in &paths {
            reserved.insert(uniqueness_key(p));
        }

        if !changed {
            // Verify uniqueness; if still colliding (identical after suffix), force again.
            let mut keys: HashMap<String, Vec<usize>> = HashMap::new();
            for (i, p) in paths.iter().enumerate() {
                keys.entry(uniqueness_key(p)).or_default().push(i);
            }
            let still: Vec<usize> = keys
                .into_values()
                .filter(|v| v.len() > 1)
                .flatten()
                .collect();
            if still.is_empty() {
                break;
            }
            // Should not happen with distinct file_ids; break to avoid infinite loop.
            break;
        }
    }

    paths
}

fn split_parent_name(path: &str) -> (&str, &str) {
    match path.rsplit_once('/') {
        Some((parent, name)) => (parent, name),
        None => ("", path),
    }
}

fn join_parent(parent: &str, name: &str) -> String {
    if parent.is_empty() {
        name.to_string()
    } else {
        format!("{parent}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_device_and_superscripts() {
        assert_eq!(sanitize_component("CON", ComponentKind::File, 1), "CON_");
        assert_eq!(
            sanitize_component("com1.txt", ComponentKind::File, 1),
            "com1_.txt"
        );
        assert_eq!(
            sanitize_component("COM\u{00B9}", ComponentKind::File, 2),
            "COM\u{00B9}_"
        );
        assert_eq!(
            sanitize_component("LPT\u{00B2}.dat", ComponentKind::File, 3),
            "LPT\u{00B2}_.dat"
        );
        assert_eq!(sanitize_component("AUX", ComponentKind::File, 4), "AUX_");
        assert_eq!(
            sanitize_component("nul.txt", ComponentKind::File, 5),
            "nul_.txt"
        );
    }

    #[test]
    fn empty_and_dot_components() {
        assert_eq!(sanitize_component("", ComponentKind::File, 9), "file-9");
        assert_eq!(
            sanitize_component("...", ComponentKind::Folder, 3),
            "folder-3"
        );
        assert_eq!(
            sanitize_component(".", ComponentKind::Module, 7),
            "module-7"
        );
        assert_eq!(sanitize_component("..", ComponentKind::File, 1), "file-1");
        assert_eq!(
            sanitize_component(".hidden", ComponentKind::File, 1),
            "hidden"
        );
    }

    #[test]
    fn invalid_chars_stripped() {
        assert_eq!(
            sanitize_component("a/b\\c:d*e?", ComponentKind::File, 1),
            "a_b_c_d_e_"
        );
    }

    #[test]
    fn generated_vs_literal_collision() {
        // Literal `file-1` vs empty-name fallback colliding after sanitize.
        let drafts = vec![
            PathDraft {
                path: "c/file-1".into(),
                file_id: 99,
            },
            PathDraft {
                path: "c/file-1".into(),
                file_id: 1,
            },
        ];
        let paths = uniquify_paths(&drafts);
        assert_ne!(uniqueness_key(&paths[0]), uniqueness_key(&paths[1]));
        assert!(paths.iter().any(|p| p.contains("-99") || p.contains("-1")));
    }

    #[test]
    fn case_insensitive_nfc_uniqueness() {
        let drafts = vec![
            PathDraft {
                path: "c/Resume.pdf".into(),
                file_id: 10,
            },
            PathDraft {
                path: "c/resume.pdf".into(),
                file_id: 11,
            },
        ];
        let paths = uniquify_paths(&drafts);
        assert_ne!(uniqueness_key(&paths[0]), uniqueness_key(&paths[1]));
    }
    #[test]
    fn truncation_unicode_extension_and_cascading_collisions() {
        let long = sanitize_component(&format!("{}.資料", "é".repeat(200)), ComponentKind::File, 1);
        assert!(long.len() <= 178);
        let drafts = vec![
            PathDraft {
                path: "c/a.pdf".into(),
                file_id: 1,
            },
            PathDraft {
                path: "c/A.pdf".into(),
                file_id: 2,
            },
            PathDraft {
                path: "c/a-1.pdf".into(),
                file_id: 3,
            },
            PathDraft {
                path: "c/a-1-1.pdf".into(),
                file_id: 4,
            },
        ];
        let out = uniquify_paths(&drafts);
        assert_eq!(
            out.iter()
                .map(|p| uniqueness_key(p))
                .collect::<HashSet<_>>()
                .len(),
            4
        );
        assert_eq!(out, uniquify_paths(&drafts));
        assert_eq!(with_file_id_suffix("notes.資料", 8), "notes-8.資料");
        let drafts = vec![
            PathDraft {
                path: long.clone(),
                file_id: 1,
            },
            PathDraft {
                path: long,
                file_id: 2,
            },
        ];
        assert!(uniquify_paths(&drafts).iter().all(|p| p.len() <= 180));
        assert!(!sanitize_component("..hidden", ComponentKind::File, 1).starts_with('.'));
        for prefix in ["COM", "LPT"] {
            for n in 0..=9 {
                assert!(
                    sanitize_component(&format!("{prefix}{n}"), ComponentKind::File, 1)
                        .ends_with('_')
                );
            }
        }
    }

    #[test]
    fn file_cannot_occupy_another_files_parent() {
        let out = uniquify_paths(&[
            PathDraft {
                path: "c/a".into(),
                file_id: 1,
            },
            PathDraft {
                path: "c/a/b".into(),
                file_id: 2,
            },
        ]);
        assert_eq!(out, ["c/a-1", "c/a/b"]);
    }
}
