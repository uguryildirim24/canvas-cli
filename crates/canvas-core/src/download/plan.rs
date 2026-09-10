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
#[allow(clippy::too_many_lines)]
pub fn plan_course(input: &PlanInput) -> Result<Vec<PlannedFile>, PlanError> {
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

    let mut modules: Vec<_> = input.modules.iter().collect();
    modules.sort_by_key(|m| (m.position, m.id));
    for module in modules {
        let mut items: Vec<_> = module.items.iter().collect();
        items.sort_by_key(|i| (i.position, i.id));
        for item in items {
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
                let slug = sanitize_component(
                    &format!("{nn}-{}", module.name),
                    ComponentKind::Module,
                    module.id,
                );
                let display = sanitize_component(&item.title, ComponentKind::File, file_id);
                let path = format!("{course_root}/modules/{slug}/{display}");
                module_owner.insert(file_id, (file_id, module.id, path, None, None));
            }
        }
    }

    // Enrich sizes from files listing when available.
    let listing_by_id: std::collections::HashMap<i64, &PlanFile> =
        input.files.iter().map(|f| (f.id, f)).collect();
    for (fid, entry) in &mut module_owner {
        if let Some(f) = listing_by_id.get(fid) {
            let parent = entry.2.rsplit_once('/').expect("planned parent").0;
            entry.2 = format!(
                "{parent}/{}",
                sanitize_component(&f.display_name, ComponentKind::File, f.id)
            );
            entry.3 = f.size;
            entry.4.clone_from(&f.updated_at);
        }
    }

    let folder_paths = build_folder_paths(&input.folders)?;

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

    let mut seen_files = std::collections::HashSet::new();
    for file in &input.files {
        if module_owner.contains_key(&file.id) || !seen_files.insert(file.id) {
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
    Ok(out)
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PlanError {
    #[error("cycle in folder tree at folder {0}")]
    FolderCycle(i64),
}

fn build_folder_paths(
    folders: &[PlanFolder],
) -> Result<std::collections::HashMap<i64, String>, PlanError> {
    let by_id: std::collections::HashMap<_, _> = folders.iter().map(|f| (f.id, f)).collect();
    let mut paths = std::collections::HashMap::new();
    for folder in folders {
        let mut seen = std::collections::HashSet::new();
        let mut components = Vec::new();
        let mut current = Some(folder.id);
        while let Some(id) = current {
            if !seen.insert(id) {
                return Err(PlanError::FolderCycle(id));
            }
            let Some(f) = by_id.get(&id) else {
                break;
            };
            if f.is_root {
                break;
            }
            components.push(sanitize_component(&f.name, ComponentKind::Folder, f.id));
            current = f.parent_folder_id;
        }
        components.reverse();
        paths.insert(folder.id, components.join("/"));
    }
    Ok(paths)
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
        let planned = plan_course(&input).unwrap();
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
        let planned = plan_course(&input).unwrap();
        assert_eq!(planned.len(), 1);
        assert!(planned[0].path.ends_with("files/Slides/lec.pdf"));
    }
    #[test]
    fn cycle_is_an_error_and_listing_duplicates_are_one_file() {
        let mut input = PlanInput {
            course_code: "CS".into(),
            course_id: 1,
            modules: vec![],
            folders: vec![PlanFolder {
                id: 1,
                name: "loop".into(),
                parent_folder_id: Some(1),
                is_root: false,
            }],
            files: vec![],
        };
        assert_eq!(plan_course(&input), Err(PlanError::FolderCycle(1)));
        input.folders.clear();
        let file = PlanFile {
            id: 1,
            display_name: "x".into(),
            folder_id: 1,
            size: None,
            updated_at: None,
        };
        input.files = vec![file.clone(), file];
        assert_eq!(plan_course(&input).unwrap().len(), 1);
    }
}
