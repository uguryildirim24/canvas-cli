//! Download path planning (§12.3 layout and ownership).

use crate::download::sanitize::{ComponentKind, PathDraft, sanitize_component, uniquify_paths};

/// Course-level planning input (plain structs; not canvas-api models).
#[derive(Debug, Clone)]
pub struct PlanInput {
    /// Course code (e.g. `CS101`).
    pub course_code: String,
    /// Canvas course id.
    pub course_id: i64,
    /// Modules with File items.
    pub modules: Vec<PlanModule>,
    /// Folder tree for the Files listing.
    pub folders: Vec<PlanFolder>,
    /// Files from the course Files listing.
    pub files: Vec<PlanFile>,
}

/// A module in the plan input.
#[derive(Debug, Clone)]
pub struct PlanModule {
    /// Module id.
    pub id: i64,
    /// Module name (becomes the slug).
    pub name: String,
    /// Module position (ownership and `NN` padding).
    pub position: i64,
    /// Items in this module.
    pub items: Vec<PlanModuleItem>,
}

/// A module item.
#[derive(Debug, Clone)]
pub struct PlanModuleItem {
    /// Module item id.
    pub id: i64,
    /// Item type (`File`, `ExternalTool`, …).
    pub item_type: String,
    /// Content id (file id when `item_type == "File"`).
    pub content_id: Option<i64>,
    /// Display title.
    pub title: String,
    /// Position within the module.
    pub position: i64,
}

/// A folder in the Files tree.
#[derive(Debug, Clone)]
pub struct PlanFolder {
    /// Folder id.
    pub id: i64,
    /// Folder name (one component).
    pub name: String,
    /// Parent folder id; `None` for the course-files root.
    pub parent_folder_id: Option<i64>,
    /// True when this is Canvas' course-files root (not emitted under `files/`).
    pub is_root: bool,
}

/// A file from the Files listing.
#[derive(Debug, Clone)]
pub struct PlanFile {
    /// File id.
    pub id: i64,
    /// Display name.
    pub display_name: String,
    /// Containing folder id.
    pub folder_id: i64,
    /// Byte size when known.
    pub size: Option<u64>,
    /// Remote `updated_at` when known (RFC 3339).
    pub updated_at: Option<String>,
}

/// Alias for documentation symmetry with the task brief.
pub type PlanCourse = PlanInput;

type OwnerEntry = (i64, i64, String, Option<u64>, Option<String>);

/// Where a planned file came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlannedSource {
    /// Owned by a module item.
    Module {
        /// Owning module id.
        module_id: i64,
    },
    /// Only present in the Files listing.
    FilesListing,
    /// External-tool module item (not downloaded).
    ExternalTool,
}

/// One planned path for a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedFile {
    /// Canvas file id (`content_id` for module File items).
    pub file_id: i64,
    /// Relative path under the destination root (`/` separators).
    pub path: String,
    /// Provenance.
    pub source: PlannedSource,
    /// Size hint from the listing when known.
    pub size: Option<u64>,
    /// Remote `updated_at` when known.
    pub updated_at: Option<String>,
    /// True when this is an external-tool skip (no path install).
    pub skipped_external: bool,
}

/// Plan all paths for a course. Independent of `--module` / `--file` filters.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn plan_course(input: &PlanInput) -> Vec<PlannedFile> {
    let course_root = format!(
        "{}-{}",
        sanitize_component(&input.course_code, ComponentKind::Folder, input.course_id),
        input.course_id
    );

    // file_id → best module owner (lowest position, then lowest module id)
    let mut module_owner: std::collections::HashMap<i64, OwnerEntry> =
        std::collections::HashMap::new();
    // (position, module_id) for comparison
    let mut owner_key: std::collections::HashMap<i64, (i64, i64)> =
        std::collections::HashMap::new();
    let mut externals: Vec<PlannedFile> = Vec::new();

    for module in &input.modules {
        for item in &module.items {
            if item.item_type.eq_ignore_ascii_case("ExternalTool") {
                externals.push(PlannedFile {
                    file_id: item.content_id.unwrap_or(item.id),
                    path: String::new(),
                    source: PlannedSource::ExternalTool,
                    size: None,
                    updated_at: None,
                    skipped_external: true,
                });
                continue;
            }
            if !item.item_type.eq_ignore_ascii_case("File") {
                continue;
            }
            let Some(file_id) = item.content_id else {
                continue;
            };
            let key = (module.position, module.id);
            let replace = match owner_key.get(&file_id) {
                None => true,
                Some(existing) => key < *existing,
            };
            if replace {
                owner_key.insert(file_id, key);
                let nn = format!("{:02}", module.position);
                let slug = sanitize_component(&module.name, ComponentKind::Module, module.id);
                let display = sanitize_component(&item.title, ComponentKind::File, file_id);
                let path = format!("{course_root}/modules/{nn}-{slug}/{display}");
                module_owner.insert(file_id, (file_id, module.id, path, None, None));
            }
        }
    }

    // Enrich sizes from files listing when available.
    let listing_by_id: std::collections::HashMap<i64, &PlanFile> =
        input.files.iter().map(|f| (f.id, f)).collect();
    for (fid, entry) in &mut module_owner {
        if let Some(f) = listing_by_id.get(fid) {
            entry.3 = f.size;
            entry.4.clone_from(&f.updated_at);
        }
    }

    let folder_paths = build_folder_paths(&input.folders);

    let mut drafts: Vec<PathDraft> = Vec::new();
    let mut meta: Vec<(i64, PlannedSource, Option<u64>, Option<String>)> = Vec::new();

    for (file_id, (_fid, module_id, path, size, updated_at)) in &module_owner {
        drafts.push(PathDraft {
            path: path.clone(),
            file_id: *file_id,
        });
        meta.push((
            *file_id,
            PlannedSource::Module {
                module_id: *module_id,
            },
            *size,
            updated_at.clone(),
        ));
    }

    for file in &input.files {
        if module_owner.contains_key(&file.id) {
            continue;
        }
        let folder_rel = folder_paths
            .get(&file.folder_id)
            .cloned()
            .unwrap_or_default();
        let display = sanitize_component(&file.display_name, ComponentKind::File, file.id);
        let path = if folder_rel.is_empty() {
            format!("{course_root}/files/{display}")
        } else {
            format!("{course_root}/files/{folder_rel}/{display}")
        };
        drafts.push(PathDraft {
            path,
            file_id: file.id,
        });
        meta.push((
            file.id,
            PlannedSource::FilesListing,
            file.size,
            file.updated_at.clone(),
        ));
    }

    // Stable order before uniqueness: by path then file_id (deterministic).
    let mut order: Vec<usize> = (0..drafts.len()).collect();
    order.sort_by(|&a, &b| {
        drafts[a]
            .path
            .cmp(&drafts[b].path)
            .then(drafts[a].file_id.cmp(&drafts[b].file_id))
    });
    let drafts_sorted: Vec<PathDraft> = order.iter().map(|&i| drafts[i].clone()).collect();
    let meta_sorted: Vec<_> = order.iter().map(|&i| meta[i].clone()).collect();

    let unique = uniquify_paths(&drafts_sorted);
    let mut out: Vec<PlannedFile> = unique
        .into_iter()
        .zip(meta_sorted)
        .map(|(path, (file_id, source, size, updated_at))| PlannedFile {
            file_id,
            path,
            source,
            size,
            updated_at,
            skipped_external: false,
        })
        .collect();
    out.extend(externals);
    out
}

