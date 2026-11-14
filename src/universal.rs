//! Universal variables: vars persisted across fush sessions.
//!
//! On-disk format (`universals` file, one line per variable):
//! `name<TAB>value1<TAB>value2...` — a var with no values is stored as
//! `name` alone (one field). Values may contain spaces (each tab-separated
//! field is one list element); values containing tabs or newlines cannot
//! be represented and are rejected by `set -U`.

use crate::runtime::Runtime;
use std::path::{Path, PathBuf};

/// Universals file path with the FUSH_UNIVERSALS override; defaults to
/// config_dir()/universals.
#[allow(dead_code)]
pub fn universals_file() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FUSH_UNIVERSALS") {
        return Some(PathBuf::from(p));
    }
    crate::config::config_dir().map(|dir| dir.join("universals"))
}

/// Persist `name values...` to the universals file (env must already be
/// updated by the caller). Rejects values containing tabs or newlines.
#[allow(dead_code)]
pub fn save(rt: &Runtime, path: &Path, name: &str, values: &[String]) -> Result<(), String> {
    for v in values {
        if v.contains('\t') || v.contains('\n') {
            return Err(format!(
                "fush: set: universal values cannot contain tabs or newlines (variable '{name}')"
            ));
        }
    }
    let mut vars = read_lines(path);
    match vars.iter().position(|(n, _)| n == name) {
        Some(i) => vars[i].1 = values.to_vec(),
        None => vars.push((name.to_string(), values.to_vec())),
    }
    let text: String = vars
        .into_iter()
        .map(|(n, vals)| {
            if vals.is_empty() {
                n
            } else {
                format!("{n}\t{}", vals.join("\t"))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        let _ = rt;
        return Err(format!("fush: set: cannot write universals file: {e}"));
    }
    let tmp = path.with_extension("tmp");
    if let Err(e) = std::fs::write(&tmp, &text) {
        let _ = rt;
        return Err(format!("fush: set: cannot write universals file: {e}"));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("fush: set: cannot write universals file: {e}"));
    }
    Ok(())
}

/// Remove `name` from the universals file if present. Missing file or
/// missing var is not an error.
#[allow(dead_code)]
pub fn erase(path: &Path, name: &str) {
    let mut vars = read_lines(path);
    let before = vars.len();
    vars.retain(|(n, _)| n != name);
    if vars.len() == before {
        return;
    }
    let text: String = vars
        .into_iter()
        .map(|(n, vals)| {
            if vals.is_empty() {
                n
            } else {
                format!("{n}\t{}", vals.join("\t"))
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let text = if text.is_empty() {
        String::new()
    } else {
        text + "\n"
    };
    let tmp = path.with_extension("tmp");
    if std::fs::write(&tmp, &text).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

/// Load universals from `path` into the global env. A line that is not
/// valid UTF-8 or has an empty name is skipped with a single warning to
/// stderr; the rest of the file still loads.
#[allow(dead_code)]
pub fn load(rt: &mut Runtime, path: &Path) {
    let (vars, bad) = read_lines_with_warnings(path, path);
    if bad {
        eprintln!(
            "fush: warning: skipped malformed line(s) in {}",
            path.display()
        );
    }
    for (name, values) in vars {
        rt.env.set_global(&name, values);
    }
}

/// Parse the file into (name, values) pairs, ignoring malformed lines.
fn read_lines(path: &Path) -> Vec<(String, Vec<String>)> {
    read_lines_with_warnings(path, path).0
}

fn read_lines_with_warnings(path: &Path, display: &Path) -> (Vec<(String, Vec<String>)>, bool) {
    let Ok(bytes) = std::fs::read(path) else {
        return (Vec::new(), false);
    };
    let Ok(text) = String::from_utf8(bytes) else {
        eprintln!(
            "fush: warning: {} is not valid UTF-8; skipping malformed line(s)",
            display.display()
        );
        return (Vec::new(), true);
    };
    let mut vars = Vec::new();
    let mut bad = false;
    for line in text.lines() {
        if line.is_empty() {
            continue;
        }
        let mut fields = line.split('\t');
        let name = fields.next().unwrap_or("");
        if name.is_empty() {
            bad = true;
            continue;
        }
        vars.push((name.to_string(), fields.map(|s| s.to_string()).collect()));
    }
    (vars, bad)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_creates_missing_parent_directory() {
        let base = std::env::temp_dir().join(format!(
            "fush-universal-mkdir-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = base.join("deeply").join("nested").join("universals");
        let rt = Runtime::new();
        save(&rt, &path, "made", &["yes".to_string()]).expect("save");
        let vars = read_lines(&path);
        assert_eq!(vars, vec![("made".to_string(), vec!["yes".to_string()])]);
        let _ = std::fs::remove_dir_all(&base);
    }

    fn temp_dir(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "fush-universal-{}-{:?}-{tag}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    #[test]
    #[allow(unused_mut)]
    fn save_then_load_round_trip() {
        let dir = temp_dir("rt");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("universals");
        let mut rt = Runtime::new();
        rt.env.set_global("greeting", vec!["hello".into()]);
        save(&rt, &path, "greeting", &["hello".to_string()]).unwrap();
        save(&rt, &path, "list", &["a".into(), "b c".into(), "d".into()]).unwrap();

        let mut rt2 = Runtime::new();
        load(&mut rt2, &path);
        assert_eq!(rt2.env.get("greeting"), Some(vec!["hello".to_string()]));
        assert_eq!(
            rt2.env.get("list"),
            Some(vec!["a".to_string(), "b c".to_string(), "d".to_string()])
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[allow(unused_mut)]
    fn save_updates_existing_variable() {
        let dir = temp_dir("upd");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("universals");
        let rt = Runtime::new();
        save(&rt, &path, "x", &["1".to_string()]).unwrap();
        save(&rt, &path, "x", &["2".to_string(), "3".to_string()]).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "x\t2\t3\n");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_rejects_tab_and_newline_values() {
        let dir = temp_dir("rej");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("universals");
        let rt = Runtime::new();
        assert!(save(&rt, &path, "x", &["a\tb".to_string()]).is_err());
        assert!(save(&rt, &path, "x", &["a\nb".to_string()]).is_err());
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn erase_removes_line_and_keeps_others() {
        let dir = temp_dir("erase");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("universals");
        let rt = Runtime::new();
        save(&rt, &path, "keep", &["1".to_string()]).unwrap();
        save(&rt, &path, "drop", &["2".to_string()]).unwrap();
        erase(&path, "drop");
        let text = std::fs::read_to_string(&path).unwrap();
        assert_eq!(text, "keep\t1\n");
        erase(&path, "missing");
        erase(&dir.join("nonexistent"), "keep");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[allow(unused_mut)]
    fn load_skips_bad_lines_with_warning() {
        let dir = temp_dir("bad");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("universals");
        std::fs::write(&path, "good\t1\n\n\tbad\nalso-good\t2\t3\n").unwrap();
        let mut rt = Runtime::new();
        load(&mut rt, &path);
        assert_eq!(rt.env.get("good"), Some(vec!["1".to_string()]));
        assert_eq!(
            rt.env.get("also-good"),
            Some(vec!["2".to_string(), "3".to_string()])
        );
        assert_eq!(rt.env.get("\tbad"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[allow(unused_mut)]
    fn load_missing_file_is_silent() {
        let mut rt = Runtime::new();
        load(&mut rt, Path::new("/nonexistent/fush/universals"));
        assert_eq!(rt.env.get("x"), None);
    }
}
