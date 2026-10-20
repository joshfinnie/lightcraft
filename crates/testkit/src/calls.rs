//! Written-out command calls in the repository's sources (Rust `"photo.rate", json!({…})`, control
//! scripts and docs `"command": "photo.rate", "params": {…}`), for tests that check each passes
//! only keys its command takes. A test that sends an unknown key on purpose says so on that line:
//! `// unknown on purpose`.

use std::path::{Path, PathBuf};

/// The source files (`.rs`, `.jsonl`, `.md`, `.py`) under the repository's `apps`, `crates`, `docs`
/// and `xtask`, relative paths and texts.
pub fn repo_sources() -> Vec<(String, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    for dir in ["apps", "crates", "docs", "xtask"] {
        sources(&root.join(dir), &mut files);
    }
    files
        .into_iter()
        .filter_map(|f| {
            let text = std::fs::read_to_string(&f).ok()?;
            Some((f.strip_prefix(&root).unwrap_or(&f).display().to_string(), text))
        })
        .collect()
}

/// Every written-out call in the repository: `(file, line, command, keys)`.
pub fn repo_calls() -> Vec<(String, usize, String, Vec<String>)> {
    let mut out = Vec::new();
    for (file, text) in repo_sources() {
        for (id, keys, line) in calls(&text) {
            out.push((file.clone(), line, id, keys));
        }
    }
    out
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" || name == "node_modules" {
            continue;
        }
        if p.is_dir() {
            sources(&p, out);
        } else if ["rs", "jsonl", "md", "py"].iter().any(|x| p.extension().is_some_and(|e| e == *x)) {
            out.push(p);
        }
    }
}

/// The string literal starting at `b[i]` (a `"`): its text and the index after it.
fn literal(b: &[u8], i: usize) -> Option<(String, usize)> {
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => return Some((String::from_utf8_lossy(&b[i + 1..j]).to_string(), j + 1)),
            b'\n' => return None,
            _ => j += 1,
        }
    }
    None
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while i < b.len() && b[i].is_ascii_whitespace() {
        i += 1;
    }
    i
}

/// The top-level keys of the object starting at `b[i]` (a `{`).
fn object_keys(b: &[u8], i: usize) -> Vec<String> {
    let (mut depth, mut j, mut keys) = (0usize, i, Vec::new());
    while j < b.len() {
        match b[j] {
            b'"' => {
                let Some((text, end)) = literal(b, j) else { return keys };
                if depth == 1 && b.get(skip_ws(b, end)) == Some(&b':') {
                    keys.push(text);
                }
                j = end;
                continue;
            }
            b'{' | b'[' | b'(' => depth += 1,
            b'}' | b']' | b')' => {
                depth -= 1;
                if depth == 0 {
                    return keys;
                }
            }
            _ => {}
        }
        j += 1;
    }
    keys
}

fn is_command_id(t: &str) -> bool {
    let mut parts = t.split('.');
    parts.next().is_some_and(|p| p.starts_with(|c: char| c.is_ascii_lowercase()))
        && t.contains('.')
        && t.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric()))
}

/// `(command, keys, line)` of each written-out call in `text` (a line saying `// unknown on purpose` is left out).
pub fn calls(text: &str) -> Vec<(String, Vec<String>, usize)> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'"' {
            i += 1;
            continue;
        }
        let Some((id, end)) = literal(b, i) else {
            i += 1;
            continue;
        };
        i = end;
        if !is_command_id(&id) {
            continue;
        }
        let mut j = skip_ws(b, end);
        if b.get(j) != Some(&b',') {
            continue;
        }
        j = skip_ws(b, j + 1);
        if b.get(j) == Some(&b'&') {
            j = skip_ws(b, j + 1);
        }
        if b[j..].starts_with(b"json!(") {
            j = skip_ws(b, j + 6);
        } else if b[j..].starts_with(b"\"params\"") {
            j = skip_ws(b, j + 8);
            if b.get(j) != Some(&b':') {
                continue;
            }
            j = skip_ws(b, j + 1);
        } else {
            continue;
        }
        if b.get(j) == Some(&b'{') {
            let line = text[..j].matches('\n').count() + 1;
            if text.lines().nth(line - 1).is_some_and(|l| l.contains("// unknown on purpose")) {
                continue;
            }
            out.push((id, object_keys(b, j), line));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::calls;

    #[test]
    fn the_scanner_finds_rust_and_json_calls() {
        let text = "s.execute(\"photo.rate\", &json!({\"rating\": 2, \"ids\": [{\"x\": 1}]}));\n{\"method\":\"engine.execute\",\"params\":{\"command\":\"photo.flag\",\"params\":{\"flag\":\"pick\"}}}";
        let c = calls(text);
        assert_eq!(c[0], ("photo.rate".to_string(), vec!["rating".to_string(), "ids".to_string()], 1));
        assert_eq!(c[1].0, "engine.execute");
        assert_eq!(c[2], ("photo.flag".to_string(), vec!["flag".to_string()], 2));
        assert!(calls("x(\"photo.rate\", json!({\"idz\": 1})); // unknown on purpose").is_empty());
    }
}
