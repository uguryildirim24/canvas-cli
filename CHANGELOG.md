# Changelog

All notable changes to `canvas-lms-cli` are recorded here. `release-plz`
generates each release section from the conventional commits since the previous
tag (`release-plz.toml`); do not hand-edit released sections.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

`0.1.0` is not released yet. The first tag will replace this section with the
generated notes.

### Added

- The command surface in `docs/SPEC.md` §5, one binary `canvas`, a local
  SQLite cache, and a versioned `--json` schema per data command.
- Distribution through the Homebrew tap `uguryildirim24/homebrew-tap`, with
  man pages and shell completions in every release archive.
