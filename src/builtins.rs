use crate::completion::CompletionRule;
use crate::runtime::Runtime;

#[derive(Debug)]
pub enum BuiltinResult {
    /// Builtin ran in-process; status and optional captured stdout.
    Done { status: i32, out: Option<Vec<u8>> },
    /// `exit` was invoked; the shell should terminate with this code.
    Exit(i32),
}

const SET_USAGE: &str = "usage: set [-l | -U | -x] [-e] VARNAME [VALUES...]";

/// Dispatch to a builtin by name. `args` excludes the builtin name.
pub fn run(rt: &mut Runtime, name: &str, args: &[String]) -> Option<BuiltinResult> {
    match name {
        "set" => Some(set(rt, args)),
        "echo" => Some(echo(rt, args)),
        "cd" => Some(cd(rt, args)),
        "exit" => Some(exit(rt, args)),
        "test" => Some(test(rt, args)),
        "functions" => Some(functions(rt, args)),
        "return" => Some(return_builtin(rt, args)),
        "abbr" => Some(abbr(rt, args)),
        "true" => Some(true_builtin(rt, args)),
        "false" => Some(false_builtin(rt, args)),
        "complete" => Some(complete(rt, args)),
        "profile" => Some(profile_builtin(rt, args)),
        "status" => Some(status_builtin(rt, args)),
        "set_color" => Some(set_color(rt, args)),
        "prompt_pwd" => Some(prompt_pwd(rt, args)),
        "fish_add_path" => Some(fish_add_path(rt, args)),
        _ => None,
    }
}

fn abbr(rt: &mut Runtime, args: &[String]) -> BuiltinResult {
    if args.is_empty() {
        let mut lines: Vec<String> = rt
            .abbrs
            .iter()
            .map(|(name, expansion)| format!("{name} {expansion}"))
            .collect();
        lines.sort();
        let out = if lines.is_empty() {
            String::new()
        } else {
            format!("{}\n", lines.join("\n"))
        };
        return BuiltinResult::Done {
            status: 0,
            out: if out.is_empty() {
                None
            } else {
                Some(out.into_bytes())
            },
        };
    }

    let first = args[0].as_str();
    if first == "-l" || first == "--list" {
        let mut names: Vec<String> = rt.abbrs.keys().cloned().collect();
        names.sort();
        let out = if names.is_empty() {
            String::new()
        } else {
            format!("{}\n", names.join("\n"))
        };
        return BuiltinResult::Done {
            status: 0,
            out: if out.is_empty() {
                None
            } else {
                Some(out.into_bytes())
            },
        };
    }

    if first == "-e" || first == "--erase" {
        let Some(name) = args.get(1) else {
            return err("fush: abbr: missing abbreviation name");
        };
        if rt.abbrs.remove(name).is_none() {
            return BuiltinResult::Done {
                status: 1,
                out: None,
            };
        }
        return BuiltinResult::Done {
            status: 0,
            out: None,
        };
    }

    if first == "-a" || first == "--add" {
        let Some(name) = args.get(1) else {
            return err("fush: abbr: missing abbreviation name");
        };
        let value = args[2..].join(" ");
        rt.abbrs.insert(name.clone(), value);
        return BuiltinResult::Done {
            status: 0,
            out: None,
        };
    }

    if args.len() == 1 {
        let name = &args[0];
        return match rt.abbrs.get(name) {
            Some(expansion) => BuiltinResult::Done {
                status: 0,
                out: Some(format!("{name} {expansion}\n").into_bytes()),
            },
            None => BuiltinResult::Done {
                status: 1,
                out: None,
            },
        };
    }

    if !first.starts_with('-') {
        let name = &args[0];
        let value = args[1..].join(" ");
        rt.abbrs.insert(name.clone(), value);
        return BuiltinResult::Done {
            status: 0,
            out: None,
        };
    }

    err(&format!("fush: abbr: unknown option '{first}'"))
}

fn functions(rt: &mut Runtime, args: &[String]) -> BuiltinResult {
    if args.is_empty() {
        let mut names: Vec<String> = rt.functions.keys().cloned().collect();
        names.sort();
        let out = names.join("\n");
        return BuiltinResult::Done {
            status: 0,
            out: if out.is_empty() {
                None
            } else {
                Some(format!("{out}\n").into_bytes())
            },
        };
    }

    if args.len() != 1 {
        return err("fush: functions: expected zero or one argument");
    }

    let name = &args[0];
    let Some(func) = rt.functions.get(name) else {
        return BuiltinResult::Done {
            status: 1,
            out: Some(format!("function {name}\nend\n").into_bytes()),
        };
    };
    let params = if func.params.is_empty() {
        String::new()
    } else {
        format!(" {}", func.params.join(" "))
    };
    let out = format!("function {name}{params}\nend\n");
    BuiltinResult::Done {
        status: 0,
        out: Some(out.into_bytes()),
    }
}

fn return_builtin(rt: &mut Runtime, args: &[String]) -> BuiltinResult {
    let code = if args.is_empty() {
        rt.status
    } else if args.len() == 1 {
        match args[0].parse::<i32>() {
            Ok(v) => v,
            Err(_) => return err(&format!("fush: return: invalid return code '{}'", args[0])),
        }
    } else {
        return err("fush: return: too many arguments");
    };

    if rt.function_return_stack.is_empty() {
        rt.status = code;
        rt.env.set_global("status", vec![code.to_string()]);
        return BuiltinResult::Done {
            status: code,
            out: None,
        };
    }

    let slot = rt
        .function_return_stack
        .last_mut()
        .expect("return inside function");
    *slot = Some(code);
    rt.status = code;
    rt.env.set_global("status", vec![code.to_string()]);
    BuiltinResult::Done {
        status: code,
        out: None,
    }
}

