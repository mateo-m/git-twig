//! Diagnostic profiles of real CLI operations; not uninstrumented benchmark timings.
use anyhow::{Context, Result, ensure};
use serde::Serialize;
use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

unsafe extern "C" {
    fn getppid() -> i32;
}
fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
}
fn redact(value: &str, project: &str) -> String {
    let value = value.replace(project, "$REPO");
    match env::var("HOME") {
        Ok(home) => value.replace(&home, "$HOME"),
        Err(_) => value,
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GitCall {
    schema_version: u32,
    pid: u32,
    git_pid: u32,
    parent_pid: i32,
    started_at_unix_ms: u128,
    elapsed_ms: f64,
    argv: Vec<String>,
    cwd: String,
    exit_code: Option<i32>,
    index_file: Option<String>,
    work_tree: Option<String>,
}
fn git_wrapper() -> Result<i32> {
    let real = env::var_os("TWIG_PROFILE_REAL_GIT").context("real Git")?;
    let directory =
        PathBuf::from(env::var_os("TWIG_PROFILE_DIRECTORY").context("profile directory")?);
    let project = env::var("TWIG_PROFILE_PROJECT")?;
    let args = env::args_os().skip(1).collect::<Vec<_>>();
    let started_at_unix_ms = now_ms();
    let start = Instant::now();
    // POSIX getppid has no pointer arguments and cannot fail.
    let parent_pid = unsafe { getppid() };
    let mut child = Command::new(real).args(&args).spawn()?;
    let git_pid = child.id();
    let status = child.wait()?;
    let call = GitCall {
        schema_version: 1,
        pid: std::process::id(),
        git_pid,
        parent_pid,
        started_at_unix_ms,
        elapsed_ms: start.elapsed().as_secs_f64() * 1000.0,
        argv: args
            .iter()
            .map(|a| redact(&a.to_string_lossy(), &project))
            .collect(),
        cwd: redact(&env::current_dir()?.to_string_lossy(), &project),
        exit_code: status.code(),
        index_file: env::var("GIT_INDEX_FILE")
            .ok()
            .map(|v| redact(&v, &project)),
        work_tree: env::var("GIT_WORK_TREE").ok().map(|v| redact(&v, &project)),
    };
    let path = directory
        .join("git-calls")
        .join(format!("{}-{}.json", call.pid, call.started_at_unix_ms));
    fs::write(path, serde_json::to_vec_pretty(&call)?)?;
    Ok(status.code().unwrap_or(1))
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Process {
    pid: u32,
    parent_pid: u32,
    command: String,
    first_observed_at_unix_ms: u128,
    last_observed_at_unix_ms: u128,
    sample_file: Option<String>,
}
fn processes(root: u32) -> Result<Vec<Process>> {
    let output = Command::new("ps")
        .args(["-axo", "pid=,ppid=,command="])
        .output()?;
    ensure!(output.status.success(), "ps failed");
    let mut all = Vec::new();
    for line in std::str::from_utf8(&output.stdout)?.lines() {
        let mut fields = line.split_whitespace();
        let Some(pid) = fields.next().and_then(|v| v.parse::<u32>().ok()) else {
            continue;
        };
        let parent_pid = fields.next().context("ps parent")?.parse()?;
        all.push(Process {
            pid,
            parent_pid,
            command: fields.collect::<Vec<_>>().join(" "),
            first_observed_at_unix_ms: now_ms(),
            last_observed_at_unix_ms: now_ms(),
            sample_file: None,
        });
    }
    let mut owned = HashSet::from([root]);
    loop {
        let old = owned.len();
        for p in &all {
            if owned.contains(&p.parent_pid) {
                owned.insert(p.pid);
            }
        }
        if owned.len() == old {
            break;
        }
    }
    Ok(all
        .into_iter()
        .filter(|p| owned.contains(&p.pid) && p.pid != root)
        .collect())
}
struct Sampler {
    pid: u32,
    child: Child,
}
fn run() -> Result<()> {
    ensure!(cfg!(target_os = "macos"), "profiling requires macOS");
    let mut args = env::args().skip(1);
    let mut options = HashMap::new();
    while let Some(key) = args.next() {
        ensure!(
            [
                "--source",
                "--revision",
                "--candidate",
                "--harness",
                "--output",
                "--repeats"
            ]
            .contains(&key.as_str()),
            "unknown option {key}"
        );
        let value = args.next().context("missing option value")?;
        ensure!(options.insert(key, value).is_none(), "duplicate option");
    }
    let required = |key: &str| options.get(key).with_context(|| format!("required {key}"));
    let source = fs::canonicalize(required("--source")?)?;
    let candidate = fs::canonicalize(required("--candidate")?)?;
    let harness = fs::canonicalize(required("--harness")?)?;
    let revision = required("--revision")?;
    ensure!(!revision.starts_with('-'), "invalid revision");
    let repeats = options
        .get("--repeats")
        .map(|v| v.parse::<usize>())
        .transpose()?
        .unwrap_or(3);
    ensure!(repeats > 0, "repeats must be positive");
    let output = PathBuf::from(required("--output")?);
    fs::create_dir_all(output.parent().context("output parent")?)?;
    fs::create_dir(&output).context("profile output must not already exist")?;
    let output = fs::canonicalize(output)?;
    for name in ["bin", "git-calls", "samples", "trace2"] {
        fs::create_dir(output.join(name))?;
    }
    let project = env::current_dir()?;
    let git_path = Command::new("which").arg("git").output()?;
    ensure!(git_path.status.success(), "Git missing");
    let git = fs::canonicalize(std::str::from_utf8(&git_path.stdout)?.trim())?;
    let exec = Command::new(&git).arg("--exec-path").output()?;
    ensure!(exec.status.success(), "Git exec-path failed");
    let exec = std::str::from_utf8(&exec.stdout)?.trim();
    fs::copy(env::current_exe()?, output.join("bin/git"))?;
    let log = fs::File::create(output.join("run.log"))?;
    let mut child = Command::new(&harness)
        .args([
            "--repo",
            &format!("linux={}", source.display()),
            "--revision",
            &format!("linux={revision}"),
            "--candidate",
        ])
        .arg(&candidate)
        .args([
            "--filesystem",
            "case-sensitive-image",
            "--image-gib",
            "32",
            "--concurrency",
            "1",
            "--worktrees",
            "1",
            "--repeats",
            &repeats.to_string(),
            "--warmup",
            "true",
            "--output",
        ])
        .arg(output.join("results.json"))
        .env(
            "PATH",
            format!("{}:{}", output.join("bin").display(), env::var("PATH")?),
        )
        .env("GIT_EXEC_PATH", exec)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("TWIG_PROFILE_REAL_GIT", &git)
        .env("TWIG_PROFILE_DIRECTORY", &output)
        .env("TWIG_PROFILE_PROJECT", &project)
        .env("GIT_TRACE2_EVENT", output.join("trace2"))
        .stdout(log.try_clone()?)
        .stderr(log)
        .spawn()?;
    let mut seen: HashMap<u32, Process> = HashMap::new();
    let mut samplers = Vec::new();
    let started = now_ms();
    let child_id = child.id();
    loop {
        for p in processes(child_id)? {
            let workload = p.command.contains("/w-")
                || is_first_status(&p.command)
                || seen_ancestor_workload(p.parent_pid, &seen);
            let current = seen.entry(p.pid).or_insert(p.clone());
            current.last_observed_at_unix_ms = p.last_observed_at_unix_ms;
            if !p.command.starts_with('(') {
                current.command.clone_from(&p.command);
            }
            let target = p
                .command
                .starts_with(&candidate.to_string_lossy().to_string())
                || p.command.starts_with(&git.to_string_lossy().to_string());
            if workload
                && target
                && current.sample_file.is_none()
                && now_ms() - current.first_observed_at_unix_ms >= 500
            {
                let file = output.join("samples").join(format!("{}.txt", p.pid));
                let stderr =
                    fs::File::create(output.join("samples").join(format!("{}.stderr", p.pid)))?;
                let sampler = Command::new("sample")
                    .args([
                        p.pid.to_string(),
                        "120".into(),
                        "5".into(),
                        "-mayDie".into(),
                        "-file".into(),
                        file.to_string_lossy().into_owned(),
                    ])
                    .stdout(Stdio::null())
                    .stderr(stderr)
                    .spawn()?;
                current.sample_file = Some(format!("samples/{}.txt", p.pid));
                samplers.push(Sampler {
                    pid: p.pid,
                    child: sampler,
                });
            }
        }
        let status = child.try_wait()?;
        let mut observations = seen.values().cloned().collect::<Vec<_>>();
        observations.sort_by_key(|p| p.pid);
        for p in &mut observations {
            p.command = redact(&p.command, &project.to_string_lossy());
        }
        let state = serde_json::json!({"schemaVersion":1,"phase":if status.is_some(){"finished"}else{"running"},"harnessPid":child_id,"startedAtUnixMs":started,"updatedAtUnixMs":now_ms(),"exitCode":status.and_then(|s|s.code()),"repeats":repeats,"processes":observations,"notes":["Instrumented diagnostic timings, not a replacement for the published uninstrumented benchmarks.","Frozen release candidate; Git wrapper plus Trace2 and 5 ms CPU stack sampling add observer overhead."]});
        let temp = output.join("state.tmp");
        fs::write(&temp, serde_json::to_vec_pretty(&state)?)?;
        fs::rename(temp, output.join("state.json"))?;
        if let Some(status) = status {
            let mut sampling = Vec::new();
            for mut sampler in samplers {
                let code = sampler.child.wait()?;
                eprintln!("sample {}: {code}", sampler.pid);
                sampling.push(serde_json::json!({"pid":sampler.pid,"exitCode":code.code()}));
            }
            fs::write(
                output.join("sampling.json"),
                serde_json::to_vec_pretty(&sampling)?,
            )?;
            ensure!(status.success(), "profile harness failed: {status}");
            break;
        }
        thread::sleep(Duration::from_millis(250));
    }
    Ok(())
}
fn is_first_status(command: &str) -> bool {
    command.split_whitespace().collect::<Vec<_>>().ends_with(&[
        "status",
        "--porcelain",
        "--untracked-files=no",
    ])
}
fn seen_ancestor_workload(mut pid: u32, seen: &HashMap<u32, Process>) -> bool {
    for _ in 0..32 {
        let Some(parent) = seen.get(&pid) else {
            return false;
        };
        if parent.command.contains("/w-") {
            return true;
        }
        pid = parent.parent_pid;
    }
    false
}
pub(crate) fn main() -> Result<()> {
    if env::args_os()
        .next()
        .is_some_and(|arg| Path::new(&arg).file_name() == Some(std::ffi::OsStr::new("git")))
    {
        std::process::exit(git_wrapper()?);
    }
    run()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn redacts_project_paths() {
        assert_eq!(redact("/project/target/x", "/project"), "$REPO/target/x");
    }
    #[test]
    fn status_is_detected_without_a_worktree_in_argv() {
        assert!(is_first_status(
            "/opt/git status --porcelain --untracked-files=no"
        ));
        assert!(!is_first_status("/opt/git config --get status"));
    }
    #[test]
    fn missing_ancestor_is_not_workload() {
        assert!(!seen_ancestor_workload(1, &HashMap::new()));
    }
}
