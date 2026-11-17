# RFD0014 — Checkout policy and hybrid materialization

- Status: implementation snapshot
- Author: leostera
- Date: 2026-11-16

## Decision and motivation

Expand same-HEAD native creation to common LF checkout policies without trusting
normalized Git cleanliness as a raw-byte certificate. Preserve verified clones
and let Git materialize exceptional tracked files in owned staging. Do not fork
Git, implement arbitrary filters, or alter source worktrees. No version bump or
release is part of this change.

An APFS research matrix reproduced two distinct hazards: line-ending conversion
can hide raw-byte differences, and `check-attr` serializes literal values named
`unset`/`unspecified` like special states. Published Twig can incorrectly bypass
its filter guard for drivers with those names. Query `--all`: genuine absence
is omitted, while literal values remain present. Conservatively delegate the
whole request when filter, ident or working-tree-encoding attributes are present.

The regular-file verifier is subsequently optimized by
[RFD0015](RFD0015-batched-raw-object-verification.md): raw Git blob hashing of
private files replaces streaming target blob contents, preserving object-identity
verification without filter normalization or stat-cache trust.

## Guide-level contract

`git twig new -b feat/x ../feat-x HEAD` may now share files in repositories using
`* text=auto eol=lf`. A donor file with uncommitted CRLF or other byte changes is
not inherited: Git writes the requested checkout bytes for that file instead.
Files that require CRLF are Git-written; unaffected files retain CoW sharing.
`TWIG_TRACE=1` reports verified-clone and Git-materialization counts.

Other revisions, unsupported filesystems/options, sparse checkouts, submodules,
external conversions and unsupported hooks still delegate before registration.
The existing staging ownership, inherited permissions, no-overwrite, hook and
partial-failure contracts remain in force.

## Reference-level contract

1. Parse committed entries including mode, raw path and validated object ID.
2. Build a private index from the resolved commit. Query attributes with
   `check-attr --cached --all -z --stdin`, never donor-worktree policy files.
   Capture effective config and attribute results, recheck before registration
   and against the destination's private target-index context before installation.
   Conditional includes and externally redirected attribute sources delegate.
   Ordinary worktree configuration remains eligible: Git copies config.worktree
   (excluding location/bare settings). Direct relative include.path values in
   that file delegate before staging because their anchor moves to the new Git
   directory without copying the included files. Absolute includes remain eligible.
3. Preserve bulk APFS candidate cloning and the Linux tracked-file walker.
   Private clones are normalized to checkout permissions and pruned as before.
   Missing/type-changed source trees may still cause whole-request fallback.
4. For supported identity checkout policies, compare each private candidate's
   raw bytes with its target blob using an unfiltered `cat-file --batch` stream.
   Blob headers must match the requested object, type and valid byte length.
   Symlink targets are compared as link text. Never follow links while verifying.
5. Classify regular paths needing conversion or whose raw bytes differ for Git
   materialization. Replace only owned private entries, using a private index
   and `checkout-index --stdin -z`. Materialize target `.gitattributes` first so
   donor edits cannot affect Git's checkout policy. Nonidentity policy paths are
   Git-written, not certified by a clean-filter roundtrip.
6. Recheck policy before Git owns registration, and again in the new worktree
   before moving staged files. A post-registration change is an explicit retained
   partial failure, never a blind retry through stock Git.

Identity means checkout leaves blob bytes unchanged, not that blobs contain LF.
Git can preserve legacy CRLF stored in blobs even with `eol=lf`. Explicit LF,
`autocrlf=input`, and applicable native-LF policies can qualify when no other
conversion applies. Ambiguous or unknown EOL/text values are Git-materialized.
Legacy `crlf` policies are also Git-materialized initially.

No filtered batch protocol is used: Git's filtered batch headers can report the
original blob size rather than converted payload length. Requests in the raw
batch contain object IDs only, avoiding pathname/newline framing ambiguity.
Git 2.40 remains the minimum; no `cat-file -Z` or `GIT_ATTR_SOURCE` dependency.

## Validation

Differential tests compare actual Git checkouts, not merely `status`: LF/CRLF,
legacy blobs, binary/empty files, attribute nesting/macros/info/global rules,
dirty and untracked attributes, literal sentinel values, private writes,
SHA-256 repositories, hostile paths, symlinks and executable modes. Retain all
existing permissions, staging, hooks, concurrent registration and Linux native
XFS/Btrfs tests. Exercise minimum Git and Rust versions. Preserve external-filter
failure/side-effect behavior by delegating before running such conversions.

Benchmark real OpenClaw with unchanged attributes only after correctness checks,
using frozen binaries and retained raw results. No speed or allocation improvement
is asserted by this design alone.

## Alternatives and limitations

Dropping the attribute guard makes normalized verification unsound. Cloning only
an LF allowlist still wastes sharing when one exceptional file exists. Checking
filtered bytes for every path duplicates Git conversion work and can execute
external filters twice. Per-file clone-only materialization abandons the existing
APFS bulk optimization without measurement. All-Git fallback is retained where
the stronger per-path contract is not established.

Config/attribute rechecks detect changes, but are not a cross-process transaction
against arbitrary writers of repository metadata. As with ordinary Git checkout,
concurrent configuration changes are outside the supported quiescent policy
contract. Privately cloned data is never trusted from source stat caches.

Different revisions, alternate donors, re-sharing existing worktrees, JSON output
and braiding are separate work. Metadata normalization beyond existing Git-mode/
umask/ACL guarantees, such as donor xattrs/quarantine, is not expanded here.
