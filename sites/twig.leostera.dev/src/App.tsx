import { useEffect, useState } from "react";
import { Button } from "@ui-sdk/react/button";
import benchmarks from "virtual:twig-benchmarks";
import type { DiskComparison } from "../scripts/benchmarks";
import { copyText } from "./clipboard";

export const onboardPrompt =
  "Fetch and execute the appropriate instructions to set me up for Twig from https://twig.leostera.dev/llms.txt";

function AgentMarks() {
  return (
    <svg
      className="agent-marks"
      width="132"
      height="32"
      viewBox="0 0 132 32"
      fill="none"
      aria-hidden="true"
    >
      <g stroke="#d47b5e" strokeWidth="2.2" strokeLinecap="round">
        {Array.from({ length: 12 }, (_, i) => (
          <path key={i} d="M15 3v7" transform={`rotate(${i * 30} 15 16)`} />
        ))}
      </g>
      <path
        d="M49 3c3-2 6 0 7 3 4-1 7 2 7 6 3 2 3 6 0 8 0 5-4 8-8 7-3 3-7 2-9-1-5 1-8-3-7-7-3-3-2-7 1-9-1-4 3-7 6-6l3-1Z"
        fill="#171918"
      />
      <path
        d="m46 12 3 4-3 4m7 0h5"
        stroke="white"
        strokeWidth="1.8"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      <path d="m82 2 14 8v15l-14 7-14-8V10l14-8Z" fill="#171918" />
      <path d="m69 10 13 7 13-7M82 17v14" stroke="white" strokeWidth="1.8" />
      <path d="M105 3h24v27h-24z" fill="#171918" />
      <path d="M111 9h12v15h-12z" fill="white" />
      <path d="M111 9h12v5h-12z" fill="#b5b7b3" />
    </svg>
  );
}

function disk(bytes: number): string {
  return bytes >= 1024 ** 3
    ? `${(bytes / 1024 ** 3).toFixed(1)} GiB`
    : `${(bytes / 1024 ** 2).toFixed(bytes < 10 * 1024 ** 2 ? 1 : 0)} MiB`;
}
function DiskChart({ data }: { data: DiskComparison[] }) {
  const scale = Math.max(
    1,
    ...data.map((repo) => repo.twigBytes / repo.gitBytes),
  );
  return (
    <figure className="disk-chart">
      <figcaption>
        <h2>Disk use</h2>
        <span>Git = 100%</span>
      </figcaption>
      {data.map((repo) => {
        const saved = (1 - repo.twigBytes / repo.gitBytes) * 100;
        const rows = [
          { label: "git worktree", bytes: repo.gitBytes, kind: "git" },
          { label: "git twig", bytes: repo.twigBytes, kind: "twig" },
        ];
        return (
          <section className="repo-group" key={repo.name}>
            <div className="repo-heading">
              <h3>
                <a href={repo.source}>{repo.name}</a>
                {repo.worktrees > 1 && (
                  <small className="cohort-count" title={`${repo.worktrees} sequential worktrees`}>×{repo.worktrees}</small>
                )}
              </h3>
              <span className={saved >= 0 ? "less" : "more"}>
                {Math.abs(saved).toFixed(1)}% {saved >= 0 ? "less" : "more"}
              </span>
            </div>
            <p className="sr-only">
              {repo.worktrees} worktrees, {repo.concurrency} in flight.
              Additional APFS allocation after first status; {repo.rounds} local
              rounds. Bar lengths are relative to Git for each repo, not
              absolute across repos.
            </p>
            <div className="bars">
              {rows.map(({ label, bytes, kind }) => (
                <div className="bar-row" key={label}>
                  <code>{label}</code>
                  <div className="bar-track">
                    <div
                      className={`bar ${kind}`}
                      style={{
                        width: `${(bytes / repo.gitBytes / scale) * 100}%`,
                      }}
                    />
                  </div>
                  <span>{disk(bytes)}</span>
                </div>
              ))}
            </div>
          </section>
        );
      })}
    </figure>
  );
}

export default function App() {
  const [copy, setCopy] = useState<"ready" | "copied" | "failed">("ready");
  useEffect(() => {
    if (copy === "ready") return;
    const timeout = setTimeout(() => setCopy("ready"), 3000);
    return () => clearTimeout(timeout);
  }, [copy]);
  async function onboard() {
    try {
      await copyText(onboardPrompt);
      setCopy("copied");
    } catch {
      setCopy("failed");
    }
  }
  return (
    <div className="page">
      <a
        className="github-link"
        href="https://github.com/leostera/git-twig"
        aria-label="Twig on GitHub"
        title="Twig on GitHub"
      >
        <svg width="24" height="24" viewBox="0 0 16 16" fill="currentColor" aria-hidden="true">
          <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82A7.65 7.65 0 0 1 8 4.73c.68 0 1.36.09 2 .27 1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.013 8.013 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
        </svg>
      </a>
      <main>
        <section className="hero" aria-labelledby="title">
          <div className="twig-mark">
            <img src="/twig.svg" width="96" height="96" alt="" />
          </div>
          <h1 id="title">twig</h1>
          <p className="tagline">lightweight git-worktrees for agents</p>
          <p className="description">
            With twig, your agents can create cheap lightweight worktrees and use
            up to 97% less space on disk. Imagine all the worktrees you could now
            make!
          </p>
          <div className="onboard-action">
            <Button className="onboard" onClick={onboard}>
              <span>Onboard your agent to Twig</span>
              <AgentMarks />
            </Button>
            <p className={copy === "ready" ? "sr-only" : "copy-toast"} role="status">
              {copy === "copied"
                ? "Setup prompt copied"
                : copy === "failed"
                  ? "Couldn’t copy. Please try again."
                  : ""}
            </p>
          </div>
        </section>
        <section className="benchmarks" aria-label="Disk usage comparison">
          <DiskChart data={benchmarks} />
        </section>
      </main>
      <footer>
        made with {"<3"} by <a href="https://github.com/leostera">@leostera</a>{" "}
        at <a href="https://cloudflare.com">@cloudflare</a>
      </footer>
    </div>
  );
}
