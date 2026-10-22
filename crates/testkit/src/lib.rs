//! Test tooling shared by the workspace's tests. A dev-dependency only: `cargo xtask layers`
//! refuses it as a normal dependency. Std only, so every crate can use it at no build cost.
//!
//! **Skips.** libtest has no "skipped" status and hides the output of passing tests, so a test that
//! needs a GPU, a corpus file or a model, and returns early without them, passes silently: "all
//! steps passed" could mean the GPU tests never ran. Such a test calls [`skip!`] before it returns:
//!
//! ```no_run
//! # let gpu_available = false;
//! if !gpu_available {
//!     lightcraft_testkit::skip!(Gpu, "no GPU adapter");
//!     return;
//! }
//! ```
//!
//! It prints the note to stderr as before and, when the [`SKIP_LOG_ENV`] variable names a file
//! (`cargo xtask ci` sets it), appends one JSON line there:
//! `{"need":"gpu","reason":"no GPU adapter","test":"camera_tone","binary":"equivalence","location":"crates/gpu/tests/equivalence.rs:22"}`.
//! `cargo xtask ci` lists the skips by need and reason in its summary and in
//! `target/ci/report.json`. A test that runs part of its checks (e.g. only the CPU half) records
//! the part it could not run the same way.
//!
//! **Deadlines.** A test that waits for background work gives up after a deadline, so a hang fails
//! the run instead of stalling it. A coverage build runs several times slower, so a deadline sized
//! for a normal build fails there for no fault of the code. Such a test sizes its deadline with
//! [`scaled`]: unchanged normally, [`COVERAGE_TIME_SCALE`] times longer under coverage, and
//! [`TIME_SCALE_ENV`] stretches it on a slow machine. A test that checks how fast something is (a
//! speed budget) asks [`speed_budgets`] first, and records a [`Need::Speed`] skip when it can't.
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![allow(clippy::disallowed_methods)]

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::Path;

/// The environment variable naming the file skips are appended to (JSON lines).
pub const SKIP_LOG_ENV: &str = "LIGHTCRAFT_SKIP_LOG";

/// What a skipped test needed and didn't find.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Need {
    /// A usable GPU adapter (`lightcraft_gpu::available()`).
    Gpu,
    /// A file from the gitignored `corpus/` (`cargo xtask corpus --download`) or a private corpus.
    Corpus,
    /// A model file named by an environment variable (SAM 3, YuNet, …).
    Model,
    /// An input file named by an environment variable (opt-in tests).
    Input,
    /// Something the operating system provides (system ICC profiles, …).
    System,
    /// Support this build doesn't have yet (a decoder that refuses a file variant, an optional
    /// cargo feature that is off).
    Feature,
    /// A build that runs at full speed: a speed budget means nothing under coverage or a stretched
    /// [`time_scale`].
    Speed,
}

impl Need {
    /// The name in the skip log and the CI summary.
    pub fn as_str(self) -> &'static str {
        match self {
            Need::Gpu => "gpu",
            Need::Corpus => "corpus",
            Need::Model => "model",
            Need::Input => "input",
            Need::System => "system",
            Need::Feature => "feature",
            Need::Speed => "speed",
        }
    }
}

/// Record that this test (or part of it) can't run here: `skip!(Gpu, "no GPU adapter ({why})")`.
/// The first argument names a [`Need`] variant; the rest is a `format!` message. Doesn't return
/// from the test: follow it with `return` (or `continue`) as the test needs.
#[macro_export]
macro_rules! skip {
    ($need:ident, $($arg:tt)+) => {
        $crate::record($crate::Need::$need, &::std::format!($($arg)+), ::std::file!(), ::std::line!())
    };
}

/// What [`skip!`] calls: print the note and append it to the skip log, if one is set. Never fails
/// the test: a log that can't be written loses the entry, not the run.
pub fn record(need: Need, reason: &str, file: &str, line: u32) {
    eprintln!("skipped ({}): {reason}", need.as_str());
    let log = std::env::var_os(SKIP_LOG_ENV).filter(|p| !p.is_empty());
    if let Some(log) = log {
        let thread = std::thread::current();
        let binary = std::env::current_exe().ok().and_then(|p| p.file_stem().map(|s| binary_name(&s.to_string_lossy()).to_owned()));
        let entry = entry(need, reason, thread.name().unwrap_or(""), binary.as_deref().unwrap_or(""), file, line);
        append(Path::new(&log), &entry);
    }
}

/// Append one line in a single write, so lines from parallel tests don't interleave.
fn append(path: &Path, line: &str) {
    let file = std::fs::OpenOptions::new().create(true).append(true).open(path);
    if let Err(e) = file.and_then(|mut f| f.write_all(line.as_bytes())) {
        eprintln!("{SKIP_LOG_ENV}: can't append to {}: {e}", path.display());
    }
}

/// A test binary's name without the hash cargo appends (`equivalence-0123abcd` → `equivalence`).
fn binary_name(stem: &str) -> &str {
    match stem.rsplit_once('-') {
        Some((name, hash)) if !name.is_empty() && hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()) => name,
        _ => stem,
    }
}