fn true_builtin(_rt: &mut Runtime, _args: &[String]) -> BuiltinResult {
    BuiltinResult::Done {
        status: 0,
        out: None,
    }
}

fn false_builtin(_rt: &mut Runtime, _args: &[String]) -> BuiltinResult {
    BuiltinResult::Done {
        status: 1,
        out: None,
    }
}

fn set(rt: &mut Runtime, args: &[String]) -> BuiltinResult {
    let mut local = false;
    let mut exported = false;
    let mut universal = false;
    let mut erase = false;
    let mut rest: &[String] = args;
    while let Some(first) = rest.first() {
        match first.as_str() {
            "-l" => local = true,
            "-x" => exported = true,
            "-U" => universal = true,
            "-e" => erase = true,
            // fish's -g is fush's default scope; accept and ignore it
            "-g" => {}
            "-Ul" | "-lU" => {
                return err(&format!(
                    "fush: set: local universal variables are not supported\n{SET_USAGE}",
                ));
            }
            "-" | "--" => {
                rest = &rest[1..];
                break;
            }
            s if s.starts_with('-') && s.len() > 1 => {
                return err(&format!("fush: set: unknown flag '{s}'\n{SET_USAGE}"));
            }
            _ => break,
        }
        rest = &rest[1..];
    }
    if local && exported {
        return err("fush: set: -l and -x are mutually exclusive");
    }
    if local && universal {
        return err("fush: set: local universal variables are not supported\n{SET_USAGE}");
    }
    let Some(name) = rest.first() else {
        return err(&format!("fush: set: missing variable name\n{SET_USAGE}"));
    };
    let values = rest[1..].to_vec();
    if erase {
        rt.env.remove(name);
        if let Some(path) = crate::universal::universals_file() {
            crate::universal::erase(&path, name);
        }
        return BuiltinResult::Done {
            status: 0,
            out: None,
        };
    }
    if exported {
        rt.env.set_global(name, values.clone());
        rt.env.export(name);
    } else if local {
        rt.env.set_local(name, values.clone());
    } else {
        rt.env.set_global(name, values.clone());
    }
    if universal {
        let path = match crate::universal::universals_file() {
            Some(p) => p,
            None => return err("fush: set: cannot determine universals file path"),
        };
        if let Err(e) = crate::universal::save(rt, &path, name, &values) {
            return err(&e);
        }
    }
    BuiltinResult::Done {
        status: 0,
        out: None,
    }
}

#[cfg(test)]
fn set_universal_with_path(
    rt: &mut Runtime,
    path: &std::path::Path,
    name: &str,
    values: &[&str],
) -> i32 {
    let values: Vec<String> = values.iter().map(|s| s.to_string()).collect();
    rt.env.set_global(name, values.clone());
    match crate::universal::save(rt, path, name, &values) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

fn echo(_rt: &mut Runtime, args: &[String]) -> BuiltinResult {
    let mut no_newline = false;
    let mut no_space = false;
    let mut rest: &[String] = args;
    while let Some(first) = rest.first() {
        match first.as_str() {
            "-n" => no_newline = true,
            "-s" => no_space = true,
            _ => break,
        }
        rest = &rest[1..];
    }
    let sep = if no_space { "" } else { " " };
    let mut out = rest.join(sep);
    if !no_newline {
        out.push('\n');
    }
    BuiltinResult::Done {
        status: 0,
        out: Some(out.into_bytes()),
    }
}

fn cd(rt: &mut Runtime, args: &[String]) -> BuiltinResult {
    let target = match args.first() {
        Some(p) => p.clone(),
        None => match rt.env.get("HOME") {
            Some(vals) if !vals.is_empty() => vals[0].clone(),
            _ => return err("fush: cd: $HOME not set"),
        },
    };
    match std::fs::canonicalize(&target) {
        Ok(path) if path.is_dir() => {
            if std::env::set_current_dir(&path).is_err() {
                return err(&format!("fush: cd: cannot change to '{}'", path.display()));
            }
            let shown = crate::runtime::strip_verbatim_prefix(&path.to_string_lossy());
            rt.env.set_global("PWD", vec![shown]);
            BuiltinResult::Done {
                status: 0,
                out: None,
            }
        }
        Ok(_) => err(&format!("fush: cd: '{}' is not a directory", target)),
        Err(_) => err(&format!("fush: cd: no such file or directory: {}", target)),
    }
}

fn exit(rt: &mut Runtime, args: &[String]) -> BuiltinResult {
    let code = match args.first() {
        Some(s) => match s.parse::<i32>() {
            Ok(n) => n,
            Err(_) => return err(&format!("fush: exit: invalid exit code '{}'", s)),
        },
        None => rt.status,
    };
    BuiltinResult::Exit(code)
}

fn test(_rt: &mut Runtime, args: &[String]) -> BuiltinResult {
    let status = match eval_test(args) {
        Ok(b) => {
            if b {
                0
            } else {
                1
            }
        }
        Err(msg) => return err(&format!("fush: test: {msg}")),
    };
    BuiltinResult::Done { status, out: None }
}

fn eval_test(args: &[String]) -> Result<bool, String> {
    if args.first().map(|s| s.as_str()) == Some("!") {
        return Ok(!eval_test(&args[1..])?);
    }
    match args.len() {
        0 => Ok(false),
        1 => Ok(!args[0].is_empty()),
        2 => {
            let op = args[0].as_str();
            let val = args[1].as_str();
            match op {
                "-n" => Ok(!val.is_empty()),
                "-z" => Ok(val.is_empty()),
                "-f" => Ok(std::fs::metadata(val).map(|m| m.is_file()).unwrap_or(false)),
                "-d" => Ok(std::fs::metadata(val).map(|m| m.is_dir()).unwrap_or(false)),
                "-x" => Ok(is_executable(val)),
                _ => Err(format!("unknown operator '{op}'")),
            }
        }
        3 => {
            let a = args[0].as_str();
            let op = args[1].as_str();
            let b = args[2].as_str();
            match op {
                "=" => Ok(a == b),
                "!=" => Ok(a != b),
                "-eq" => int_cmp(a, b, |x, y| x == y),
                "-ne" => int_cmp(a, b, |x, y| x != y),
                "-lt" => int_cmp(a, b, |x, y| x < y),
                "-le" => int_cmp(a, b, |x, y| x <= y),
                "-gt" => int_cmp(a, b, |x, y| x > y),
                "-ge" => int_cmp(a, b, |x, y| x >= y),
                _ => Err(format!("unknown operator '{op}'")),
            }
        }
        _ => Err(format!("too many arguments ({})", args.len())),
    }
}

fn int_cmp(a: &str, b: &str, f: impl Fn(i64, i64) -> bool) -> Result<bool, String> {
    let x: i64 = a
        .parse()
        .map_err(|_| format!("integer expression expected, got '{a}'"))?;
    let y: i64 = b
        .parse()
        .map_err(|_| format!("integer expression expected, got '{b}'"))?;
    Ok(f(x, y))
}

#[cfg(unix)]
fn is_executable(path: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(path)
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(_path: &str) -> bool {
    false
}

fn complete(rt: &mut Runtime, args: &[String]) -> BuiltinResult {
    let mut cmd: Option<String> = None;
    let mut erase = false;
    let mut suppress_files = false;
    let mut candidates: Vec<String> = Vec::new();
    let mut description: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-c" => {
                i += 1;
                match args.get(i) {
                    Some(v) => cmd = Some(v.clone()),
                    None => return err("complete: -c requires a command name"),
                }
            }
            "-e" => erase = true,
            "-f" => suppress_files = true,
            "-a" => {
                i += 1;
                match args.get(i) {
                    Some(v) => candidates.extend(split_unquoted_spaces(v)),
                    None => return err("complete: -a requires a candidate list"),
                }
            }
            "-d" => {
                i += 1;
                match args.get(i) {
                    Some(v) => description = Some(v.clone()),
                    None => return err("complete: -d requires a description"),
                }
            }
            other => {
                let _ = other;
                return err("complete: unknown option");
            }
        }
        i += 1;
    }
    let Some(cmd) = cmd else {
        return err("complete: -c requires a command name");
    };
    if erase {
        rt.completions.remove(&cmd);
        return BuiltinResult::Done {
            status: 0,
            out: None,
        };
    }
    let rule = CompletionRule {
        candidates,
        suppress_files,
        dirs_only: false,
        description,
    };
    rt.completions.entry(cmd).or_default().push(rule);
    BuiltinResult::Done {
        status: 0,
        out: None,
    }
}

