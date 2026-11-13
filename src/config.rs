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

/// Primary config path for an injected environment, mirroring
/// `config_dir_for`: `<base>/.config/fush/config.fush`.
#[allow(dead_code)]
pub fn primary_config_path_for(
    home: Option<&str>,
    userprofile: Option<&str>,
    windows: bool,
) -> Option<PathBuf> {
    Some(config_dir_for(home, userprofile, windows)?.join("config.fush"))
}

/// Primary config path, `<config_dir>/config.fush`. Unlike `config_file`,
/// no environment override applies here.
#[allow(dead_code)]
pub fn primary_config_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let userprofile = std::env::var_os("USERPROFILE").map(PathBuf::from);
    primary_config_path_for(
        home.as_deref().and_then(|p| p.to_str()),
        userprofile.as_deref().and_then(|p| p.to_str()),
        cfg!(windows),
    )
}

/// Primary config path with the FUSH_CONFIG override; defaults to
/// `primary_config_path()`.
#[allow(dead_code)]
pub fn primary_config_file() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FUSH_CONFIG") {
        return Some(PathBuf::from(p));
    }
    primary_config_path()
}

/// Source the primary config (`config.fush`) into `rt`. Missing file is
/// silent; parse or runtime errors print `fush: config: <msg>` and the
/// shell continues, matching `source_config`.
#[allow(dead_code)]
pub fn source_primary_config(rt: &mut Runtime) {
    if let Some(path) = primary_config_file() {
        source_config_path(rt, &path);
    }
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

/// Outcome of first-run detection. `Import` and `Decline` are the two
/// recorded results of prompting the user; `Nothing` means no prompt is
/// needed (primary config exists, or there is no fish config to offer).
#[allow(dead_code)]
pub enum FirstRun {
    Import,
    Decline,
    Nothing,
}

/// Whether first-run import is pending: only when the primary config is
/// absent and a fish config exists. Every other combination is `Nothing`;
/// `Import`/`Decline` are set by the caller after prompting.
#[allow(dead_code)]
pub fn first_run_state(fush: Option<&std::path::Path>, fish: Option<&std::path::Path>) -> FirstRun {
    match (fush, fish) {
        (Some(fush), Some(fish)) if !fush.exists() && fish.exists() => FirstRun::Import,
        _ => FirstRun::Nothing,
    }
}

/// Copy the fish config to the primary config path atomically: read the
/// source, create missing parent dirs, write a tmp file, then rename over
/// the target. Overwrites an existing target without corrupting it.
/// Errors start `fush: config:`.
#[allow(dead_code)]
pub fn import_fish_config(fish: &std::path::Path, fush: &std::path::Path) -> Result<(), String> {
    let bytes = std::fs::read(fish).map_err(|e| format!("fush: config: cannot read fish config: {e}"))?;
    write_atomic(fush, &bytes)
}

/// Create an empty primary config file (and any missing parent dirs).
/// Errors start `fush: config:`.
#[allow(dead_code)]
pub fn create_empty_config(fush: &std::path::Path) -> Result<(), String> {
    write_atomic(fush, b"")
}

fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return Err(format!("fush: config: cannot create config dir: {e}"));
    }
    let tmp = path.with_extension("tmp");
    if let Err(e) = std::fs::write(&tmp, bytes) {
        return Err(format!("fush: config: cannot write config: {e}"));
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(format!("fush: config: cannot write config: {e}"));
    }
    Ok(())
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
    fn primary_config_path_is_config_fush() {
        let dir = config_dir_for(Some("/h"), None, false).unwrap();
        let path = primary_config_path_for(Some("/h"), None, false).unwrap();
        assert_eq!(path, dir.join("config.fush"));
    }

    #[test]
    fn primary_config_path_uses_userprofile_on_windows() {
        let userprofile = r"C:\Users\alice";
        let path = PathBuf::from(userprofile)
            .join(".config")
            .join("fush")
            .join("config.fush");
        assert_eq!(
            path,
            primary_config_path_for(None, Some(userprofile), true).unwrap()
        );
    }

    #[test]
    fn primary_config_file_prefers_fush_config() {
        let prev = std::env::var_os("FUSH_CONFIG");
        // SAFETY: tests in this module run serially; no other thread reads env.
        unsafe {
            std::env::set_var("FUSH_CONFIG", "/tmp/fush-custom.fush");
        }
        let got = primary_config_file();
        // SAFETY: restoring the prior value in the same test.
        unsafe {
            match prev {
                Some(v) => std::env::set_var("FUSH_CONFIG", v),
                None => std::env::remove_var("FUSH_CONFIG"),
            }
        }
        assert_eq!(got, Some(PathBuf::from("/tmp/fush-custom.fush")));
    }

    #[test]
    fn source_primary_config_missing_file_is_silent() {
        let prev = std::env::var_os("FUSH_CONFIG");
        // SAFETY: tests in this module run serially; no other thread reads env.
        unsafe {
            std::env::set_var("FUSH_CONFIG", "/tmp/fush-no-such-config-file");
        }
        let mut rt = Runtime::new();
        source_primary_config(&mut rt);
        // SAFETY: restoring the prior value in the same test.
        unsafe {
            match prev {
                Some(v) => std::env::set_var("FUSH_CONFIG", v),
                None => std::env::remove_var("FUSH_CONFIG"),
            }
        }
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

#[cfg(test)]
mod first_run_tests {
    use super::*;

    fn temp_dir(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "fush-config-{}-{:?}-{tag}",
            std::process::id(),
            std::thread::current().id()
        ))
    }

    fn write(path: &std::path::Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn first_run_state_matrix() {
        let base = temp_dir("matrix");
        let fush = base.join("cfg").join("config.fush");
        let fish = base.join("fish").join("config.fish");
        let _ = std::fs::remove_dir_all(&base);

        write(&fish, b"set -x EDITOR vim\n");

        // fush missing + fish exists -> pending import.
        assert!(matches!(
            first_run_state(Some(&fush), Some(&fish)),
            FirstRun::Import
        ));
        // fush missing + fish missing -> nothing.
        assert!(matches!(
            first_run_state(Some(&fush), None),
            FirstRun::Nothing
        ));
        assert!(matches!(
            first_run_state(Some(&fush), Some(&base.join("nope"))),
            FirstRun::Nothing
        ));

        write(&fush, b"");
        // fush exists -> nothing, fish present or not.
        assert!(matches!(
            first_run_state(Some(&fush), Some

(&fish)),
            FirstRun::Nothing
        ));
        assert!(matches!(first_run_state(Some(&fush), None), FirstRun::Nothing));

        // fush missing (None) -> nothing even with fish present.
        assert!(matches!(
            first_run_state(None, Some(&fish)),
            FirstRun::Nothing
        ));
        assert!(matches!(first_run_state(None, None), FirstRun::Nothing));

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn import_copies_bytes_exactly() {
        let

base = temp_dir("import");
        let fish = base.join("fish").join("config.fish");
        let fush = base.join("cfg").join("config.fush");
        let _ = std::fs::remove_dir_all(&base);
        let bytes = b"set -g x 1\nalias ll 'ls -l'\n";
        write(&fish, bytes);

        import_fish_config(&fish, &fush).expect("import");
        assert_eq!(std::fs::read(&fush).unwrap(), bytes);

        let _

 = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn import_overwrites_existing_target() {
        let base = temp_dir("overwrite");
        let fish = base.join("fish").join("config.fish");
        let fush = base.join("cfg").


join("config.fush");
        let _ = std::fs::remove_dir_all(&base);
        write(&fush, b"old-content-that-should-go\n");
        let bytes = b"new\n";
        write(&fish, bytes);

        import_fish_config(&fish, &fush).expect("import");
        assert_eq!(std::fs::read(&fush).unwrap(), bytes);
        assert!(!fush
            .with_extension("tmp")
            .exists());

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn create_empty_config_zero_bytes_and_parents() {
        let base = temp_dir("empty");
        let fush = base.join("a").join("b").join("config.fush");
        let _ = std::fs::remove_dir_all(&base);

        create_empty_config(&fush).expect("create");
        assert!(fush.is_file());
        assert_eq!(std::fs::read(&fush).unwrap(), Vec::<u8>::new());

        let _ = std::fs::remove_dir_all(&base);
    }
}
