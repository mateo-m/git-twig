import { readFileSync } from "node:fs";
import { z } from "zod";

const operation = z.object({
  verified: z.literal(true),
  exitCode: z.literal(0),
  error: z.null(),
  nativeCow: z.boolean(),
});
const cohort = z.object({
  repository: z.string(),
  backend: z.enum(["git", "twig"]),
  sourceCommit: z.string(),
  sourceTree: z.string(),
  sourceUnchanged: z.literal(true),
  succeeded: z.number().int().positive(),
  failed: z.literal(0),
  growthAfterStatusBytes: z.number().nonnegative(),
  operations: z.array(operation),
});
const suiteFields = {
  concurrency: z.number().int().positive(),
  worktreesPerCohort: z.number().int().positive().optional(),
  cohorts: z.array(z.unknown()),
};
const suite = z.discriminatedUnion("schemaVersion", [
  // The original isolated-image harness had no filesystem selector.
  z.object({
    ...suiteFields,
    schemaVersion: z.literal(1),
    filesystem: z.undefined().optional(),
  }),
  z.object({
    ...suiteFields,
    schemaVersion: z.literal(2),
    filesystem: z.enum(["isolated-image", "case-sensitive-image"]),
  }),
  z.object({
    ...suiteFields,
    schemaVersion: z.literal(3),
    filesystem: z.enum(["isolated-image", "case-sensitive-image"]),
    dirty: z.literal(false),
    failures: z.array(z.string()).length(0),
  }),
]);
const header = z.object({ repository: z.string() });

export interface DiskComparison {
  name: string;
  worktrees: number;
  concurrency: number;
  rounds: number;
  gitBytes: number;
  twigBytes: number;
  source: string;
}

function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  return sorted.length % 2
    ? sorted[middle]!
    : (sorted[middle - 1]! + sorted[middle]!) / 2;
}

export function summarize(
  raw: unknown,
  repository: string,
  name: string,
  source: string,
): DiskComparison {
  const run = suite.parse(raw);
  const rows = run.cohorts
    .filter((value) => header.parse(value).repository === repository)
    .map((value) => cohort.parse(value));
  const git = rows.filter((row) => row.backend === "git");
  const twig = rows.filter((row) => row.backend === "twig");
  if (!git.length || git.length !== twig.length)
    throw new Error("Missing paired allocation cohorts");
  const count = run.worktreesPerCohort ?? run.concurrency;
  for (const row of rows) {
    if (row.succeeded !== count || row.operations.length !== count)
      throw new Error("Incomplete cohort");
    if (
      row.sourceTree !== rows[0]!.sourceTree ||
      row.sourceCommit !== rows[0]!.sourceCommit
    )
      throw new Error("Different source trees");
    if (row.backend === "twig" && row.operations.some((op) => !op.nativeCow))
      throw new Error("Not native CoW");
  }
  const gitBytes = median(git.map((row) => row.growthAfterStatusBytes));
  const twigBytes = median(twig.map((row) => row.growthAfterStatusBytes));
  if (!gitBytes || !twigBytes)
    throw new Error("Allocation ratio requires positive measurements");
  return {
    name,
    worktrees: count,
    concurrency: run.concurrency,
    rounds: git.length,
    gitBytes,
    twigBytes,
    source,
  };
}

export const sources = [
  {
    name: "Linux kernel",
    repository: "linux",
    file: "2026-11-17-final/linux-images.json",
  },
  { name: "Pulumi docs", repository: "pulumi", file: "2026-11-17-issue5/after-images.json" },
  { name: "OpenClaw", repository: "openclaw", file: "2026-11-17-final/openclaw-images.json" },
  { name: "Axo", repository: "axo", file: "2026-11-17-final/axo-images.json" },
  { name: "git-twig", repository: "twig", file: "2026-11-17-final/twig-images.json" },
  { name: "Workers starter", repository: "workers-starter", file: "2026-11-17-final/workers-starter-images.json" },
];

export function benchmarkFile(file: string): URL {
  return new URL(`../../../bench/results/${file}`, import.meta.url);
}

export function loadBenchmarks(): DiskComparison[] {
  return sources.map(({ name, repository, file }) => {
    const path = benchmarkFile(file);
    return summarize(
      JSON.parse(readFileSync(path, "utf8")),
      repository,
      name,
      `https://github.com/leostera/git-twig/blob/main/bench/results/${file}`,
    );
  });
}