/// Word-split on unquoted spaces: quotes group words, quote characters
/// themselves are stripped (fish strips them; quoted candidate forms are
/// not required this milestone).
fn split_unquoted_spaces(value: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    for ch in value.chars() {
        match quote {
            Some(q) => {
                if ch == q {
                    quote = None;
                } else {
                    cur.push(ch);
                }
            }
            None => {
                if ch == '\"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch.is_whitespace() {
                    if !cur.is_empty() {
                        parts.push(std::mem::take(&mut cur));
                    }
                } else {
                    cur.push(ch);
                }
            }
        }
    }
    if !cur.is_empty() {
        parts.push(cur);
    }
    parts
}

fn profile_builtin(rt: &mut Runtime, args: &[String]) -> BuiltinResult {
    if !rt.profile_loading {
        return err("fush: profile: only valid while loading a profile");
    }
    let Some(first) = args.first() else {
        return err("fush: profile: expected 'allow CMD...' or 'allow-write PATH...'");
    };
    let Some(profile) = rt.profile.as_mut() else {
        return err("fush: profile: only valid while loading a profile");
    };
    match first.as_str() {
        "allow" => {
            if args.len() < 2 {
                return err("fush: profile: expected 'allow CMD...' or 'allow-write PATH...'");
            }
            for name in &args[1..] {
                profile.allow.insert(name.clone());
            }
        }
        "allow-write" => {
            if args.len() < 2 {
                return err("fush: profile: expected 'allow CMD...' or 'allow-write PATH...'");
            }
            for path in &args[1..] {
                profile.allow_write.insert(path.clone());
            }
        }
        _ => {
            return err("fush: profile: expected 'allow CMD...' or 'allow-write PATH...'");
        }
    }
    BuiltinResult::Done {
        status: 0,
        out: None,
    }
}

const STATUS_USAGE: &str = "usage: status [is-interactive | is-command-substitution]";

fn status_builtin(rt: &mut Runtime, args: &[String]) -> BuiltinResult {
    match args.first().map(String::as_str) {
        None => {
            let word = if rt.interactive {
                "interactive"
            } else {
                "noninteractive"
            };
            BuiltinResult::Done {
                status: 0,
                out: Some(format!("{word}\n").into_bytes()),
            }
        }
        Some("is-interactive") => BuiltinResult::Done {
            status: if rt.interactive { 0 } else { 1 },
            out: None,
        },
        Some("is-command-substitution") => BuiltinResult::Done {
            status: if rt.capture.is_some() { 0 } else { 1 },
            out: None,
        },
        Some(other) => {
            eprintln!("fush: status: unknown subcommand '{other}'\n{STATUS_USAGE}");
            BuiltinResult::Done {
                status: 2,
                out: None,
            }
        }
    }
}

