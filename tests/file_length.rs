//! Project rule: no source file longer than 400 lines (tests included).
//! Split a module into a directory (`foo/mod.rs` + `foo/bar.rs`) instead.

use std::path::{Path, PathBuf};

const MAX_LINES: usize = 400;

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn source_files_stay_under_400_lines() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut files = Vec::new();
    for dir in ["src", "tests"] {
        rust_files(&root.join(dir), &mut files);
    }
    assert!(!files.is_empty());

    let too_long: Vec<String> = files
        .iter()
        .filter_map(|f| {
            let n = std::fs::read_to_string(f).unwrap().lines().count();
            (n > MAX_LINES).then(|| format!("{} has {n} lines", f.strip_prefix(root).unwrap().display()))
        })
        .collect();
    assert!(
        too_long.is_empty(),
        "files over the {MAX_LINES}-line limit:\n  {}",
        too_long.join("\n  ")
    );
}
