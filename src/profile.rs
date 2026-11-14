use crate::config;
use crate::exec;
use crate::parser;
use crate::runtime::{Profile, Runtime};

/// Load the profile named by `--profile`: None = read-only mode (empty
/// allow-lists, no file, a virtual load so the `profile` builtin works
/// during it and errors after); Some(name) = source
/// `<config_dir>/profiles/NAME.fush` with `profile_loading` set. A missing
/// profile file is a hard error (fail closed).
pub fn load_profile(rt: &mut Runtime, name: Option<&str>) -> Result<(), String> {
    rt.profile = Some(Profile::default());
    rt.profile_loading = true;
    let result = match name {
        None => Ok(()),
        Some(name) => load_profile_file(rt, name),
    };
    rt.profile_loading = false;
    result
}

fn load_profile_file(rt: &mut Runtime, name: &str) -> Result<(), String> {
    let Some(dir) = config::config_dir() else {
        return Err("no config directory; cannot load profile".to_string());
    };
    let path = dir.join("profiles").join(format!("{name}.fush"));
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
    let program = parser::parse(&text).map_err(|e| e.to_string())?;
    if let Err(e) = exec::run_stmts(rt, &program.stmts) {
        return Err(e.to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_profile_starts_empty_and_loading_ends() {
        let mut rt = Runtime::new();
        load_profile(&mut rt, None).unwrap();
        assert!(rt.profile.is_some());
        assert!(rt.profile.as_ref().unwrap().allow.is_empty());
        assert!(rt.profile.as_ref().unwrap().allow_write.is_empty());
        assert!(!rt.profile_loading);
    }

    #[test]
    fn named_profile_missing_file_is_error() {
        let mut rt = Runtime::new();
        let err = load_profile(&mut rt, Some("definitely-missing-profile")).unwrap_err();
        assert!(err.contains("cannot read"), "got: {err}");
        assert!(!rt.profile_loading);
    }
}