/// Map a fish color name (or `colorNNN` 256-color form) to its SGR code.
fn color_code(name: &str) -> Option<&'static str> {
    match name {
        "normal" => Some("0"),
        "black" => Some("30"),
        "red" => Some("31"),
        "green" => Some("32"),
        "yellow" => Some("33"),
        "blue" => Some("34"),
        "magenta" => Some("35"),
        "cyan" => Some("36"),
        "white" => Some("37"),
        "brblack" => Some("90"),
        "brred" => Some("91"),
        "brgreen" => Some("92"),
        "bryellow" => Some("93"),
        "brblue" => Some("94"),
        "brmagenta" => Some("95"),
        "brcyan" => Some("96"),
        "brwhite" => Some("97"),
        _ => {
            let digits = name.strip_prefix("color")?;
            if !digits.is_empty() && digits.len() <= 3 && digits.bytes().all(|b| b.is_ascii_digit())
            {
                // 256-color codes are dynamic; fall through to caller via
                // the owned form below.
                return None;
            }
            None
        }
    }
}

/// Owned SGR code for `colorNNN` (256-color palette).
fn color_code_owned(name: &str) -> Option<String> {
    let digits = name.strip_prefix("color")?;
    if !digits.is_empty() && digits.len() <= 3 && digits.bytes().all(|b| b.is_ascii_digit()) {
        Some(format!("38;5;{digits}"))
    } else {
        None
    }
}

fn set_color(_rt: &mut Runtime, args: &[String]) -> BuiltinResult {
    let mut bold = false;
    let mut italic = false;
    let mut underline = false;
    let mut dim = false;
    let mut color: Option<String> = None;
    for arg in args {
        match arg.as_str() {
            "-b" | "--bold" | "-o" => bold = true,
            "-i" | "--italic" => italic = true,
            "-u" | "--underline" => underline = true,
            "-d" | "--dim" => dim = true,
            s if s.starts_with('-') => continue,
            s => {
                let code = color_code(s)
                    .map(str::to_string)
                    .or_else(|| color_code_owned(s));
                match code {
                    Some(code) => color = Some(code),
                    None => {
                        return err(&format!("fush: set_color: unknown color '{s}'"));
                    }
                }
            }
        }
    }
    let Some(color) = color else {
        return err("fush: set_color: expected a color");
    };
    let mut codes = Vec::new();
    if bold {
        codes.push("1".to_string());
    }
    if dim {
        codes.push("2".to_string());
    }
    if italic {
        codes.push("3".to_string());
    }
    if underline {
        codes.push("4".to_string());
    }
    codes.push(color);
    BuiltinResult::Done {
        status: 0,
        out: Some(format!("\x1b[{}m", codes.join(";")).into_bytes()),
    }
}

/// Render fish's `prompt_pwd` short form: `~` for the home prefix, all
/// directory components except the last two truncated to one character.
fn prompt_pwd_format(cwd: &str, home: Option<&str>) -> String {
    let normalize = |s: &str| s.replace('\\', "/");
    let cwd = normalize(cwd);
    let home = home.map(normalize);
    let mut components: Vec<String> = Vec::new();
    let mut rest = cwd.as_str();
    if let Some(home) = &home
        && !home.is_empty()
    {
        if rest == home {
            return "~".to_string();
        }
        if let Some(stripped) = rest.strip_prefix(home.as_str())
            && stripped.starts_with('/')
        {
            components.push("~".to_string());
            rest = stripped;
        }
    }
    components.extend(
        rest.split('/')
            .filter(|c| !c.is_empty())
            .map(str::to_string),
    );
    let absolute = cwd.starts_with('/');
    let full = components.len().saturating_sub(2);
    let parts: Vec<String> = components
        .into_iter()
        .enumerate()
        .map(|(i, c)| {
            if i < full {
                c.chars().take(1).collect()
            } else {
                c
            }
        })
        .collect();
    let joined = parts.join("/");
    if absolute && !joined.starts_with('~') {
        format!("/{joined}")
    } else {
        joined
    }
}

fn prompt_pwd(_rt: &mut Runtime, _args: &[String]) -> BuiltinResult {
    let cwd = std::env::current_dir()
        .map(|p| crate::runtime::strip_verbatim_prefix(&p.to_string_lossy()))
        .unwrap_or_else(|_| ".".to_string());
    // On Windows HOME is often unset; fall back to USERPROFILE like fish.
    let home = std::env::var("HOME")
        .ok()
        .filter(|h| !h.is_empty())
        .or_else(|| std::env::var("USERPROFILE").ok());
    let out = prompt_pwd_format(&cwd, home.as_deref());
    BuiltinResult::Done {
        status: 0,
        out: Some(format!("{out}\n").into_bytes()),
    }
}

fn fish_add_path(rt: &mut Runtime, args: &[String]) -> BuiltinResult {
    let mut paths: Vec<String> = Vec::new();
    for arg in args {
        if arg.starts_with('-') {
            continue;
        }
        paths.push(arg.clone());
    }
    let existing = rt.env.get("PATH").unwrap_or_default();
    let mut new_front: Vec<String> = Vec::new();
    for path in &paths {
        let dir = std::path::Path::new(path);
        if !dir.is_dir() {
            continue;
        }
        if existing.iter().any(|e| e == path) || new_front.iter().any(|e| e == path) {
            continue;
        }
        new_front.push(path.clone());
    }
    if !new_front.is_empty() {
        new_front.extend(existing.iter().cloned());
        rt.env.set_global("PATH", new_front.clone());
        rt.env.export("PATH");
    }
    BuiltinResult::Done {
        status: 0,
        out: None,
    }
}

