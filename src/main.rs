mod ast;
mod builtins;
mod completion;
mod config;
mod exec;
mod highlight;
mod lexer;
mod parser;
mod profile;
mod repl;
mod runtime;
mod suggest;
mod universal;

#[derive(Debug)]
struct Args {
    ast_mode: bool,
    interactive: bool,
    src: Option<String>,
    /// Outer Some = `--profile` given; inner = explicit profile name
    /// (None = read-only mode).
    profile: Option<Option<String>>,
}

fn parse_args(args: &[String]) -> Result<Args, (String, i32)> {
    let mut parsed = Args {
        ast_mode: false,
        interactive: false,
        src: None,
        profile: None,
    };
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--ast" => parsed.ast_mode = true,
            "-i" => parsed.interactive = true,
            "--profile" => {
                if parsed.profile.is_some() {
                    return Err(("fush: --profile given twice".into(), 2));
                }
                match iter.clone().next() {
                    Some(name) if !name.starts_with('-') => {
                        iter.next();
                        parsed.profile = Some(Some(name.clone()));
                    }
                    _ => parsed.profile = Some(None),
                }
            }
            "-c" => match iter.next().cloned() {
                Some(s) => parsed.src = Some(s),
                None => return Err(("fush: -c requires a command".into(), 2)),
            },
            "-h" | "--help" => {
                println!("usage: fush [--profile [NAME]] [--ast] [-c 'command'] [script]");
                println!("  --profile NAME   restrict the session to what NAME allows");
                println!("  --profile        read-only mode: no externals, no writes");
                return Err((String::new(), 0));
            }
            _ => match std::fs::read_to_string(arg) {
                Ok(text) => parsed.src = Some(text),
                Err(e) => {
                    return Err((format!("fush: cannot read '{arg}': {e}"), 1));
                }
            },
        }
    }
    Ok(parsed)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mut parsed = match parse_args(&args[1..]) {
        Ok(p) => p,
        Err((msg, code)) => {
            if !msg.is_empty() {
                eprintln!("{msg}");
            }
            std::process::exit(code);
        }
    };
    let src = parsed.src.unwrap_or_default();
    if parsed.ast_mode {
        run_ast_mode(&src);
        return;
    }
    if parsed.interactive || src.is_empty() {
        let profile = parsed.profile.take();
        std::process::exit(repl::run_repl(profile));
    }
    let program = match parser::parse(&src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    let mut rt = runtime::Runtime::new();
    if let Err(e) = match &parsed.profile {
        Some(name) => profile::load_profile(&mut rt, name.as_deref()),
        None => Ok(()),
    } {
        eprintln!("fush: profile: {e}");
        std::process::exit(2);
    }
    if let Some(path) = universal::universals_file() {
        universal::load(&mut rt, &path);
    }
    config::source_primary_config(&mut rt);
    let status = exec::run_program(&mut rt, &program.stmts);
    std::process::exit(status);
}

fn run_ast_mode(src: &str) {
    match parser::parse(src) {
        Ok(program) => println!("{program:#?}"),
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn script(name: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("fush-test-script-{name}.fush"));
        std::fs::write(&path, "echo hi").unwrap();
        path
    }

    #[test]
    fn profile_with_name_consumes_one_arg() {
        let script = script("named");
        let spath = script.to_string_lossy().into_owned();
        let parsed = parse_args(&args(&["--profile", "dev", &spath])).unwrap();
        assert_eq!(parsed.profile, Some(Some("dev".into())));
        assert_eq!(parsed.src.as_deref(), Some("echo hi"));
        std::fs::remove_file(&script).ok();
    }

    #[test]
    fn profile_alone_is_read_only() {
        let parsed = parse_args(&args(&["--profile"])).unwrap();
        assert_eq!(parsed.profile, Some(None));
    }

    #[test]
    fn profile_twice_is_error_exit_2() {
        let err = parse_args(&args(&["--profile", "--profile", "dev"])).unwrap_err();
        assert_eq!(err.1, 2);
    }

    #[test]
    fn profile_before_c_is_not_eaten_by_c() {
        let parsed = parse_args(&args(&["--profile", "dev", "-c", "echo hi"])).unwrap();
        assert_eq!(parsed.profile, Some(Some("dev".into())));
        assert_eq!(parsed.src.as_deref(), Some("echo hi"));
    }

    #[test]
    fn profile_flag_looking_name_means_read_only() {
        let parsed = parse_args(&args(&["--profile", "-c", "echo hi"])).unwrap();
        assert_eq!(parsed.profile, Some(None));
        assert_eq!(parsed.src.as_deref(), Some("echo hi"));
    }
}
