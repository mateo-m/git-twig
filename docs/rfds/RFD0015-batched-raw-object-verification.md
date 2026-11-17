# RFD0015 — Batched raw-object verification without blob inflation

- Status: implementation snapshot
- Author: leostera
- Date: 2026-11-17

## Decision and motivation

Address the verification overhead highlighted by issue #5 without falling back
to stat-cache trust. For regular files, hash owned private clone bytes with Git's
`hash-object --no-filters --stdin-paths` and compare with the target blob object
IDs. This avoids inflating target blobs and copying their contents through a pipe
just to compare them with another copy of the same bytes.

The first local profile of the pinned pulumi/docs tree did not reproduce the
reporter's two-minute latency, either with a shallow object store or a 2.62-GiB
full-history pack. It did show the candidate reading Git's cat-file output while
Git inflated/wrote blobs. These observations justify measuring an alternative,
not claiming the reported slowdown is already proven fixed.

## Contract

- Git supplies raw blob hashing, the repository's SHA-1/SHA-256 format and its
  SHA-1 collision detection. No new hashing implementation or stat certificate.
- Target inventory includes blob lengths from `ls-tree -rlz`. A private file whose
  length differs cannot match and goes directly to Git materialization. In
  particular, an oversized dirty sparse donor must not trigger hashing terabytes.
- Only normalized private regular entries are sent to the hashing process. No
  source file is hashed, mutated or trusted from stat metadata. Snapshot ancestors
  and leaves have been checked and this phase exclusively owns their mutation.
- `--no-filters` is mandatory and `-w` is never used. Hashing must not execute a
  clean/smudge filter or write objects to the repository.
- Requests use Git C-quoted absolute paths. Three-digit octal escapes preserve
  newlines, quotes, backslashes and non-UTF8 bytes on Git 2.40; this command does
  not have a NUL-path input interface. This is not a pathspec protocol.
- Output must contain exactly one valid object ID of the expected format per
  requested path, in order. Wrong counts, malformed hashes and child failures
  abort preparation and conservatively delegate before registration.
- Large inventories (at least 512 regular files and 64 MiB) use up to four Git
  workers, capped by available CPU count. Largest-first byte balancing limits
  skew. Smaller inventories remain one process. No process per file.
- Every hash worker joins before any repair, installation or fallback. A worker
  error cannot leave another worker reading files concurrently with replacement.
- Symlink targets retain raw blob comparison; no followed symlink is hashed as a
  regular file. Conversion-required paths still go to Git's checkout machinery.
- Modes, path inventory, hooks, policy rechecks, no-overwrite and staging ownership
  remain as in RFD0014. Byte identity here means Git object identity, not normalized
  index cleanliness and not a promise against a malicious same-user staging writer.

Opt-in trace reports worker/byte counts and snapshot/verification phase timings.
Staging diagnostics say it is retained *if* creation fails rather than implying a
successful command leaked a staging directory.

## Validation

Keep all raw dirty-byte, sentinel-filter, mmap, SHA-256, odd-path, permissions,
policy-race and write-isolation regressions. Add malformed hash-response and
C-quoting tests, a 64-GiB sparse dirty donor bounded-time test, and an inventory
that exercises parallel hashing and repairs under both object formats.

Measure actual stock Git, pre-fix Twig and the frozen new executable on the exact
pulumi/docs commit from #5, with the same full-history input. Retain warmups,
repetitions, resource/latency observations, independent file/mode/link checks and
source/private-write verification. Do not use instrumented profile time as a
benchmark sample or claim a reporter-specific 20x regression was reproduced when
it was not. Refresh release measurements affected by this algorithm change.

## Alternatives and limitations

Keeping cat-file comparisons is simple but duplicates blob materialization and
serializes requests on one pipe. Trusting index stat records is faster but weakens
the content invariant. Implementing SHA-1/SHA-256 ourselves adds dependencies and
must also handle Git's SHA-1 collision-detection behavior. Git's raw hashing
plumbing preserves those semantics with a bounded batch interface.

Parallel hashing can contend with other agents and concurrent creates; the bound
is deliberately small and small repositories do not spawn extra workers. It is
not a universal latency win. A new worktree's fresh index can still make its first
status expensive; verified-index reuse is separate work and not silently enabled.
