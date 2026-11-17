#!/usr/bin/env bun
/** Issue #5: actual CLI before/after + stock Git, same full-history pinned input. */
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, openSync, closeSync, readFileSync, writeFileSync, renameSync, statfsSync } from "node:fs";
import { resolve } from "node:path";
const [runArg, inputArg, beforeArg, afterArg, harnessArg] = process.argv.slice(2);
if (![runArg,inputArg,beforeArg,afterArg,harnessArg].every(Boolean)) throw new Error("RUN INPUT BEFORE AFTER HARNESS required");
const run = resolve(runArg!), input = resolve(inputArg!), before = resolve(beforeArg!), after = resolve(afterArg!), harness = resolve(harnessArg!);
if (existsSync(run)) throw new Error("Never overwrite a run");
mkdirSync(run, { recursive: true });
const capture = (argv: string[]) => {
  const p = Bun.spawnSync(argv, { stdout: "pipe", stderr: "pipe" });
  if (p.exitCode) throw new Error(new TextDecoder().decode(p.stderr));
  return new TextDecoder().decode(p.stdout).trim();
};
if (capture(["git","status","--porcelain"])) throw new Error("Commit benchmark source before measurement");
const hash = (file: string) => createHash("sha256").update(readFileSync(file)).digest("hex");
const pin = "430261138aa08539cd598b9269bfe8f82fd49b78";
capture(["git","-C",input,"cat-file","-e",`${pin}^{commit}`]);
const manifest = {
  schemaVersion: 1, generatedAt: new Date().toISOString(), sourceCommit: capture(["git","rev-parse","HEAD"]), dirty: false,
  inputCommit: pin, git: capture(["git","--version"]), platform: capture(["sw_vers"]), cpu: capture(["sysctl","-n","machdep.cpu.brand_string"]),
  inputObjects: capture(["git","-C",input,"count-objects","-vH"]),
  binaries: { before: hash(before), after: hash(after), harness: hash(harness) },
  config: { worktrees: 1, concurrency: 1, repeats: 3, warmup: true, imageGiB: 8, minimumHostFreeGiB: 128 },
  notes: ["Same full-history input and commit in every arm; original gitattributes preserved. Warm caches, shared Mac, no competing builds/tests.",
    "Before=pre-optimization raw-cat-file verifier; after=bounded parallel raw hash-object verifier. Stock Git appears in the after suites. No stat-cache substitution.",
    "Creation excludes subsequent full raw content/mode/link verification and private-write/source-isolation checks. Host timing and image allocation are separate.",
    "Original reporter's 20x result has not been reproduced on this Mac; this campaign must not be presented as reproducing it. Profiles are separate instrumented evidence, not these measurements."]
};
writeFileSync(resolve(run,"manifest.json"),JSON.stringify(manifest,null,2)+"\n");
const jobs = [
  { name: "before-host", candidate: before, filesystem: "host", backend: "twig" },
  { name: "after-host", candidate: after, filesystem: "host", backend: null },
  { name: "before-images", candidate: before, filesystem: "case-sensitive-image", backend: "twig" },
  { name: "after-images", candidate: after, filesystem: "case-sensitive-image", backend: null },
];
const completed: unknown[] = [];
let phase = "running", current = "", pid: number | null = null;
const save = (error?: string) => {
  writeFileSync(resolve(run,"state.tmp"),JSON.stringify({schemaVersion:1,phase,current,pid,supervisorPid:process.pid,completed,updatedAt:new Date().toISOString(),error},null,2)+"\n");
  renameSync(resolve(run,"state.tmp"),resolve(run,"state.json"));
};
try {
  for (const job of jobs) {
    current = job.name;
    const fs = statfsSync(run); if (fs.bavail*fs.bsize < 128*2**30) throw new Error("free-space guard");
    const args = [harness,"--repo",`pulumi=${input}`,"--revision",`pulumi=${pin}`,"--candidate",job.candidate,"--filesystem",job.filesystem,"--image-gib","8","--worktrees","1","--concurrency","1","--repeats","3","--warmup","true","--output",resolve(run,job.name+".json")];
    if (job.backend) args.push("--backend",job.backend);
    console.log(new Date().toISOString(),"START",current);
    const log = openSync(resolve(run,job.name+".log"),"wx");
    const child = Bun.spawn(args,{stdin:"ignore",stdout:log,stderr:log,env:{...process.env,GIT_CONFIG_NOSYSTEM:"1",GIT_CONFIG_GLOBAL:"/dev/null",GIT_ATTR_NOSYSTEM:"1",GIT_TERMINAL_PROMPT:"0"}});
    pid=child.pid;save();const exitCode=await child.exited;closeSync(log);
    const data=JSON.parse(readFileSync(resolve(run,job.name+".json"),"utf8"));
    completed.push({job:job.name,exitCode,cohorts:data.cohorts.length,failures:data.failures});save();
    if(exitCode || data.failures.length || data.cohorts.some((c:any)=>c.failed || !c.sourceUnchanged || c.operations.some((o:any)=>!o.verified || o.error || (c.backend==="twig"&&!o.nativeCow)))) throw new Error("verification/measurement failure: "+current);
    console.log(new Date().toISOString(),"FINISH",current);
  }
  phase="finished";pid=null;save();
} catch(error) {phase="failed";save(String(error));throw error;}
