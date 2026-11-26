//! yusAgent (YA-11, ticket 41, PR A, answer 17): every process the worker, the supervisor, the
//! bridge or the TUI launches goes through the guard (`ya_secret_guard::command`), so no process
//! launched by yusAgent can read the credential files.
//!
//! - `every_process_launch_goes_through_the_guard`: the sources of the crates linked into the
//!   `yusagent` binary hold no process-launch API outside the guard module, except the commented
//!   list `ya_secret_guard/launch_exceptions.txt` (one category only: the relaunch of the
//!   yusAgent binary itself, from `current_exe()`).
//! - `every_crate_of_the_lockfile_was_reviewed`: a crate new to `Cargo.lock` may launch
//!   processes; it fails here until it is reviewed and added to
//!   `ya_secret_guard/reviewed_crates.txt`.
//!
//! Test code (`#[cfg(test)]` modules, `tests.rs`, `*_tests.rs`, `tests/` directories) is not
//! scanned: it never runs in the product.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// The crates linked into the `yusagent` binary (`crates/pa-cli/Cargo.toml` and its workspace
/// dependencies); `pa-perf` is a benchmark tool, not shipped.
const SCANNED_CRATES: &[&str] = &[
    "pa-agent",
    "pa-ai",
    "pa-cli",
    "pa-core",
    "pa-daemon",
    "pa-models",
    "pa-telemetry",
    "pa-tui",
    "pa-types",
    "ya-apply-patch",
];

/// Source patterns that launch a process (aliases such as `StdCommand::new` contain the first).
const LAUNCH_PATTERNS: &[&str] = &[
    "Command::new(",
    "libc::fork",
    "libc::vfork",
    "libc::execv",
    "libc::execve",
    "libc::execvp",
    "libc::posix_spawn",
    "nix::unistd::fork",
    "nix::unistd::exec",
];

/// The guard module itself builds the wrapped commands.
const GUARD_FILE: &str = "ya_secret_guard.rs";

const RELAUNCH: &str = "relance du binaire yusAgent";
const WINDOWS: &str = "propre à Windows";
const LINUX: &str = "propre à Linux, jusqu'à la PR C";
const UNREACHABLE: &str = "inatteignable";

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn rust_files(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if path.is_dir() {
            if name != "tests" {
                rust_files(&path, found);
            }
        } else if name.ends_with(".rs") && name != "tests.rs" && !name.ends_with("_tests.rs") {
            found.push(path);
        }
    }
}

/// The source with every `#[cfg(test)]`/`#[cfg(all(test, …))]` item blanked (brace matching).
fn without_test_items(source: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = source.lines().collect();
    let mut kept = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let trimmed = lines[index].trim_start();
        if trimmed.starts_with("#[cfg(test)]") || trimmed.starts_with("#[cfg(all(test") {
            // Skip the attributed item: up to its closing brace, or its `;`.
            let mut depth = 0_i64;
            let mut opened = false;
            index += 1;
            while index < lines.len() {
                for ch in lines[index].chars() {
                    match ch {
                        '{' => {
                            depth += 1;
                            opened = true;
                        }
                        '}' => depth -= 1,
                        _ => {}
                    }
                }
                let ends_without_body = !opened && lines[index].trim_end().ends_with(';');
                index += 1;
                if (opened && depth <= 0) || ends_without_body {
                    break;
                }
            }
            continue;
        }
        kept.push((index + 1, lines[index].to_string()));
        index += 1;
    }
    kept
}

struct Exception {
    file: String,
    line: String,
    category: String,
    reason: String,
}

fn exceptions(root: &Path) -> Vec<Exception> {
    let text = std::fs::read_to_string(
        root.join("crates/pa-core/tests/ya_secret_guard/launch_exceptions.txt"),
    )
    .unwrap();
    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .map(|line| {
            let fields: Vec<&str> = line.split(" | ").collect();
            assert_eq!(fields.len(), 4, "malformed exception: {line}");
            assert!(
                [RELAUNCH, WINDOWS, LINUX, UNREACHABLE].contains(&fields[2]),
                "unknown exception category: {line}"
            );
            Exception {
                file: fields[0].to_string(),
                line: fields[1].to_string(),
                category: fields[2].to_string(),
                reason: fields[3].to_string(),
            }
        })
        .collect()
}

