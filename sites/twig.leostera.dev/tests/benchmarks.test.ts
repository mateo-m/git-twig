import { expect, test } from "bun:test";
import { loadBenchmarks, summarize } from "../scripts/benchmarks";

test("uses verified checked-in allocation measurements", () => {
  const data = loadBenchmarks();
  expect(data.map((d) => d.name)).toEqual([
    "Linux kernel",
    "Pulumi docs",
    "OpenClaw",
    "Axo",
    "git-twig",
    "Workers starter",
  ]);
  expect(data[0]!.gitBytes).toBeGreaterThan(data[0]!.twigBytes);
  expect(
    data.every((d) =>
      d.source.startsWith("https://github.com/leostera/git-twig/"),
    ),
  ).toBe(true);
});
test("accepts clean schema-v3 APFS runs and rejects dirty or failed runs", () => {
  const operation = { verified: true, exitCode: 0, error: null, nativeCow: true };
  const row = {
    repository: "fixture", sourceCommit: "commit", sourceTree: "tree",
    sourceUnchanged: true, succeeded: 1, failed: 0, operations: [operation],
  };
  const raw = {
    schemaVersion: 3, filesystem: "case-sensitive-image", dirty: false,
    failures: [], concurrency: 1, worktreesPerCohort: 1,
    cohorts: [
      { ...row, backend: "git", growthAfterStatusBytes: 100 },
      { ...row, backend: "twig", growthAfterStatusBytes: 20 },
    ],
  };
  expect(summarize(raw, "fixture", "Fixture", "").twigBytes).toBe(20);
  expect(() => summarize({ ...raw, dirty: true }, "fixture", "Fixture", "")).toThrow();
  expect(() => summarize({ ...raw, failures: ["interrupted"] }, "fixture", "Fixture", "")).toThrow();
});

test("does not present missing cohorts as zero space", () => {
  expect(() =>
    summarize(
      {
        schemaVersion: 2,
        filesystem: "isolated-image",
        concurrency: 1,
        cohorts: [],
      },
      "linux",
      "Linux",
      "",
    ),
  ).toThrow("Missing paired");
});