fn build_folder_paths(folders: &[PlanFolder]) -> std::collections::HashMap<i64, String> {
    let by_id: std::collections::HashMap<i64, &PlanFolder> =
        folders.iter().map(|f| (f.id, f)).collect();
    let mut cache: std::collections::HashMap<i64, String> = std::collections::HashMap::new();

    for folder in folders {
        let _ = resolve_folder_path(folder.id, &by_id, &mut cache);
    }
    cache
}

fn resolve_folder_path(
    id: i64,
    by_id: &std::collections::HashMap<i64, &PlanFolder>,
    cache: &mut std::collections::HashMap<i64, String>,
) -> String {
    if let Some(existing) = cache.get(&id) {
        return existing.clone();
    }
    let Some(folder) = by_id.get(&id).copied() else {
        return String::new();
    };
    if folder.is_root {
        cache.insert(id, String::new());
        return String::new();
    }
    let name = sanitize_component(&folder.name, ComponentKind::Folder, folder.id);
    let path = match folder.parent_folder_id {
        Some(parent) => {
            let parent_path = resolve_folder_path(parent, by_id, cache);
            if parent_path.is_empty() {
                name
            } else {
                format!("{parent_path}/{name}")
            }
        }
        None => name,
    };
    cache.insert(id, path.clone());
    path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn module_ownership_lowest_position_then_id() {
        let input = PlanInput {
            course_code: "CS".into(),
            course_id: 10,
            modules: vec![
                PlanModule {
                    id: 2,
                    name: "Week B".into(),
                    position: 2,
                    items: vec![PlanModuleItem {
                        id: 1,
                        item_type: "File".into(),
                        content_id: Some(100),
                        title: "a.pdf".into(),
                        position: 1,
                    }],
                },
                PlanModule {
                    id: 1,
                    name: "Week A".into(),
                    position: 1,
                    items: vec![PlanModuleItem {
                        id: 2,
                        item_type: "File".into(),
                        content_id: Some(100),
                        title: "a.pdf".into(),
                        position: 1,
                    }],
                },
            ],
            folders: vec![],
            files: vec![],
        };
        let planned = plan_course(&input);
        assert_eq!(planned.len(), 1);
        assert!(planned[0].path.contains("modules/01-Week A") || planned[0].path.contains("01-"));
        assert!(matches!(
            planned[0].source,
            PlannedSource::Module { module_id: 1 }
        ));
    }

    #[test]
    fn files_listing_under_files_tree() {
        let input = PlanInput {
            course_code: "CS".into(),
            course_id: 5,
            modules: vec![],
            folders: vec![
                PlanFolder {
                    id: 1,
                    name: "course files".into(),
                    parent_folder_id: None,
                    is_root: true,
                },
                PlanFolder {
                    id: 2,
                    name: "Slides".into(),
                    parent_folder_id: Some(1),
                    is_root: false,
                },
            ],
            files: vec![PlanFile {
                id: 50,
                display_name: "lec.pdf".into(),
                folder_id: 2,
                size: Some(3),
                updated_at: None,
            }],
        };
        let planned = plan_course(&input);
        assert_eq!(planned.len(), 1);
        assert!(planned[0].path.ends_with("files/Slides/lec.pdf"));
    }
}
