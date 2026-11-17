# Releases

Version 0.4.0 is published on [crates.io](https://crates.io/crates/git-twig) and
[GitHub](https://github.com/leostera/git-twig/releases/tag/v0.4.0).
Crates.io publication is manual and requires owner approval.

Before publishing:
1. Run all local/CI checks and benchmark the intended commit. Add the next
   [changelog](../CHANGELOG.md) entry for implemented and verified changes;
   do not describe planned fixes as shipped.
2. Confirm the git-twig crate name/version is available on crates.io.
3. Review `cargo package -p git-twig --list`; include the license and bundled skill,
   never local fixtures. Run `cargo publish -p git-twig --dry-run --locked`.
4. Bump the workspace version in Cargo.toml.
5. Configure GitHub's **release** environment with required reviewers. Ensure the
   release bot may update Formula/git-twig.rb on main, or adapt it to open a PR.
6. Review GitHub Actions permissions and the generated formula.

With owner approval, push a vVERSION tag matching Cargo.toml. The release workflow
builds macOS arm64/x86_64 and Linux x86_64 archives (including `git-twig.1`),
SHA256SUMS and a stable Homebrew formula. It creates a GitHub release and updates the same repository's
Formula/git-twig.rb. Native CoW supports macOS/APFS and Linux on Btrfs or
reflink-enabled XFS; unsupported filesystems delegate to Git. macOS binaries are
not signed/notarized yet.

Homebrew needs the explicit remote because the repository is not named
homebrew-git-twig:

```sh
brew tap leostera/git-twig https://github.com/leostera/git-twig
brew install leostera/git-twig/git-twig
```

Use --HEAD only when intentionally installing development source. Both source
and stable Homebrew installs include the manual for `git twig --help`. Cargo does not install manuals;
its users can use `git twig -h` or `git-twig --help`.

Crates.io publishing is a separate owner-approved action:
publish only `git-twig` with `cargo publish -p git-twig --locked`. Its library and
CLI are packaged together; there is no separate core crate or publication order.
Use cargo login locally, never paste credentials into chat. Do not publish
`twig-bench`.

Workflow YAML and formula generation can be checked locally; actual hosted builds,
downloaded binary installs and registry publication still require end-to-end
validation after repository publication.