fn err(msg: &str) -> BuiltinResult {
    eprintln!("{msg}");
    BuiltinResult::Done {
        status: 1,
        out: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_builtin_accumulates_allows() {
        let mut rt = Runtime::new();
        rt.profile = Some(crate::runtime::Profile::default());
        rt.profile_loading = true;
        profile_builtin(&mut rt, &args(&["allow", "git", "ls"]));
        profile_builtin(&mut rt, &args(&["allow-write", "/tmp/work", "/tmp/out"]));
        let p = rt.profile.as_ref().unwrap();
        assert!(p.allow.contains("git") && p.allow.contains("ls"));
        assert!(p.allow_write.contains("/tmp/work"));
        assert_eq!(p.allow.len(), 2);
    }

    #[test]
    fn profile_builtin_rejects_outside_loading() {
        let mut rt = Runtime::new();
        rt.profile_loading = false;
        let r = profile_builtin(&mut rt, &args(&["allow", "git"]));
        match r {
            BuiltinResult::Done {
                status: 1,
                out: None,
            } => {}
            other => panic!("expected error result, got {other:?}"),
        }
    }

    fn assert_usage_error(r: BuiltinResult) {
        match r {
            BuiltinResult::Done {
                status: 1,
                out: None,
            } => {}
            other => panic!("expected usage error, got {other:?}"),
        }
    }

    #[test]
    fn profile_builtin_usage_errors() {
        let mut rt = Runtime::new();
        rt.profile = Some(crate::runtime::Profile::default());
        rt.profile_loading = true;
        assert_usage_error(profile_builtin(&mut rt, &args(&[])));
        assert_usage_error(profile_builtin(&mut rt, &args(&["deny", "x"])));
        assert_usage_error(profile_builtin(&mut rt, &args(&["allow"])));
        assert_usage_error(profile_builtin(&mut rt, &args(&["allow-write"])));
    }

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    fn status_of(rt: &mut Runtime, name: &str, list: &[&str]) -> i32 {
        match run(rt, name, &args(list)) {
            Some(BuiltinResult::Done { status, .. }) => status,
            _ => panic!("expected Done"),
        }
    }

    #[test]
    fn status_is_interactive_tracks_runtime_flag() {
        let mut rt = Runtime::new();
        rt.interactive = false;
        assert_eq!(status_of(&mut rt, "status", &["is-interactive"]), 1);
        rt.interactive = true;
        assert_eq!(status_of(&mut rt, "status", &["is-interactive"]), 0);
    }

    #[test]
    fn status_bare_prints_interactive_word() {
        let mut rt = Runtime::new();
        rt.interactive = true;
        match run(&mut rt, "status", &args(&[])) {
            Some(BuiltinResult::Done {
                status: 0,
                out: Some(out),
            }) => {
                assert_eq!(out, b"interactive\n");
            }
            other => panic!("expected Done with output, got {other:?}"),
        }
        rt.interactive = false;
        match run(&mut rt, "status", &args(&[])) {
            Some(BuiltinResult::Done {
                status: 0,
                out: Some(out),
            }) => {
                assert_eq!(out, b"noninteractive\n");
            }
            other => panic!("expected Done with output, got {other:?}"),
        }
    }

    #[test]
    fn status_is_command_substitution_tracks_capture() {
        let mut rt = Runtime::new();
        assert_eq!(
            status_of(&mut rt, "status", &["is-command-substitution"]),
            1
        );
        rt.capture = Some(Vec::new());
        assert_eq!(
            status_of(&mut rt, "status", &["is-command-substitution"]),
            0
        );
        rt.capture = None;
        assert_eq!(
            status_of(&mut rt, "status", &["is-command-substitution"]),
            1
        );
    }

    #[test]
    fn status_unknown_subcommand_is_status_two() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "status", &["bogus"]), 2);
    }

    fn out_of(rt: &mut Runtime, name: &str, list: &[&str]) -> Vec<u8> {
        match run(rt, name, &args(list)) {
            Some(BuiltinResult::Done { out: Some(out), .. }) => out,
            other => panic!("expected Done with output, got {other:?}"),
        }
    }

    #[test]
    fn set_color_basic_and_bright_names() {
        let mut rt = Runtime::new();
        assert_eq!(out_of(&mut rt, "set_color", &["red"]), b"\x1b[31m");
        assert_eq!(out_of(&mut rt, "set_color", &["brblue"]), b"\x1b[94m");
        assert_eq!(out_of(&mut rt, "set_color", &["normal"]), b"\x1b[0m");
    }

    #[test]
    fn set_color_256_color_form() {
        let mut rt = Runtime::new();
        assert_eq!(
            out_of(&mut rt, "set_color", &["color123"]),
            b"\x1b[38;5;123m"
        );
        assert_eq!(status_of(&mut rt, "set_color", &["color9999"]), 1);
    }

    #[test]
    fn set_color_flags_add_attributes_and_last_color_wins() {
        let mut rt = Runtime::new();
        assert_eq!(
            out_of(&mut rt, "set_color", &["--bold", "green"]),
            b"\x1b[1;32m"
        );
        assert_eq!(
            out_of(&mut rt, "set_color", &["-i", "-u", "-b", "cyan"]),
            b"\x1b[1;3;4;36m"
        );
        assert_eq!(out_of(&mut rt, "set_color", &["red", "blue"]), b"\x1b[34m");
    }

    #[test]
    fn set_color_unknown_color_errors() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "set_color", &["chartreuse"]), 1);
        assert_eq!(status_of(&mut rt, "set_color", &[]), 1);
    }

    #[test]
    fn prompt_pwd_format_truncates_all_but_last_two() {
        assert_eq!(
            prompt_pwd_format("/usr/local/share/fish", None),
            "/u/l/share/fish"
        );
        assert_eq!(
            prompt_pwd_format("/home/user/project/src", Some("/home/user")),
            "~/project/src"
        );
        assert_eq!(
            prompt_pwd_format("/home/user/project/deep/src", Some("/home/user")),
            "~/p/deep/src"
        );
        assert_eq!(prompt_pwd_format("/home/user", Some("/home/user")), "~");
        assert_eq!(
            prompt_pwd_format("/home/user/a/b", Some("/home/user")),
            "~/a/b"
        );
        assert_eq!(prompt_pwd_format("/usr", None), "/usr");
        assert_eq!(prompt_pwd_format("/usr/bin", None), "/usr/bin");
    }

    #[test]
    fn prompt_pwd_prints_short_form_with_newline() {
        let mut rt = Runtime::new();
        let out = out_of(&mut rt, "prompt_pwd", &[]);
        let text = String::from_utf8(out).unwrap();
        assert!(text.ends_with('\n'));
        let cwd = std::env::current_dir().unwrap();
        let expected = prompt_pwd_format(
            &crate::runtime::strip_verbatim_prefix(&cwd.to_string_lossy()),
            std::env::var("HOME")
                .ok()
                .filter(|h| !h.is_empty())
                .or_else(|| std::env::var("USERPROFILE").ok())
                .as_deref(),
        );
        assert_eq!(text, format!("{expected}\n"));
    }

    #[test]
    fn fish_add_path_prepends_existing_dirs_without_duplicates() {
        let mut rt = Runtime::new();
        rt.env
            .set_global("PATH", vec!["/bin".into(), "/usr/bin".into()]);
        let dir = std::env::temp_dir().to_string_lossy().to_string();
        status_of(
            &mut rt,
            "fish_add_path",
            &["-g", &dir, "/nonexistent-dir-xyz"],
        );
        assert_eq!(
            rt.env.get("PATH"),
            Some(vec![dir.clone(), "/bin".into(), "/usr/bin".into()])
        );
        status_of(&mut rt, "fish_add_path", &[&dir]);
        assert_eq!(
            rt.env.get("PATH"),
            Some(vec![dir, "/bin".into(), "/usr/bin".into()])
        );
    }

    #[test]
    fn true_builtin_exits_zero_and_ignores_args() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "true", &[]), 0);
        assert_eq!(status_of(&mut rt, "true", &["extra"]), 0);
    }

    #[test]
    fn false_builtin_exits_one_and_ignores_args() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "false", &[]), 1);
        assert_eq!(status_of(&mut rt, "false", &["extra"]), 1);
    }

    #[test]
    fn set_global_single_value() {
        let mut rt = Runtime::new();
        status_of(&mut rt, "set", &["x", "1"]);
        assert_eq!(rt.env.get("x"), Some(vec!["1".into()]));
    }

    #[test]
    fn set_global_list_values() {
        let mut rt = Runtime::new();
        status_of(&mut rt, "set", &["x", "1", "2", "3"]);
        assert_eq!(
            rt.env.get("x"),
            Some(vec!["1".into(), "2".into(), "3".into()])
        );
    }

    #[test]
    fn set_local_goes_to_local_scope() {
        let mut rt = Runtime::new();
        rt.env.push_scope();
        status_of(&mut rt, "set", &["-l", "y", "1"]);
        assert_eq!(rt.env.get("y"), Some(vec!["1".into()]));
        rt.env.pop_scope();
        assert_eq!(rt.env.get("y"), None);
    }

    #[test]
    fn set_export_marks_and_sets_global() {
        let mut rt = Runtime::new();
        status_of(&mut rt, "set", &["-x", "FOO", "bar"]);
        assert!(rt.env.is_exported("FOO"));
        assert_eq!(rt.env.get("FOO"), Some(vec!["bar".into()]));
    }

    #[test]
    fn set_universal_persists_and_erases() {
        let dir = std::env::temp_dir().join(format!(
            "fush-setu-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("universals");
        let mut rt = Runtime::new();
        assert_eq!(set_universal_with_path(&mut rt, &path, "x", &["1", "2"]), 0);
        assert_eq!(rt.env.get("x"), Some(vec!["1".into(), "2".into()]));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "x\t1\t2\n");
        // Update an existing universal: env + file.
        assert_eq!(set_universal_with_path(&mut rt, &path, "x", &["9"]), 0);
        assert_eq!(rt.env.get("x"), Some(vec!["9".into()]));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "x\t9\n");
        // Erase: env + file (set -e path with the injected universals file).
        rt.env.remove("x");
        crate::universal::erase(&path, "x");
        assert_eq!(rt.env.get("x"), None);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn set_ul_local_universal_rejected() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "set", &["-Ul", "x", "1"]), 1);
        assert_eq!(rt.env.get("x"), None);
    }

    #[test]
    fn set_rejects_unknown_flag() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "set", &["-q", "x"]), 1);
        assert_eq!(rt.env.get("x"), None);
    }

    #[test]
    fn set_rejects_missing_name() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "set", &[]), 1);
        assert_eq!(status_of(&mut rt, "set", &["-l"]), 1);
    }

    #[test]
    fn set_rejects_conflicting_flags() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "set", &["-l", "-x", "x"]), 1);
    }

    #[test]
    fn echo_joins_with_spaces_and_newline() {
        let mut rt = Runtime::new();
        match run(&mut rt, "echo", &args(&["a", "b", "c"])) {
            Some(BuiltinResult::Done { out: Some(out), .. }) => {
                assert_eq!(String::from_utf8(out).unwrap(), "a b c\n")
            }
            _ => panic!("expected output"),
        }
    }

    #[test]
    fn echo_no_newline_flag() {
        let mut rt = Runtime::new();
        match run(&mut rt, "echo", &args(&["-n", "hi"])) {
            Some(BuiltinResult::Done { out: Some(out), .. }) => {
                assert_eq!(String::from_utf8(out).unwrap(), "hi")
            }
            _ => panic!("expected output"),
        }
    }

    #[test]
    fn echo_no_space_flag() {
        let mut rt = Runtime::new();
        match run(&mut rt, "echo", &args(&["-s", "a", "b"])) {
            Some(BuiltinResult::Done { out: Some(out), .. }) => {
                assert_eq!(String::from_utf8(out).unwrap(), "ab\n")
            }
            _ => panic!("expected output"),
        }
    }

    #[test]
    fn echo_empty_produces_newline_only() {
        let mut rt = Runtime::new();
        match run(&mut rt, "echo", &args(&[])) {
            Some(BuiltinResult::Done { out: Some(out), .. }) => {
                assert_eq!(String::from_utf8(out).unwrap(), "\n")
            }
            _ => panic!("expected output"),
        }
    }

    #[test]
    fn cd_sets_pwd() {
        let mut rt = Runtime::new();
        let tmp = std::env::temp_dir();
        let status = status_of(&mut rt, "cd", &[&tmp.to_string_lossy()]);
        assert_eq!(status, 0);
        assert_eq!(
            rt.env.get("PWD"),
            Some(vec![crate::runtime::strip_verbatim_prefix(
                &std::fs::canonicalize(&tmp).unwrap().to_string_lossy()
            )])
        );
    }

    #[test]
    fn cd_fails_on_file() {
        let mut rt = Runtime::new();
        let file = std::env::temp_dir().join("fush-cd-file-probe");
        std::fs::write(&file, "x").unwrap();
        let file_str = file.to_string_lossy().into_owned();
        assert_eq!(status_of(&mut rt, "cd", &[&file_str]), 1);
        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn cd_fails_on_missing() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "cd", &["/nonexistent-xyz-123"]), 1);
    }

    #[test]
    fn exit_uses_given_code() {
        let mut rt = Runtime::new();
        match run(&mut rt, "exit", &args(&["3"])) {
            Some(BuiltinResult::Exit(3)) => {}
            _ => panic!("expected Exit(3)"),
        }
    }

    #[test]
    fn exit_defaults_to_status() {
        let mut rt = Runtime::new();
        rt.status = 7;
        match run(&mut rt, "exit", &args(&[])) {
            Some(BuiltinResult::Exit(7)) => {}
            _ => panic!("expected Exit(7)"),
        }
    }

    #[test]
    fn exit_rejects_non_numeric() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "exit", &["abc"]), 1);
    }

    #[test]
    fn test_string_ops() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "test", &["a", "=", "a"]), 0);
        assert_eq!(status_of(&mut rt, "test", &["a", "=", "b"]), 1);
        assert_eq!(status_of(&mut rt, "test", &["a", "!=", "b"]), 0);
        assert_eq!(status_of(&mut rt, "test", &["-n", "x"]), 0);
        assert_eq!(status_of(&mut rt, "test", &["-n", ""]), 1);
        assert_eq!(status_of(&mut rt, "test", &["-z", ""]), 0);
        assert_eq!(status_of(&mut rt, "test", &["-z", "x"]), 1);
    }

    #[test]
    fn test_int_ops() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "test", &["1", "-eq", "1"]), 0);
        assert_eq!(status_of(&mut rt, "test", &["1", "-ne", "2"]), 0);
        assert_eq!(status_of(&mut rt, "test", &["1", "-lt", "2"]), 0);
        assert_eq!(status_of(&mut rt, "test", &["2", "-le", "2"]), 0);
        assert_eq!(status_of(&mut rt, "test", &["3", "-gt", "2"]), 0);
        assert_eq!(status_of(&mut rt, "test", &["2", "-ge", "2"]), 0);
        assert_eq!(status_of(&mut rt, "test", &["2", "-lt", "1"]), 1);
        assert_eq!(status_of(&mut rt, "test", &["x", "-eq", "1"]), 1);
    }

    #[test]
    fn test_file_ops() {
        let mut rt = Runtime::new();
        let dir = std::env::temp_dir();
        let file = dir.join("fush-test-file-probe");
        std::fs::write(&file, "x").unwrap();
        let file_str = file.to_string_lossy().into_owned();
        let dir_str = dir.to_string_lossy().into_owned();
        assert_eq!(status_of(&mut rt, "test", &["-f", &file_str]), 0);
        assert_eq!(status_of(&mut rt, "test", &["-f", &dir_str]), 1);
        assert_eq!(status_of(&mut rt, "test", &["-d", &dir_str]), 0);
        assert_eq!(status_of(&mut rt, "test", &["-d", &file_str]), 1);
        assert_eq!(status_of(&mut rt, "test", &["-f", "/nonexistent-xyz"]), 1);
        std::fs::remove_file(&file).ok();
    }

    #[test]
    fn test_negation() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "test", &["!", "a", "=", "a"]), 1);
        assert_eq!(status_of(&mut rt, "test", &["!", "a", "=", "b"]), 0);
    }

    #[test]
    fn test_unknown_operator_errors() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "test", &["a", "-foo", "b"]), 1);
    }

    #[test]
    fn functions_lists_names_sorted() {
        let mut rt = Runtime::new();
        rt.register_function("zeta", vec![], vec![]);
        rt.register_function("alpha", vec![], vec![]);
        match run(&mut rt, "functions", &[]) {
            Some(BuiltinResult::Done { out: Some(out), .. }) => {
                assert_eq!(String::from_utf8(out).unwrap(), "alpha\nzeta\n");
            }
            _ => panic!("expected functions output"),
        }
    }

    #[test]
    fn abbr_adds_lists_and_overwrites() {
        let mut rt = Runtime::new();
        assert_eq!(
            status_of(&mut rt, "abbr", &["-a", "gs", "git", "status"]),
            0
        );
        assert_eq!(
            status_of(&mut rt, "abbr", &["-a", "gs", "git", "status", "--short"]),
            0
        );
        assert_eq!(rt.abbrs.get("gs"), Some(&"git status --short".to_string()));

        match run(&mut rt, "abbr", &[]) {
            Some(BuiltinResult::Done { out: Some(out), .. }) => {
                assert_eq!(String::from_utf8(out).unwrap(), "gs git status --short\n");
            }
            _ => panic!("expected abbr list output"),
        }
    }

    #[test]
    fn abbr_list_names_only() {
        let mut rt = Runtime::new();
        rt.abbrs.insert("zeta".to_string(), "echo z".to_string());
        rt.abbrs.insert("alpha".to_string(), "echo a".to_string());
        match run(&mut rt, "abbr", &["-l".to_string()]) {
            Some(BuiltinResult::Done { out: Some(out), .. }) => {
                assert_eq!(String::from_utf8(out).unwrap(), "alpha\nzeta\n");
            }
            _ => panic!("expected name list output"),
        }
    }

    #[test]
    fn abbr_erase_removes_and_statuses_missing() {
        let mut rt = Runtime::new();
        rt.abbrs.insert("gs".to_string(), "git status".to_string());
        assert_eq!(status_of(&mut rt, "abbr", &["-e", "gs"]), 0);
        assert_eq!(rt.abbrs.get("gs"), None);
        assert_eq!(status_of(&mut rt, "abbr", &["-e", "missing"]), 1);
    }

    #[test]
    fn builtin_wins_over_function_name() {
        let mut rt = Runtime::new();
        rt.register_function("echo", vec![], vec![]);
        match run(&mut rt, "echo", &args(&["hello"])) {
            Some(BuiltinResult::Done { out: Some(out), .. }) => {
                assert_eq!(String::from_utf8(out).unwrap(), "hello\n");
            }
            _ => panic!("expected builtin echo output"),
        }
    }

    #[test]
    fn unknown_builtin_returns_none() {
        let mut rt = Runtime::new();
        assert!(run(&mut rt, "definitely-not-a-builtin", &[]).is_none());
    }

    #[test]
    fn complete_registers_and_accumulates_rules() {
        let mut rt = Runtime::new();
        assert_eq!(
            status_of(&mut rt, "complete", &["-c", "probe", "-a", "one two"]),
            0
        );
        assert_eq!(
            status_of(&mut rt, "complete", &["-c", "probe", "-a", "three"]),
            0
        );
        let rules = &rt.completions["probe"];
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].candidates, vec!["one", "two"]);
        assert_eq!(rules[1].candidates, vec!["three"]);
    }

    #[test]
    fn complete_e_erases_rules() {
        let mut rt = Runtime::new();
        assert_eq!(
            status_of(&mut rt, "complete", &["-c", "probe", "-a", "one"]),
            0
        );
        assert_eq!(status_of(&mut rt, "complete", &["-c", "probe", "-e"]), 0);
        assert!(!rt.completions.contains_key("probe"));
        assert_eq!(status_of(&mut rt, "complete", &["-c", "probe", "-e"]), 0);
    }

    #[test]
    fn complete_f_sets_suppression_and_d_stores_description() {
        let mut rt = Runtime::new();
        assert_eq!(
            status_of(
                &mut rt,
                "complete",
                &["-c", "probe", "-f", "-d", "the desc"]
            ),
            0
        );
        let rules = &rt.completions["probe"];
        assert!(rules[0].suppress_files);
        assert_eq!(rules[0].description.as_deref(), Some("the desc"));
        assert!(rules[0].candidates.is_empty());
    }

    #[test]
    fn complete_a_word_splits_on_unquoted_spaces() {
        let mut rt = Runtime::new();
        status_of(&mut rt, "complete", &["-c", "probe", "-a", "a b  c"]);
        assert_eq!(rt.completions["probe"][0].candidates, vec!["a", "b", "c"]);
        status_of(&mut rt, "complete", &["-c", "probe", "-a", "\"x y\" z"]);
        assert_eq!(rt.completions["probe"][1].candidates, vec!["x y", "z"]);
    }

    #[test]
    fn complete_without_rules_and_without_a_is_pure_registration() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "complete", &["-c", "probe"]), 0);
        assert!(rt.completions["probe"][0].candidates.is_empty());
    }

    #[test]
    fn complete_requires_c_and_known_options() {
        let mut rt = Runtime::new();
        assert_eq!(status_of(&mut rt, "complete", &[]), 1);
        assert_eq!(status_of(&mut rt, "complete", &["-c"]), 1);
        assert_eq!(status_of(&mut rt, "complete", &["-c", "p", "-z"]), 1);
    }

    #[test]
    fn default_completions_registered_at_startup() {
        let rt = Runtime::new();
        let set_rules = &rt.completions["set"];
        assert!(set_rules[0].candidates.contains(&"-l".to_string()));
        assert!(set_rules[0].candidates.contains(&"-q".to_string()));
        let cd_rules = &rt.completions["cd"];
        assert!(cd_rules[0].suppress_files);
        assert!(cd_rules[0].dirs_only);
    }
}