/// One skip-log line (JSON, newline-terminated).
fn entry(need: Need, reason: &str, test: &str, binary: &str, file: &str, line: u32) -> String {
    let mut s = String::from("{");
    for (i, (key, value)) in [("need", need.as_str()), ("reason", reason), ("test", test), ("binary", binary)].into_iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        let _ = write!(s, "\"{key}\":\"{}\"", escape(value));
    }
    let _ = writeln!(s, ",\"location\":\"{}:{line}\"}}", escape(&file.replace('\\', "/")));
    s
}

/// A JSON string's contents.
fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out
}

/// The environment variable that stretches test deadlines on a slow machine: a number from 1 to
/// [`MAX_TIME_SCALE`] (`LIGHTCRAFT_TEST_TIME_SCALE=4`).
pub const TIME_SCALE_ENV: &str = "LIGHTCRAFT_TEST_TIME_SCALE";

/// How much longer deadlines are in a coverage build, unless [`TIME_SCALE_ENV`] says otherwise.
pub const COVERAGE_TIME_SCALE: f64 = 10.0;

/// The largest stretch [`TIME_SCALE_ENV`] can ask for.
pub const MAX_TIME_SCALE: f64 = 100.0;

/// Whether this test binary is a coverage build: `cargo llvm-cov` names the profile file the
/// instrumented binary writes in `LLVM_PROFILE_FILE`.
pub fn instrumented() -> bool {
    std::env::var_os("LLVM_PROFILE_FILE").is_some_and(|v| !v.is_empty())
}

/// How much longer than normal deadlines are here (1 in a normal run).
pub fn time_scale() -> f64 {
    scale_from(std::env::var(TIME_SCALE_ENV).ok().as_deref(), instrumented())
}

fn scale_from(env: Option<&str>, instrumented: bool) -> f64 {
    match env.and_then(|v| v.trim().parse::<f64>().ok()).filter(|v| v.is_finite()) {
        Some(v) => v.clamp(1.0, MAX_TIME_SCALE),
        None if instrumented => COVERAGE_TIME_SCALE,
        None => 1.0,
    }
}

/// A deadline of `d` in a normal run, stretched by [`time_scale`] where the build or the machine is
/// slower.
pub fn scaled(d: std::time::Duration) -> std::time::Duration {
    d.mul_f64(time_scale())
}

/// Whether a test's speed budget can be checked here: not in a coverage build, nor where deadlines
/// are stretched. A test that can't check one records `skip!(Speed, …)`.
pub fn speed_budgets() -> bool {
    !instrumented() && time_scale() <= 1.0
}

pub mod calls;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deadlines_stretch_under_coverage_and_on_request() {
        assert_eq!(scale_from(None, false), 1.0);
        assert_eq!(scale_from(None, true), COVERAGE_TIME_SCALE);
        assert_eq!(scale_from(Some("4"), false), 4.0);
        assert_eq!(scale_from(Some(" 2.5 "), true), 2.5, "the variable wins over the coverage default");
        assert_eq!(scale_from(Some("0.1"), false), 1.0, "never shorter than normal");
        assert_eq!(scale_from(Some("1e9"), false), MAX_TIME_SCALE);
        assert_eq!(scale_from(Some("NaN"), true), COVERAGE_TIME_SCALE);
        assert_eq!(scale_from(Some("fast"), false), 1.0);
    }

    #[test]
    fn an_entry_is_one_json_line_with_its_text_escaped() {
        let line = entry(Need::Corpus, "skip: C:\\corpus\\raw \"absent\"\n\u{1}", "corpus::decodes", "corpus", "crates\\raw\\tests\\corpus.rs", 28);
        assert!(line.ends_with('\n'));
        assert_eq!(line.matches('\n').count(), 1, "{line}");
        assert_eq!(
            line,
            "{\"need\":\"corpus\",\"reason\":\"skip: C:\\\\corpus\\\\raw \\\"absent\\\"\\n\\u0001\",\"test\":\"corpus::decodes\",\
             \"binary\":\"corpus\",\"location\":\"crates/raw/tests/corpus.rs:28\"}\n"
        );
    }

    #[test]
    fn binary_names_lose_cargos_hash_only() {
        assert_eq!(binary_name("equivalence-0f3a9c2b1d4e5f60"), "equivalence");
        assert_eq!(binary_name("lightcraft_engine-0f3a9c2b1d4e5f60"), "lightcraft_engine");
        assert_eq!(binary_name("sparse_corpus"), "sparse_corpus");
        assert_eq!(binary_name("cr3-corpus"), "cr3-corpus");
        assert_eq!(binary_name("-0f3a9c2b1d4e5f60"), "-0f3a9c2b1d4e5f60");
        assert_eq!(binary_name("tool-cafe"), "tool-cafe");
    }

    #[test]
    fn appended_lines_accumulate() {
        let dir = std::env::temp_dir().join(format!("lightcraft-testkit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("skips.jsonl");
        let _ = std::fs::remove_file(&log);
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let log = log.clone();
                std::thread::spawn(move || append(&log, &entry(Need::Gpu, &format!("no GPU {i}"), "t", "b", "f.rs", i)))
            })
            .collect();
        for t in threads {
            t.join().unwrap();
        }
        let text = std::fs::read_to_string(&log).unwrap();
        assert_eq!(text.lines().count(), 8);
        assert!(text.lines().all(|l| l.starts_with("{\"need\":\"gpu\"") && l.ends_with('}')));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_missing_log_directory_does_not_fail_the_test() {
        append(Path::new("/nonexistent-lightcraft-dir/skips.jsonl"), "{}\n");
    }
}
