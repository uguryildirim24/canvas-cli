//! Capability-scoped containment (§12.3).

use std::io;
use std::path::{Component, Path};

use cap_fs_ext::{DirExt, FollowSymlinks, OpenOptionsFollowExt};
use cap_std::fs::{Dir, File, OpenOptions};
use thiserror::Error;

/// Containment failure.
#[derive(Debug, Error)]
pub enum ContainError {
    /// Symlink, reparse point, `..`, or absolute path.
    #[error("unsafe_path")]
    UnsafePath,
    /// Underlying I/O error.
    #[error(transparent)]
    Io(#[from] io::Error),
}

/// Parent directory handle plus final file name after a contained walk.
#[derive(Debug)]
pub struct ContainedPath {
    /// Retained parent `Dir` (no-follow).
    pub parent: Dir,
    /// Final path component (file or dir name).
    pub name: String,
    /// Intermediate directory handles (excluding root and parent), oldest first.
    pub intermediates: Vec<Dir>,
}

/// Walk `rel` under `root`, creating missing directories, refusing symlinks/`..`/absolute.
pub fn walk_parent(root: &Dir, rel: &str) -> Result<ContainedPath, ContainError> {
    let components = normalize_rel_components(rel)?;
    if components.is_empty() {
        return Err(ContainError::UnsafePath);
    }
    let (dirs, name) = components.split_at(components.len() - 1);
    let name = name[0].clone();

    let mut current = root.try_clone()?;
    let mut intermediates = Vec::new();
    for comp in dirs {
        current = open_or_create_dir(&current, comp)?;
        intermediates.push(current.try_clone()?);
    }

    // Inspect final entry if present (no-follow).
    match current.symlink_metadata(&name) {
        Ok(meta) if is_link(&meta) => return Err(ContainError::UnsafePath),
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }

    Ok(ContainedPath {
        parent: current,
        name,
        intermediates,
    })
}

/// Open the final file with `FollowSymlinks::No` and keep the descriptor.
pub fn open_contained_file(contained: &ContainedPath, write: bool) -> Result<File, ContainError> {
    // Re-check immediately before open (TOCTOU: symlink swapped between inspect and open).
    match contained.parent.symlink_metadata(&contained.name) {
        Ok(meta) if is_link(&meta) => return Err(ContainError::UnsafePath),
        Ok(meta) if !meta.is_file() => return Err(ContainError::UnsafePath),
        Ok(_) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if !write {
                return Err(e.into());
            }
        }
        Err(e) => return Err(e.into()),
    }

    let mut opts = OpenOptions::new();
    opts.read(true).follow(FollowSymlinks::No);
    if write {
        opts.write(true).create(true);
    }
    match contained.parent.open_with(&contained.name, &opts) {
        Ok(f) if f.metadata()?.is_file() => Ok(f),
        Ok(_) => Err(ContainError::UnsafePath),
        Err(e) if e.kind() == io::ErrorKind::NotFound && write => {
            // create_new style for first install uses separate temp; for direct open allow create.
            let mut opts = OpenOptions::new();
            opts.write(true)
                .create_new(true)
                .read(true)
                .follow(FollowSymlinks::No);
            Ok(contained.parent.open_with(&contained.name, &opts)?)
        }
        Err(e) => {
            if contained
                .parent
                .symlink_metadata(&contained.name)
                .is_ok_and(|m| is_link(&m))
            {
                Err(ContainError::UnsafePath)
            } else {
                Err(e.into())
            }
        }
    }
}

/// Create a temp part file `.<name>.<random>.part` with `create_new` in the parent.
pub fn create_part_file(
    parent: &Dir,
    final_name: &str,
    random: &str,
) -> Result<(File, String), ContainError> {
    validate_name(final_name)?;
    validate_name(random)?;
    let part_name = format!(".{final_name}.{random}.part");
    let mut opts = OpenOptions::new();
    opts.write(true)
        .create_new(true)
        .read(true)
        .follow(FollowSymlinks::No);
    let file = parent.open_with(&part_name, &opts)?;
    Ok((file, part_name))
}

/// Rename part → final within the same parent handle.
pub fn install_rename(parent: &Dir, part_name: &str, final_name: &str) -> Result<(), ContainError> {
    validate_name(part_name)?;
    validate_name(final_name)?;
    // Refuse if final is currently a symlink.
    if let Ok(meta) = parent.symlink_metadata(final_name)
        && is_link(&meta)
    {
        return Err(ContainError::UnsafePath);
    }
    parent.rename(part_name, parent, final_name)?;
    sync_dir(parent)?;
    Ok(())
}

