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
