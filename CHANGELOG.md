# Changelog

This log starts with one consolidated entry for the feature set available in
v0.3.0, including features introduced in earlier releases. Future entries will
describe changes from this baseline after they are implemented and verified.

## Unreleased

### Fixed

- Distinguish absent attributes from literal filter names such as `unset` and
  `unspecified`; these filters can no longer bypass native eligibility checks.
- Verify private clone bytes against raw target blobs rather than trusting a
  normalized clean-filter/index comparison.
- Resolve attributes from the target index, not dirty, staged or untracked
  source attribute files. Recheck policy before registration and installation;
  retain partial state on a post-registration policy change.
- Delegate before staging when copied worktree configuration contains relative
  includes whose resolution would change in the new worktree; keep consistent
  ordinary worktree overrides and absolute includes eligible.

### Added

- Native coverage for supported identity-checkout policies, including
  `text=auto eol=lf` and applicable `core.autocrlf=input` configurations.
- Hybrid creation: Git writes files requiring line-ending conversion or whose
  candidate bytes differ; other verified files retain copy-on-write sharing.
- Trace counts for verified file clones and Git-written paths.
- Native creation on Windows ReFS volumes, such as a Dev Drive, with block
  cloning. NTFS and other volumes delegate to Git.
- Native creation with `core.symlinks=false`: Git checks out symlinks as plain
  files that hold the link text, and Twig clones them like other files.
- Specific fallback reasons for unsupported requests, configuration, hooks and
  destinations; attribute refusals name the escaped path, attribute and value.
- Differential and failure tests for line endings, literal attribute values,
  attribute precedence, dirty policies, mapped donors, private writes, SHA-256
  repositories, unusual paths and policy changes during creation.
- CI coverage for the minimum supported Git 2.40 on macOS and Linux.

External filters, ident expansion, working-tree encodings, conditional config
includes and relative attribute-file configuration remain conservative
whole-request fallbacks. Other-revision cloning, braiding and re-sharing are
not included. No new version has been tagged or published.

## [0.3.0] — 2026-11-16

### Worktree creation

- `git twig new` creates ordinary Git linked worktrees with native copy-on-write
  file sharing on macOS/APFS and Linux Btrfs or reflink-enabled XFS.
- Native creation supports explicit new branches (`-b`) and detached worktrees
  (`--detach`) at the source checkout's current `HEAD`.
- Git owns worktree registration, branch validation and shared repository data.
  Native creation runs the post-checkout hook after installation.
- Private clones use Git-based verification against the requested commit;
  uncommitted changes and untracked or ignored files are not intentionally
  inherited. Writes do not modify source files; source hardlinks are never used.
- macOS clones tracked subtrees in bulk and prunes unrelated entries. Linux
  clones tracked regular files individually. Executable modes and symlinks are
  handled as Git checkout entries.
- Existing destinations are not overwritten. Owned staging lives under the
  common Git directory in `twigs/staging`; successful operations clean their
  staging, while failed or interrupted attempts retain it for inspection.

### Commands and agent support

- `new` is the creation command. `add` is rejected with guidance to use `new`.
- `list`, `remove`, `move`, `lock`, `unlock`, `prune` and `repair` delegate to Git.
  Machine-readable listing is available through `list --porcelain [-z]`.
- `git twig skill` prints bundled agent instructions without requiring a checkout.
- Short help, subcommand help and `--version` work outside a repository.
- `TWIG_TRACE=1` reports native backend selection or fallback.
- The website serves agent setup instructions at
  <https://twig.leostera.dev/llms.txt> and provides a copyable onboarding prompt.

### Installation

- One `git-twig` Cargo package contains the CLI and Rust library.
- Homebrew and GitHub release archives provide macOS arm64/x86_64 and Linux
  x86_64 binaries. Homebrew and archives include the `git-twig(1)` manual.
- Requires Git 2.40+; building from source requires Rust 1.95+.

### Compatibility limits

- Native CoW requires source, destination and common Git metadata on the same
  supported filesystem. Other revisions, unsupported options/filesystems and
  unsupported checkout semantics delegate to `git worktree`.
- Native coverage excludes conversion attributes such as `text=auto eol=lf`,
  non-false `core.autocrlf`, sparse checkouts, submodules and certain hooks.
  This is not full native coverage of Git's checkout behavior.
- Failed installation can leave a partial registered worktree; inspect retained
  state before retrying. There is no automatic repair or sweep of old staging.
- JSON listing, commit/integration queues and re-sharing existing worktrees are
  not available in this release. macOS binaries are not signed or notarized.

[0.3.0]: https://github.com/leostera/git-twig/releases/tag/v0.3.0
