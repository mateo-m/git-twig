# RFD0015: Windows block cloning

- Status: implementation snapshot
- Author: mateo-m
- Date: 2026-11-17

## Decision and motivation

Extend the verified same-HEAD materialization path to Windows using ReFS block
cloning. Windows users on ReFS, such as a Dev Drive, get independent CoW worktrees.
NTFS and other volumes continue to delegate to Git. This extends
[RFD0011](RFD0011-linux-reflinks.md) and keeps the checkout policy of
[RFD0014](RFD0014-checkout-policy-and-hybrid-materialization.md), without changing
CLI syntax.

## Contract

Use the Linux walker: enumerate the committed tree and clone each tracked regular
file. Open sources with `FILE_FLAG_OPEN_REPARSE_POINT` and reject directories and
reparse points. Require `FILE_SUPPORTS_BLOCK_REFCOUNTING` on the source volume
before creating a destination. Create destinations exclusively.

`FSCTL_DUPLICATE_EXTENTS_TO_FILE` needs one volume, matching sparse and integrity
settings, a presized destination, and cluster-aligned regions below 4 GiB. Read the
cluster size and integrity settings with `FSCTL_GET_INTEGRITY_INFORMATION`. Copy a
sparse flag and enabled integrity to the destination. Set the destination length,
then clone in cluster-aligned steps. The last region rounds up past end of file.
The ioctl shares clusters or fails: it never copies bytes.

Raw-byte verification opens private candidates with `FILE_FLAG_OPEN_REPARSE_POINT`,
like `O_NOFOLLOW` on Unix. The native line ending is CRLF on Windows, so `core.eol`
of `native` is an identity policy only on other platforms.

Git for Windows sets `core.symlinks=false` in each repository that cannot create
symlinks. Git then checks out a symlink as a plain file that holds the link text.
With `core.symlinks=false`, on every OS, treat such an entry as a regular file:
clone it, compare its bytes with the blob, and let Git write it with the same
setting. A real symlink in the source fails the regular-file checks and falls back.
With symlinks enabled, Windows delegates trees that contain symlinks.

Emit `Windows block clone CoW: verified tracked files` only after verification.
Failures before registration fall back with a `TWIG_TRACE` line, for example
`stock Git: volume does not support block cloning` on NTFS.

## Testing and rollout

Run formatting, Clippy and `cargo test -p git-twig` on Windows on NTFS and on a
ReFS Dev Drive. Set `TWIG_TEST_FIXTURE_ROOT` to a directory on the Dev Drive and
`TWIG_TEST_REQUIRE_NATIVE=1` so fallback cannot pass native correctness tests.
The Windows test probes the volume with PowerShell `Get-Volume`, not Twig.

## Alternatives and limitations

Windows has no directory clone, so per-file clones cost one set of calls per file.
NTFS has no block cloning. Windows CI, release archives and Windows benchmarks
are out of scope. `twig-bench` stays Unix-only.

See [block cloning](https://learn.microsoft.com/en-us/windows/win32/fileio/block-cloning)
and [ReFS block cloning](https://learn.microsoft.com/en-us/windows-server/storage/refs/block-cloning).
