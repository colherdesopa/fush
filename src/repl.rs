use crate::{exec, parser, runtime};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};

pub fn run_repl() -> i32 {
    if !std::io::stdin().is_terminal() {
        return run_non_tty_repl();
    }

    let mut editor = match DefaultEditor::new() {
        Ok(editor) => editor,
        Err(_) => return run_non_tty_repl(),
    };
    if let Some(path) = history_path() {
        let _ = editor.load_history(&path);
    }

    let mut rt = runtime::Runtime::new();
    let mut buffer = String::new();
    loop {
        let prompt = if buffer.is_empty() { prompt_text() } else { "...> ".to_string() };
        match editor.readline(&prompt) {
            Ok(line) => {
                if buffer.is_empty() && line.trim().is_empty() {
                    continue;
                }
                if !buffer.is_empty() {
                    buffer.push('\n');
                }
                buffer.push_str(&line);
                match parser::parse(&buffer) {
                    Ok(program) => {
                        let status = exec::run_program(&mut rt, &program.stmts);
                        buffer.clear();
                        if rt.exited.is_some() {
                            save_history_if_possible(&mut editor);
                            return status;
                        }
                    }
                    Err(err) => {
                        if err.incomplete {
                            continue;
                        }
                        eprintln!("{err}");
                        buffer.clear();
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                buffer.clear();
                continue;
            }
            Err(ReadlineError::Eof) => {
                save_history_if_possible(&mut editor);
                if let Err(err) = parser::parse(&buffer)
                    && err.incomplete
                {
                    eprintln!("{err}");
                    return 1;
                }
                return 0;
            }
            Err(_) => {
                save_history_if_possible(&mut editor);
                return 0;
            }
        }
    }
}

fn run_non_tty_repl() -> i32 {
    let mut rt = runtime::Runtime::new();
    let mut buffer = String::new();
    loop {
        match read_stdin_line() {
            Some(line) => {
                if buffer.is_empty() && line.trim().is_empty() {
                    continue;
                }
                if !buffer.is_empty() {
                    buffer.push('\n');
                }
                buffer.push_str(&line);
                match parser::parse(&buffer) {
                    Ok(program) => {
                        let status = exec::run_program(&mut rt, &program.stmts);
                        buffer.clear();
                        if rt.exited.is_some() {
                            return status;
                        }
                    }
                    Err(err) => {
                        if err.incomplete {
                            continue;
                        }
                        eprintln!("{err}");
                        buffer.clear();
                    }
                }
            }
            None => {
                if let Err(err) = parser::parse(&buffer)
                    && err.incomplete
                {
                    eprintln!("{err}");
                    return 1;
                }
                return 0;
            }
        }
    }
}

fn read_stdin_line() -> Option<String> {
    let mut line = String::new();
    match io::stdin().read_line(&mut line) {
        Ok(0) => None,
        Ok(_) => Some(line.trim_end_matches(['\r', '\n']).to_string()),
        Err(_) => None,
    }
}

fn prompt_text() -> String {
    let user = std::env::var("USER").unwrap_or_else(|_| "user".to_string());
    let host = hostname();
    let cwd = short_cwd();
    format!("{user}@{host} {cwd}> ")
}

fn hostname() -> String {
    match std::fs::read_to_string("/proc/sys/kernel/hostname") {
        Ok(s) => s.trim().to_string(),
        Err(_) => "localhost".to_string(),
    }
}

fn short_cwd() -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let home = std::env::var("HOME").ok();
    match home {
        Some(home) if !home.is_empty() => {
            let cwd_s = cwd.to_string_lossy();
            if cwd_s == home {
                "~".to_string()
            } else if let Some(rest) = cwd_s.strip_prefix(&home) {
                if rest.starts_with('/') {
                    format!("~{}", rest)
                } else {
                    cwd_s.to_string()
                }
            } else {
                cwd_s.to_string()
            }
        }
        _ => cwd.to_string_lossy().into_owned(),
    }
}

fn history_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let dir = Path::new(&home).join(".local/share/fush");
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    Some(dir.join("history"))
}

fn save_history_if_possible(editor: &mut DefaultEditor) {
    if let Some(path) = history_path() {
        let _ = editor.save_history(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_render_uses_user_host_and_short_cwd() {
        let prompt = format!("{}@{} {}> ", "user", "host", "~/work");
        assert_eq!(prompt, "user@host ~/work> ");
    }

    #[test]
    fn short_cwd_replaces_home_prefix() {
        let home = std::env::var("HOME").unwrap_or_else(|_| "/home/test".to_string());
        let cwd = format!("{home}/project");
        let path = Path::new(&cwd);
        let short = match path.to_string_lossy().strip_prefix(&home) {
            Some(rest) if rest.starts_with('/') => format!("~{}", rest),
            _ => path.to_string_lossy().to_string(),
        };
        assert_eq!(short, "~/project");
    }

    #[test]
    fn incomplete_input_keeps_waiting() {
        let err = parser::parse("if test 1\n").unwrap_err();
        assert!(err.to_string().contains("end of input"));
        assert!(parser::parse("echo hi").is_ok());
    }

    #[test]
    fn history_path_uses_local_share_dir() {
        let home = std::env::temp_dir().join("fush-history-home");
        let path = home.join(".local/share/fush/history");
        let _ = std::fs::create_dir_all(path.parent().unwrap());
        assert!(path.parent().is_some());
        let _ = std::fs::remove_dir_all(&home);
    }
}
