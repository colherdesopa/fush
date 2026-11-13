use crate::exec;
use crate::parser;
use crate::runtime::Runtime;
use std::path::PathBuf;

#[allow(dead_code)]
pub fn config_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let userprofile = std::env::var_os("USERPROFILE").map(PathBuf::from);
    config_dir_for(
        home.as_deref().and_then(|p| p.to_str()),
        userprofile.as_deref().and_then(|p| p.to_str()),
        cfg!(windows),
    )
}

#[allow(dead_code)]
pub fn config_dir_for(
    home: Option<&str>,
    userprofile: Option<&str>,
    windows: bool,
) -> Option<PathBuf> {
    let base = if windows {
        userprofile.or(home)?
    } else {
        home?
    };
    let mut dir = PathBuf::from(base);
    dir.push(".config");
    dir.push("fush");
    Some(dir)
}

#[allow(dead_code)]
pub fn config_path() -> Option<PathBuf> {
    let dir = config_dir()?;
    Some(dir.join("config.fish"))
}

/// Config path with the FUSH_CONFIG override; defaults to config_path().
#[allow(dead_code)]
pub fn config_file() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FUSH_CONFIG") {
        return Some(PathBuf::from(p));
    }
    config_path()
}

/// Source the startup config into `rt` once, before the first prompt or
/// before -c/script execution. Missing file is silent; parse or runtime
/// errors print `fush: config: <msg>` and the shell continues. Runs with
/// the cwd unchanged; `set -l` at config top level behaves like `set -l`
/// at top level anywhere else (globals, since there is no enclosing
/// function scope).
#[allow(dead_code)]
pub fn source_config(rt: &mut Runtime) {
    if let Some(path) = config_file() {
        source_config_path(rt, &path);
    }
}

/// source_config with an injected path, for tests.
#[allow(dead_code)]
pub fn source_config_path(rt: &mut Runtime, path: &std::path::Path) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return;
    };
    let program = match parser::parse(&text) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("fush: config: {e}");
            return;
        }
    };
    if let Err(e) = exec::run_stmts(rt, &program.stmts) {
        eprintln!("fush: config: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_dir_uses_home_on_unix() {
        let home = "/tmp/fush-config-home";
        let path = PathBuf::from(home).join(".config").join("fush");
        assert_eq!(path, config_dir_for(Some(home), None, false).unwrap());
    }

    #[test]
    fn config_dir_uses_userprofile_on_windows() {
        let home = r"C:\Users\alice";
        let path = PathBuf::from(home).join(".config").join("fush");
        assert_eq!(path, config_dir_for(None, Some(home), true).unwrap());
    }

    #[test]
    fn config_dir_prefers_userprofile_over_home_on_windows() {
        let home = r"C:\Users\fallback";
        let userprofile = r"C:\Users\alice";
        let path = PathBuf::from(userprofile).join(".config").join("fush");
        assert_eq!(
            path,
            config_dir_for(Some(home), Some(userprofile), true).unwrap()
        );
    }

    #[test]
    fn source_config_populates_abbrs_and_vars() {
        let dir = std::env::temp_dir().join(format!(
            "fush-unit-config-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = dir.join("config.fish");
        std::fs::write(&cfg, "set greeting hello\nabbr gs git status").unwrap();
        let mut rt = Runtime::new();
        source_config_path(&mut rt, &cfg);
        let got_env = rt.env.get("greeting");
        let got_abbr = rt.abbrs.get("gs").cloned();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(got_env, Some(vec!["hello".to_string()]));
        assert_eq!(got_abbr, Some("git status".to_string()));
    }

    #[test]
    fn config_path_joins_config_fish() {
        let home = "/tmp/fush-config-home";
        let path = PathBuf::from(home)
            .join(".config")
            .join("fush")
            .join("config.fish");
        assert_eq!(
            path,
            config_dir_for(Some(home), None, false)
                .unwrap()
                .join("config.fish")
        );
    }
}
