//! Fallbacks must explain their concrete boundary without changing Git's outcome.
use super::*;

fn reason(output: &Output, expected: &str) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("git-twig: stock Git:"), "{stderr}");
    assert!(stderr.contains(expected), "{stderr}");
    assert!(!stderr.contains("outside the CoW fast path"), "{stderr}");
    assert!(!stderr.contains("CoW: verified tracked files"), "{stderr}");
}

#[test]
fn attributes_name_the_path_key_and_literal_value() -> Result<()> {
    for value in ["unset", "unspecified", "upper"] {
        let f = Fixture::new()?;
        fs::write(
            f.source.join(".gitattributes"),
            format!("hello.txt filter={value}\n"),
        )?;
        git(&f.source, &["add", "."])?;
        git(&f.source, &["commit", "-m", "opaque attribute"])?;
        let output = checked(f.add()?)?;
        reason(
            &output,
            &format!("checkout attribute filter=\"{value}\" on \"hello.txt\""),
        );
        assert_eq!(fs::read(f.destination().join("hello.txt"))?, b"hello\n");
        clean(&f.destination())?;
    }
    Ok(())
}

#[test]
fn environment_overrides_are_named_and_trace_is_opt_in() -> Result<()> {
    let f = Fixture::new()?;
    let output = checked(
        command(env!("CARGO_BIN_EXE_git-twig"), &f.source)
            .args(["new", "--detach", "../feature"])
            .env("GIT_ATTR_SOURCE", "HEAD")
            .output()?,
    )?;
    reason(
        &output,
        "environment override GIT_ATTR_SOURCE requires stock Git",
    );
    clean(&f.destination())?;

    let f = Fixture::new()?;
    git(&f.source, &["config", "core.symlinks", "false"])?;
    let output = checked(
        command(env!("CARGO_BIN_EXE_git-twig"), &f.source)
            .args(["new", "--detach", "../feature"])
            .env_remove("TWIG_TRACE")
            .output()?,
    )?;
    assert!(!String::from_utf8_lossy(&output.stderr).contains("git-twig: "));
    clean(&f.destination())?;
    Ok(())
}

#[cfg(unix)]
#[test]
fn attribute_paths_are_escaped_in_diagnostics() -> Result<()> {
    let f = Fixture::new()?;
    fs::write(f.source.join("odd\tname\n.txt"), "hello\n")?;
    fs::write(
        f.source.join(".gitattributes"),
        "\"odd\\tname\\n.txt\" filter=unset\n",
    )?;
    git(&f.source, &["add", "."])?;
    git(&f.source, &["commit", "-m", "escaped attribute path"])?;
    let output = checked(f.add()?)?;
    reason(
        &output,
        r#"checkout attribute filter="unset" on "odd\tname\n.txt""#,
    );
    assert_eq!(
        fs::read(f.destination().join("odd\tname\n.txt"))?,
        b"hello\n"
    );
    clean(&f.destination())?;
    Ok(())
}

#[test]
fn unsupported_creation_syntax_explains_the_supported_subset() -> Result<()> {
    let f = Fixture::new()?;
    let output = checked(f.twig(&["new", "--no-checkout", "--detach", "../feature"])?)?;
    reason(&output, "creation options require Git");
    assert!(!f.destination().join("hello.txt").exists());
    Ok(())
}

#[test]
fn revision_and_sparse_checkout_have_distinct_reasons() -> Result<()> {
    let f = Fixture::new()?;
    fs::write(f.source.join("hello.txt"), "second\n")?;
    git(&f.source, &["commit", "-am", "second"])?;
    let output = checked(f.twig(&["new", "--detach", "../feature", "HEAD~1"])?)?;
    reason(&output, "requested revision differs from source HEAD");
    assert_eq!(fs::read(f.destination().join("hello.txt"))?, b"hello\n");
    let f = Fixture::new()?;
    git(&f.source, &["sparse-checkout", "init", "--cone"])?;
    let output = checked(f.add()?)?;
    reason(&output, "sparse checkout requires stock Git");
    Ok(())
}

#[test]
fn destination_reasons_preserve_existing_paths_and_git_parent_creation() -> Result<()> {
    let f = Fixture::new()?;
    fs::create_dir(f.destination())?;
    fs::write(f.destination().join("keep"), "untouched")?;
    let output = f.add()?;
    assert!(!output.status.success());
    reason(&output, "destination already exists");
    assert_eq!(fs::read(f.destination().join("keep"))?, b"untouched");
    let f = Fixture::new()?;
    let output = checked(f.twig(&["new", "--detach", "../missing/feature"])?)?;
    reason(&output, "destination parent does not exist");
    clean(&f.root.join("missing/feature"))?;
    Ok(())
}

#[test]
fn hooks_are_identified() -> Result<()> {
    for event in ["reference-transaction", "post-index-change"] {
        let f = Fixture::new()?;
        fs::write(
            f.source.join(".git/hooks").join(event),
            "#!/bin/sh\nexit 0\n",
        )?;
        let output = checked(f.add()?)?;
        reason(&output, &format!("{event} hook requires stock Git"));
        clean(&f.destination())?;
    }
    Ok(())
}

#[test]
fn gitlinks_are_reported_as_submodules() -> Result<()> {
    let f = Fixture::new()?;
    let oid = String::from_utf8(git(&f.source, &["rev-parse", "HEAD"])?.stdout)?;
    git(
        &f.source,
        &[
            "update-index",
            "--add",
            "--cacheinfo",
            "160000",
            oid.trim(),
            "sub",
        ],
    )?;
    git(&f.source, &["commit", "-m", "gitlink"])?;
    let output = checked(f.add()?)?;
    reason(&output, "submodules require stock Git");
    clean(&f.destination())?;
    Ok(())
}
