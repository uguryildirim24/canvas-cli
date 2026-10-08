# Changelog

All notable changes to `canvas-lms-cli` are recorded here. `release-plz`
generates each release section from the conventional commits since the previous
tag (`release-plz.toml`); do not hand-edit released sections.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

`0.1.0` is not released yet. `release-plz` inserts each generated release
**above** this section and never removes it, so empty it by hand when the
first tag lands.

### Removed

- Superseded agent task briefs and dialogue notes. Historical review evidence
  and research reports remain, with private paths removed.

### Added

- The command surface in `docs/SPEC.md` §5, one binary `canvas`, a local
  SQLite cache, and a versioned `--json` schema per data command.
- Release packaging configuration for archives, man pages, shell completions,
  and a Homebrew formula. No package-install route has been verified.
