//! `cargo xtask slow-tests [N]`: the workspace's N slowest tests (default 20), to see where test
//! time goes.
//!
//! The tests are built as `cargo test` builds them, then each test binary runs from its package's
//! folder with libtest's per-test timing (`-Z unstable-options --report-time`). That option is
//! nightly-only unless `RUSTC_BOOTSTRAP` is set; it is set on the test binaries only, never on
//! cargo, whose builds would otherwise not match `cargo test`'s and rebuild the workspace.
//! Times are taken with each binary's tests running side by side, as in `cargo test`.

use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// A test binary and the package folder it runs from.
struct Binary {
    exe: PathBuf,
    dir: PathBuf,
}

pub fn run(args: &[&str]) -> Result<(), String> {
    let n = match args {
        [] => 20,
        [n] => n.parse::<usize>().map_err(|_| format!("slow-tests: `{n}` is not a number"))?,
        _ => return Err("usage: cargo xtask slow-tests [N]".into()),
    };
    let binaries = build()?;
    let mut times: Vec<(String, f64)> = Vec::new();
    for b in &binaries {
        let name = b.exe.file_stem().map(|s| label(&s.to_string_lossy())).unwrap_or_default();
        eprintln!("== {name}");
        let out = Command::new(&b.exe)
            .current_dir(&b.dir)
            .env("RUSTC_BOOTSTRAP", "1")
            .env("CARGO_MANIFEST_DIR", &b.dir)
            .args(["-Z", "unstable-options", "--report-time"])
            .output()
            .map_err(|e| format!("{}: {e}", b.exe.display()))?;
        times.extend(parse(&String::from_utf8_lossy(&out.stdout)).into_iter().map(|(test, secs)| (format!("{name} {test}"), secs)));
    }
    times.sort_by(|a, b| b.1.total_cmp(&a.1));
    let total: f64 = times.iter().map(|t| t.1).sum();
    println!("\n{} tests, {:.0} s in all; the {} slowest:", times.len(), total, n.min(times.len()));
    for (test, secs) in times.iter().take(n) {
        println!("{secs:>8.2} s  {test}");
    }
    Ok(())
}

/// Build every test binary as `cargo test --workspace` does, and list them.
fn build() -> Result<Vec<Binary>, String> {
    let out = crate::cargo()
        .args(["test", "--workspace", "--no-run", "--message-format=json-render-diagnostics"])
        .stderr(std::process::Stdio::inherit())
        .output()
        .map_err(|e| format!("cargo test --no-run: {e}"))?;
    if !out.status.success() {
        return Err("cargo test --no-run failed".into());
    }
    Ok(binaries(&String::from_utf8_lossy(&out.stdout)))
}

/// The test executables in cargo's JSON messages, with their package folders.
fn binaries(messages: &str) -> Vec<Binary> {
    messages
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["profile"]["test"] == true)
        .filter_map(|m| {
            let exe = PathBuf::from(m["executable"].as_str()?);
            let dir = PathBuf::from(m["manifest_path"].as_str()?).parent()?.to_path_buf();
            Some(Binary { exe, dir })
        })
        .collect()
}

/// A test binary's name without cargo's hash: `lightcraft_raw-1a2b3c` → `lightcraft_raw`.
fn label(stem: &str) -> String {
    match stem.rsplit_once('-') {
        Some((name, hash)) if !hash.is_empty() && hash.bytes().all(|b| b.is_ascii_hexdigit()) => name.to_string(),
        _ => stem.to_string(),
    }
}

/// `test name ... ok <1.234s>` lines of libtest's `--report-time` output.
fn parse(stdout: &str) -> Vec<(String, f64)> {
    stdout
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("test ")?;
            let (name, result) = rest.split_once(" ... ")?;
            let secs = result.rsplit_once('<')?.1.strip_suffix("s>")?.parse::<f64>().ok()?;
            Some((name.to_string(), secs))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_report_time_lines() {
        let out = "running 3 tests\ntest a::b ... ok <0.012s>\ntest c ... FAILED <3.500s>\ntest d ... ignored\ntest result: ok.";
        assert_eq!(parse(out), [("a::b".to_string(), 0.012), ("c".to_string(), 3.5)]);
    }

    #[test]
    fn labels_drop_cargos_hash() {
        assert_eq!(label("lightcraft_raw-0f1e2d3c4b5a6978"), "lightcraft_raw");
        assert_eq!(label("robust"), "robust");
        assert_eq!(label("param_callers-x"), "param_callers-x");
    }

    #[test]
    fn lists_test_binaries_with_their_package_folders() {
        let msgs = concat!(
            r#"{"reason":"compiler-artifact","profile":{"test":true},"executable":"/t/debug/deps/raw-1a2b","manifest_path":"/w/crates/raw/Cargo.toml"}"#,
            "\n",
            r#"{"reason":"compiler-artifact","profile":{"test":false},"executable":null,"manifest_path":"/w/crates/raw/Cargo.toml"}"#,
            "\n",
            r#"{"reason":"build-finished","success":true}"#
        );
        let b = binaries(msgs);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].exe, PathBuf::from("/t/debug/deps/raw-1a2b"));
        assert_eq!(b[0].dir, PathBuf::from("/w/crates/raw"));
    }
}
