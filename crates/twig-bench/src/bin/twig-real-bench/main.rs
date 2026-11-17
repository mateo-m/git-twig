#[cfg(unix)]
mod unix;

#[cfg(unix)]
fn main() -> anyhow::Result<()> {
    unix::main()
}

#[cfg(not(unix))]
fn main() {
    eprintln!("twig-real-bench needs macOS or Linux");
    std::process::exit(1);
}
