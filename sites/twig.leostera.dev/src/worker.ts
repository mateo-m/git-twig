import { Hono } from "hono";
import skill from "../../../crates/git-twig/skills/git-twig/SKILL.md?raw";

export const onboarding = `# Twig — git-worktrees for agents

Twig creates ordinary linked Git worktrees using native copy-on-write.
Native cloning needs macOS/APFS, Linux on Btrfs or reflink-enabled XFS, or Windows on ReFS.
This is a prototype, not a sandbox.

## Install

Check whether git twig --version, git twig skill and git twig new --help work.
These instructions require v0.3.0 or newer; add is no longer a Twig command. Otherwise,
choose one installation method. Git 2.40+ is required.

With Rust 1.95+:

\`\`\`sh
cargo install git-twig --locked
git twig --version
\`\`\`

Or with Homebrew:

\`\`\`sh
brew tap leostera/git-twig https://github.com/leostera/git-twig
brew install leostera/git-twig/git-twig
git twig --version
\`\`\`

Version 0.3.0 is published on crates.io and as GitHub release binaries. Use the
published crate or Homebrew tap; do not invent a curl-to-shell installer.

For an older release, upgrade with the same manager: cargo install git-twig --locked,
or brew update followed by brew upgrade leostera/git-twig/git-twig. For an existing
Homebrew HEAD install, use brew upgrade --fetch-HEAD leostera/git-twig/git-twig.
Do not leave multiple versions shadowing each other in PATH. Verify that
git twig --version reports at least 0.3.0 and git twig new --help works.

## Use

Read the bundled skill with git twig skill. Its contents follow.

${skill}
`;

const app = new Hono<{ Bindings: { ASSETS: { fetch: typeof fetch } } }>();
app.get("/llms.txt", (c) => {
  c.header("Cache-Control", "public, max-age=300");
  c.header("X-Content-Type-Options", "nosniff");
  return c.text(onboarding);
});
app.all("*", (c) => c.env.ASSETS.fetch(c.req.raw));
export default app;