/// Each category's proof: `current_exe()` in the file, the platform `cfg` in the 40 lines
/// before the launch, or the cited blocking `file:line`.
fn check_exception(root: &Path, exception: &Exception, source: &str, number: usize) {
    let lines: Vec<&str> = source.lines().collect();
    let before = lines[number.saturating_sub(40)..number].join("\n");
    match exception.category.as_str() {
        RELAUNCH => assert!(
            source.contains("current_exe()"),
            "{}: a relaunch exception must launch `current_exe()`",
            exception.file
        ),
        WINDOWS => assert!(
            before.contains("cfg(windows)"),
            "{}:{number}: no `cfg(windows)` before a Windows exception",
            exception.file
        ),
        LINUX => assert!(
            before.contains("not(target_vendor = \"apple\")") || before.contains("linux"),
            "{}:{number}: no Linux-only marker before a Linux exception",
            exception.file
        ),
        _ => {
            let cited = exception
                .reason
                .split('`')
                .find(|part| part.starts_with("crates/") && part.contains(".rs:"))
                .unwrap_or_else(|| {
                    panic!(
                        "{}: an unreachable launch cites no blocking file:line",
                        exception.file
                    )
                });
            let (file, line) = cited.rsplit_once(':').unwrap();
            let line: usize = line.parse().unwrap();
            let blocking = std::fs::read_to_string(root.join(file)).unwrap();
            assert!(
                blocking.lines().count() >= line,
                "{}: the cited blocking line {cited} does not exist",
                exception.file
            );
        }
    }
}

#[test]
fn every_process_launch_goes_through_the_guard() {
    let root = repo_root();
    let allowed = exceptions(&root);
    let mut used = BTreeSet::new();
    let mut violations = Vec::new();
    for krate in SCANNED_CRATES {
        let mut files = Vec::new();
        rust_files(&root.join("crates").join(krate).join("src"), &mut files);
        files.sort();
        for file in files {
            if file.file_name().and_then(|name| name.to_str()) == Some(GUARD_FILE) {
                continue;
            }
            let source = std::fs::read_to_string(&file).unwrap();
            let relative = file
                .strip_prefix(&root)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            for (number, line) in without_test_items(&source) {
                let code = line.split("//").next().unwrap_or_default();
                if !LAUNCH_PATTERNS.iter().any(|pattern| code.contains(pattern)) {
                    continue;
                }
                let trimmed = line.trim();
                let excepted = allowed
                    .iter()
                    .position(|exception| exception.file == relative && exception.line == trimmed);
                match excepted {
                    Some(position) => {
                        check_exception(&root, &allowed[position], &source, number);
                        used.insert(position);
                    }
                    None => violations.push(format!("{relative}:{number}: {trimmed}")),
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "{} process launch(es) outside the guard (ticket 41, X1):\n{}",
        violations.len(),
        violations.join("\n")
    );
    let stale: Vec<&str> = allowed
        .iter()
        .enumerate()
        .filter(|(position, _)| !used.contains(position))
        .map(|(_, exception)| exception.file.as_str())
        .collect();
    assert!(stale.is_empty(), "stale launch exceptions: {stale:?}");
}

#[test]
fn every_crate_of_the_lockfile_was_reviewed() {
    let root = repo_root();
    let reviewed: BTreeSet<String> = std::fs::read_to_string(
        root.join("crates/pa-core/tests/ya_secret_guard/reviewed_crates.txt"),
    )
    .unwrap()
    .lines()
    .map(str::to_string)
    .collect();
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap();
    let unreviewed: Vec<&str> = lock
        .lines()
        .filter_map(|line| line.strip_prefix("name = \""))
        .filter_map(|rest| rest.strip_suffix('"'))
        .filter(|name| !reviewed.contains(*name))
        .collect();
    assert!(
        unreviewed.is_empty(),
        "crates new to Cargo.lock, not reviewed for process launches (ticket 41): {unreviewed:?}"
    );
}
