//! `cargo xtask dist-assets --out DIR`: man pages and shell completions.
//!
//! Output is deterministic: the same command tree and the same `--out` produce
//! byte-identical files on every run, so release archives are reproducible.
//! `clap_mangen` writes no timestamp into the `.TH` header, and the completion
//! scripts come from [`canvas_cli::dist`], the module `canvas completions`
//! uses, so a shipped script matches the running binary.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use canvas_cli::dist;

/// Subdirectory holding the section-1 man pages.
pub const MAN_DIR: &str = "man";

/// Subdirectory holding the completion scripts.
pub const COMPLETIONS_DIR: &str = "completions";

/// `.TH` manual name shared by every page.
const MANUAL: &str = "Canvas CLI Manual";

/// What one `dist-assets` run wrote, relative to `--out`.
#[derive(Debug, Default)]
pub struct Assets {
    pub man_pages: Vec<PathBuf>,
    pub completions: Vec<PathBuf>,
}

/// Generate every distribution asset under `out`.
///
/// Existing `man/` and `completions/` directories are replaced, so a rerun
/// never leaves a stale page behind from a command that was removed.
pub fn generate(out: &Path) -> Result<Assets> {
    let man_dir = out.join(MAN_DIR);
    let completions_dir = out.join(COMPLETIONS_DIR);
    reset_dir(&man_dir)?;
    reset_dir(&completions_dir)?;

    let mut assets = Assets::default();
    // `build()` resolves the display names man page filenames come from
    // (`canvas-auth-login`), and propagates `disable_help_subcommand` down the
    // tree so clap's implicit `help` command gets no page.
    let mut root = dist::command().disable_help_subcommand(true);
    root.build();
    write_man_pages(&root, &man_dir, &mut assets.man_pages)?;
    assets.man_pages.sort();

    for &shell in dist::SHELLS {
        let name = dist::completion_file_name(shell);
        let path = completions_dir.join(&name);
        let mut script = Vec::new();
        dist::write_completions(shell, &mut script);
        fs::write(&path, &script)
            .with_context(|| format!("write completions {}", path.display()))?;
        assets
            .completions
            .push(Path::new(COMPLETIONS_DIR).join(&name));
    }
    Ok(assets)
}

/// One page per command and subcommand: `canvas.1`, `canvas-auth.1`,
/// `canvas-auth-login.1`, ... clap's implicit `help` subcommand gets no page.
fn write_man_pages(cmd: &clap::Command, dir: &Path, written: &mut Vec<PathBuf>) -> Result<()> {
    for sub in cmd
        .get_subcommands()
        .filter(|s| !s.is_hide_set() && s.get_name() != "help")
    {
        write_man_pages(sub, dir, written)?;
    }
    // `source` and `manual` are set on every page: a subcommand carries no
    // version of its own, so the default header would read `submit `. The date
    // stays empty because any real date would break determinism.
    let man = clap_mangen::Man::new(cmd.clone())
        .source(format!("canvas {}", env!("CARGO_PKG_VERSION")))
        .manual(MANUAL);
    let name = man.get_filename();
    let path = dir.join(&name);
    let mut page = Vec::new();
    man.render(&mut page)
        .with_context(|| format!("render man page {name}"))?;
    fs::write(&path, &page).with_context(|| format!("write man page {}", path.display()))?;
    written.push(Path::new(MAN_DIR).join(&name));
    Ok(())
}

fn reset_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        fs::remove_dir_all(dir).with_context(|| format!("clear {}", dir.display()))?;
    }
    fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    Ok(())
}
