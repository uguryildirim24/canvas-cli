//! Color decision for human output (§7).

/// CLI `--color` choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    Auto,
    Always,
    Never,
}

/// Whether styled output should emit ANSI colors.
///
/// Precedence:
/// 1. `ColorMode::Never` → no color
/// 2. `ColorMode::Always` → color (CLI override)
/// 3. `NO_COLOR` set (any value) → no color
/// 4. `CLICOLOR_FORCE` set and not `"0"` → color
/// 5. otherwise → `stdout_is_tty`
#[must_use]
pub fn should_use_color(mode: ColorMode, stdout_is_tty: bool) -> bool {
    should_use_color_with_env(
        mode,
        stdout_is_tty,
        std::env::var_os("NO_COLOR").is_some(),
        std::env::var_os("CLICOLOR_FORCE").as_deref(),
    )
}

/// Testable color decision with explicit env inputs.
#[must_use]
pub fn should_use_color_with_env(
    mode: ColorMode,
    stdout_is_tty: bool,
    no_color: bool,
    clicolor_force: Option<&std::ffi::OsStr>,
) -> bool {
    match mode {
        ColorMode::Never => false,
        ColorMode::Always => true,
        ColorMode::Auto => {
            if no_color {
                return false;
            }
            if clicolor_force_enabled(clicolor_force) {
                return true;
            }
            stdout_is_tty
        }
    }
}

/// Resolve color for an invocation.
///
/// `--json` disables color and progress (SPEC §7).
#[must_use]
pub fn resolve_color(json: bool, mode: ColorMode, stdout_is_tty: bool) -> bool {
    if json {
        return false;
    }
    should_use_color(mode, stdout_is_tty)
}

fn clicolor_force_enabled(value: Option<&std::ffi::OsStr>) -> bool {
    match value {
        None => false,
        Some(v) => v != "0",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn never_and_always_override_env() {
        assert!(!should_use_color_with_env(
            ColorMode::Never,
            true,
            true,
            Some(OsStr::new("1")),
        ));
        assert!(should_use_color_with_env(
            ColorMode::Always,
            false,
            true,
            None,
        ));
    }

    #[test]
    fn auto_honors_no_color() {
        assert!(!should_use_color_with_env(
            ColorMode::Auto,
            true,
            true,
            Some(OsStr::new("1")),
        ));
    }

    #[test]
    fn auto_honors_clicolor_force() {
        assert!(should_use_color_with_env(
            ColorMode::Auto,
            false,
            false,
            Some(OsStr::new("1")),
        ));
        assert!(!should_use_color_with_env(
            ColorMode::Auto,
            false,
            false,
            Some(OsStr::new("0")),
        ));
    }

    #[test]
    fn auto_falls_back_to_tty() {
        assert!(should_use_color_with_env(
            ColorMode::Auto,
            true,
            false,
            None
        ));
        assert!(!should_use_color_with_env(
            ColorMode::Auto,
            false,
            false,
            None,
        ));
    }

    #[test]
    fn json_disables_color() {
        assert!(!resolve_color(true, ColorMode::Always, true));
        assert!(resolve_color(false, ColorMode::Always, false));
    }
}