/// Persist directory entry changes where directory fsync is supported.
///
/// A `Dir` cannot be flushed directly. Where the platform has `O_PATH` —
/// Linux, Android, FreeBSD — `cap-std` opens every directory with it, because
/// a sandbox root only ever needs to be a `*at` anchor. `fsync` on an `O_PATH`
/// descriptor fails with `EBADF`, so syncing the handle — or a clone of it —
/// errors on exactly those platforms. macOS has no `O_PATH`, which is why the
/// fault only ever showed on Linux.
///
/// Reopening `.` through the directory itself yields an ordinary read-only
/// descriptor for the same inode, which the kernel will flush. The reopen
/// stays inside the sandbox and resolves no name, so it cannot cross a
/// symlink that the caller's contained walk already refused.
pub(crate) fn sync_dir(dir: &Dir) -> Result<(), io::Error> {
    #[cfg(unix)]
    dir.open(".")?.sync_all()?;
    #[cfg(not(unix))]
    let _ = dir;
    Ok(())
}

fn open_or_create_dir(parent: &Dir, name: &str) -> Result<Dir, ContainError> {
    match parent.symlink_metadata(name) {
        Ok(meta) if is_link(&meta) => Err(ContainError::UnsafePath),
        Ok(meta) if meta.is_dir() => parent
            .open_dir_nofollow(name)
            .map_err(|_| ContainError::UnsafePath),
        Ok(_) => Err(ContainError::UnsafePath),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            match parent.create_dir(name) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
            // Re-open nofollow; if a symlink appeared, fail.
            match parent.open_dir_nofollow(name) {
                Ok(d) => Ok(d),
                Err(_) => Err(ContainError::UnsafePath),
            }
        }
        Err(e) => Err(e.into()),
    }
}

pub(crate) fn is_link(meta: &cap_std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use cap_std::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}

fn validate_name(name: &str) -> Result<(), ContainError> {
    if name.is_empty() || name == "." || name == ".." || name.contains(['/', '\\', ':', '\0']) {
        return Err(ContainError::UnsafePath);
    }
    Ok(())
}

fn normalize_rel_components(rel: &str) -> Result<Vec<String>, ContainError> {
    if rel.is_empty() || rel.contains(['\\', ':', '\0']) {
        return Err(ContainError::UnsafePath);
    }
    let path = Path::new(rel);
    if path.is_absolute() {
        return Err(ContainError::UnsafePath);
    }
    let mut out = Vec::new();
    for comp in path.components() {
        match comp {
            Component::Normal(s) => {
                let s = s.to_string_lossy();
                if s == ".." || s == "." {
                    return Err(ContainError::UnsafePath);
                }
                out.push(s.into_owned());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ContainError::UnsafePath);
            }
        }
    }
    if out.is_empty() {
        return Err(ContainError::UnsafePath);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_scratch::Scratch;
    use cap_std::ambient_authority;

    fn scratch() -> (Scratch, Dir) {
        let path = Scratch::new("canvas-core-contain");
        let dir = Dir::open_ambient_dir(&path, ambient_authority()).unwrap();
        (path, dir)
    }

    #[test]
    fn rejects_dotdot_and_absolute() {
        let (_p, root) = scratch();
        assert!(matches!(
            walk_parent(&root, "../x"),
            Err(ContainError::UnsafePath)
        ));
        #[cfg(unix)]
        assert!(matches!(
            walk_parent(&root, "/abs"),
            Err(ContainError::UnsafePath)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_parent() {
        let (path, root) = scratch();
        std::fs::create_dir(path.join("real")).unwrap();
        std::os::unix::fs::symlink("real", path.join("link")).unwrap();
        assert!(matches!(
            walk_parent(&root, "link/file.txt"),
            Err(ContainError::UnsafePath)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_final() {
        let (path, root) = scratch();
        std::fs::write(path.join("target.txt"), b"x").unwrap();
        root.symlink("target.txt", "link.txt").unwrap();
        assert!(matches!(
            walk_parent(&root, "link.txt"),
            Err(ContainError::UnsafePath)
        ));
    }

    #[cfg(unix)]
    #[test]
    fn symlink_swapped_between_inspect_and_open() {
        let (path, root) = scratch();
        std::fs::write(path.join("ok.txt"), b"data").unwrap();
        let contained = walk_parent(&root, "ok.txt").unwrap();
        std::fs::remove_file(path.join("ok.txt")).unwrap();
        std::os::unix::fs::symlink("elsewhere", path.join("ok.txt")).unwrap();
        assert!(matches!(
            open_contained_file(&contained, false),
            Err(ContainError::UnsafePath)
        ));
    }

    /// Every durable write ends in `sync_dir`, so it has to work on the
    /// `O_PATH` directory handles `cap-std` hands out on Linux as well as on
    /// the plain ones macOS gives. Syncing the handle itself returns `EBADF`
    /// on the former and nothing at all on the latter, so the difference is
    /// invisible without this.
    #[test]
    fn syncs_a_directory_handle_and_the_ones_below_it() {
        let (path, root) = scratch();
        sync_dir(&root).unwrap();

        let contained = walk_parent(&root, "a/b/c.txt").unwrap();
        std::fs::write(path.join("a/b/c.txt"), b"data").unwrap();
        sync_dir(&contained.parent).unwrap();
        for dir in &contained.intermediates {
            sync_dir(dir).unwrap();
        }
    }
}
