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

/// Install lines the generated Homebrew formula is missing.
///
/// `dist` 0.32 has no hook for extra formula content: its template installs the
/// binary and sweeps everything else into `pkgshare`, which would leave the man
/// pages and completions unusable. This rewrites that sweep into real Homebrew
/// destinations. It is idempotent, and it fails loudly if the template it
/// anchors on changes.
const FORMULA_ANCHOR: &str = "    # Homebrew will automatically install these";

const FORMULA_INSTALLS: &str = r#"    man1.install Dir["man/*.1"]
    bash_completion.install "completions/canvas.bash" => "canvas"
    zsh_completion.install "completions/_canvas"
    fish_completion.install "completions/canvas.fish"
    # Homebrew has no shared location for these two shells.
    pkgshare.install "completions/_canvas.ps1", "completions/canvas.elv"

"#;

const FORMULA_LEFTOVERS: &str = r#"    leftover_contents = Dir["*"] - doc_files"#;

const FORMULA_LEFTOVERS_PATCHED: &str =
    r#"    leftover_contents = Dir["*"] - doc_files - ["man", "completions"]"#;

/// Add the man page and completion install lines to a `dist`-generated formula.
///
/// Returns `true` when the file was rewritten, `false` when it already had
/// them.
pub fn patch_formula(path: &Path) -> Result<bool> {
    let original =
        fs::read_to_string(path).with_context(|| format!("read formula {}", path.display()))?;
    if original.contains(r#"man1.install Dir["man/*.1"]"#) {
        return Ok(false);
    }
    anyhow::ensure!(
        original.matches(FORMULA_ANCHOR).count() == 1
            && original.matches(FORMULA_LEFTOVERS).count() == 1,
        "{} is not a formula this task knows how to patch; \
         the dist Homebrew template changed and xtask/src/dist_assets.rs needs updating",
        path.display()
    );
    let patched = original
        .replace(
            FORMULA_ANCHOR,
            &format!("{FORMULA_INSTALLS}{FORMULA_ANCHOR}"),
        )
        .replace(FORMULA_LEFTOVERS, FORMULA_LEFTOVERS_PATCHED);
    fs::write(path, patched).with_context(|| format!("write formula {}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tail of a `dist` 0.32 Homebrew formula, as generated.
    const GENERATED: &str = r#"  def install
    if OS.mac? && Hardware::CPU.arm?
      bin.install "canvas"
    end

    install_binary_aliases!

    # Homebrew will automatically install these, so we don't need to do that
    doc_files = Dir["README.*", "readme.*", "LICENSE", "LICENSE.*", "CHANGELOG.*"]
    leftover_contents = Dir["*"] - doc_files

    # Install any leftover files in pkgshare; these are probably config or
    # sample files.
    pkgshare.install(*leftover_contents) unless leftover_contents.empty?
  end
end
"#;

    use tempfile::TempDir;

    fn write(dir: &TempDir, body: &str) -> PathBuf {
        let path = dir.path().join("canvas-lms-cli.rb");
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn patch_installs_man_pages_and_completions() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, GENERATED);
        assert!(patch_formula(&path).unwrap());
        let patched = fs::read_to_string(&path).unwrap();
        for line in [
            r#"man1.install Dir["man/*.1"]"#,
            r#"bash_completion.install "completions/canvas.bash" => "canvas""#,
            r#"zsh_completion.install "completions/_canvas""#,
            r#"fish_completion.install "completions/canvas.fish""#,
            r#"pkgshare.install "completions/_canvas.ps1", "completions/canvas.elv""#,
        ] {
            assert!(patched.contains(line), "missing {line}\n{patched}");
        }
        // The generic sweep must no longer claim the two directories.
        assert!(patched.contains(FORMULA_LEFTOVERS_PATCHED));
        assert_eq!(patched.matches("pkgshare.install").count(), 2);
    }

    #[test]
    fn patch_is_idempotent() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, GENERATED);
        assert!(patch_formula(&path).unwrap());
        let once = fs::read_to_string(&path).unwrap();
        assert!(!patch_formula(&path).unwrap());
        assert_eq!(once, fs::read_to_string(&path).unwrap());
    }

    #[test]
    fn patch_refuses_a_template_it_does_not_recognize() {
        let dir = TempDir::new().unwrap();
        let path = write(&dir, "class CanvasLmsCli < Formula\nend\n");
        let err = patch_formula(&path).unwrap_err().to_string();
        assert!(err.contains("dist Homebrew template changed"), "{err}");
    }
}
