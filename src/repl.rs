use crate::completion::{self, BUILTIN_NAMES, CompletionEngine, CompletionRule};
use crate::{exec, highlight, parser, runtime, suggest::Suggester};
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::{Context, Editor, Helper};
use std::cell::RefCell;
use std::borrow::Cow;
use std::rc::Rc;
use std::io::Write;
use std::io::{self, IsTerminal};
use std::path::PathBuf;

type FushEditor = Editor<FushHelper, DefaultHistory>;

#[derive(Debug, Clone)]
pub struct FushHelper {
    snapshot: Rc<RefCell<RuntimeSnapshot>>,
    suggester: Suggester,
}

#[derive(Debug, Clone, Default)]
struct RuntimeSnapshot {
    functions: Vec<String>,
    path_dirs: Vec<PathBuf>,
    completions: std::collections::HashMap<String, Vec<CompletionRule>>,
}

impl RuntimeSnapshot {
    fn from_runtime(rt: &runtime::Runtime) -> Self {
        Self {
            functions: rt.functions.keys().cloned().collect(),
            path_dirs: std::env::var_os("PATH")
                .map(|v| std::env::split_paths(&v).collect())
                .unwrap_or_default(),
            completions: rt.completions.clone(),
        }
    }
}

impl FushHelper {
    fn new(suggester: Suggester) -> Self {
        Self {
            snapshot: Rc::new(RefCell::new(RuntimeSnapshot::default())),
            suggester,
        }
    }

    #[allow(dead_code)]
    fn is_builtin(&self, command: &str) -> bool {
        BUILTIN_NAMES.contains(&command)
    }

    fn refresh_snapshot(&self, rt: &runtime::Runtime) {
        *self.snapshot.borrow_mut() = RuntimeSnapshot::from_runtime(rt);
    }

    fn from_history(history: &DefaultHistory) -> Self {
        let mut suggester = Suggester::new();
        for entry in history.iter() {
            suggester.push(entry.as_str());
        }
        if suggester.is_empty() {
            return Self::new(suggester);
        }
        Self::new(suggester)
    }

    fn colors_disabled_for(no_color: Option<&std::ffi::OsStr>, stdout_is_tty: bool) -> bool {
        no_color.is_some() || !stdout_is_tty
    }

    #[allow(dead_code)]
    fn colors_disabled() -> bool {
        Self::colors_disabled_for(
            std::env::var_os("NO_COLOR").as_deref(),
            std::io::stdout().is_terminal(),
        )
    }

    fn hint_for(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
        enabled: bool,
        no_color: Option<&std::ffi::OsStr>,
        stdout_is_tty: bool,
    ) -> Option<String> {
        if !enabled
            || Self::colors_disabled_for(no_color, stdout_is_tty)
            || line.is_empty()
            || pos != line.len()
        {
            return None;
        }
        self.suggester.suggest(line)
    }

    fn highlight_for<'l>(
        &self,
        line: &'l str,
        _pos: usize,
        enabled: bool,
        no_color: Option<&std::ffi::OsStr>,
        stdout_is_tty: bool,
    ) -> Cow<'l, str> {
        if !enabled || Self::colors_disabled_for(no_color, stdout_is_tty) || line.is_empty() {
            return Cow::Borrowed(line);
        }
        let rendered = highlight::render_ansi(&highlight::highlight(line));
        if rendered == line {
            Cow::Borrowed(line)
        } else {
            Cow::Owned(rendered)
        }
    }
}

impl Helper for FushHelper {}

impl rustyline::completion::Completer for FushHelper {
    type Candidate = String;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &Context<'_>,
    ) -> rustyline::Result<(usize, Vec<Self::Candidate>)> {
        let snapshot = self.snapshot.borrow();
        let mut engine = CompletionEngine::new(
            BUILTIN_NAMES.iter().map(|name| name.to_string()).collect(),
            snapshot.functions.clone(),
            snapshot.path_dirs.clone(),
        );
        engine.completions = snapshot.completions.clone();
        drop(snapshot);
        let (start, candidates) = engine.complete_line(line, pos, &completion::FsDirSource);
        Ok((start, candidates))
    }
}

impl rustyline::validate::Validator for FushHelper {}

impl rustyline::hint::Hinter for FushHelper {
    type Hint = String;

    fn hint(&self, line: &str, pos: usize, ctx: &Context<'_>) -> Option<Self::Hint> {
        self.hint_for(
            line,
            pos,
            ctx,
            true,
            std::env::var_os("NO_COLOR").as_deref(),
            std::io::stdout().is_terminal(),
        )
    }
}

impl rustyline::highlight::Highlighter for FushHelper {
    fn highlight<'l>(&self, line: &'l str, _pos: usize) -> Cow<'l, str> {
        self.highlight_for(
            line,
            _pos,
            true,
            std::env::var_os("NO_COLOR").as_deref(),
            std::io::stdout().is_terminal(),
        )
    }
}

pub fn run_repl() -> i32 {
    if !std::io::stdin().is_terminal() {
        return run_non_tty_repl();
    }

    let mut editor = match FushEditor::with_helper(FushHelper::new(Suggester::new())) {
        Ok(editor) => editor,
        Err(_) => return run_non_tty_repl(),
    };
    if let Some(path) = history_path() {
        let _ = editor.load_history(&path);
    }
    {
        let history = editor.history().to_owned();
        let helper = FushHelper::from_history(history);
        editor.set_helper(Some(helper));
    }

    let mut rt = runtime::Runtime::new();
    load_universals(&mut rt);
    first_run_prompt();
    crate::config::source_primary_config(&mut rt);
    let mut buffer = String::new();
    loop {
        let prompt = if buffer.is_empty() {
            prompt_text()
        } else {
            "...> ".to_string()
        };
        match editor.readline(&prompt) {
            Ok(line) => {
                if line.trim().is_empty() && buffer.is_empty() {
                    continue;
                }
                let expanded = expand_abbr(&rt.abbrs, &line);
                if !buffer.is_empty() {
                    buffer.push('\n');
                }
                buffer.push_str(&expanded);
                if !expanded.trim().is_empty() {
                    if let Some(helper) = editor.helper_mut() {
                        helper.suggester.push(&expanded);
                    }
                    let _ = editor.add_history_entry(expanded.as_str());
                }
                if let Some(helper) = editor.helper_mut() {
                    helper.refresh_snapshot(&rt);
                }
                match parser::parse(&buffer) {
                    Ok(program) => {
                        let status = exec::run_program(&mut rt, &program.stmts);
                        if let Some(helper) = editor.helper_mut() {
                            helper.refresh_snapshot(&rt);
                        }
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

/// Ask once, on an interactive TTY only, whether to import an existing
/// fish config into config.fush. Errors are reported and the shell
/// continues without config.
fn first_run_prompt() {
    let fush = crate::config::primary_config_file();
    let fish = crate::config::fish_config_file();
    if !matches!(
        crate::config::first_run_state(fush.as_deref(), fish.as_deref()),
        crate::config::FirstRun::Import
    ) {
        return;
    }
    let (fush, fish) = (fush.unwrap(), fish.unwrap());
    eprint!(
        "Found a fish config at {}. Import it into config.fush? [Y/n] ",
        fish.display()
    );
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    let read = std::io::stdin().read_line(&mut line);
    let answer = match read {
        Ok(0) | Err(_) => String::from("y"),
        Ok(_) => line.trim().to_lowercase(),
    };
    let result = if answer.is_empty() || answer == "y" {
        crate::config::import_fish_config(&fish, &fush)
    } else {
        crate::config::create_empty_config(&fush)
    };
    if let Err(msg) = result {
        eprintln!("{msg}");
    }
}

fn run_non_tty_repl() -> i32 {
    let mut rt = runtime::Runtime::new();
    load_universals(&mut rt);
    crate::config::source_primary_config(&mut rt);
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

fn load_universals(rt: &mut runtime::Runtime) {
    if let Some(path) = crate::universal::universals_file() {
        crate::universal::load(rt, &path);
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
    let user = std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "user".to_string());
    let host = hostname();
    let cwd = short_cwd();
    format!("{user}@{host} {cwd}> ")
}

#[cfg(windows)]
fn hostname() -> String {
    hostname_fallback(std::env::var("COMPUTERNAME").ok().as_deref())
}

#[cfg(not(windows))]
fn hostname() -> String {
    hostname_fallback(
        std::fs::read_to_string("/proc/sys/kernel/hostname")
            .ok()
            .as_deref(),
    )
}

fn hostname_fallback(value: Option<&str>) -> String {
    match value {
        Some(value) if !value.trim().is_empty() => value.trim().to_ascii_lowercase(),
        _ => "localhost".to_string(),
    }
}

fn home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
    }

    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn short_cwd() -> String {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let home = home_dir();
    match home {
        Some(home) if !home.as_os_str().is_empty() => {
            let cwd_s = cwd.to_string_lossy();
            let home_s = home.to_string_lossy();
            if cwd_s == home_s {
                "~".to_string()
            } else if let Some(rest) = cwd_s.strip_prefix(home_s.as_ref()) {
                if rest.starts_with('/') || rest.starts_with('\\') {
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

fn history_dir() -> Option<PathBuf> {
    let home = home_dir().map(|p| p.to_string_lossy().into_owned());
    let userprofile = std::env::var("USERPROFILE").ok();
    history_dir_for(home.as_deref(), userprofile.as_deref(), cfg!(windows))
}

fn history_dir_for(
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
    if windows {
        dir.push(".fush");
    } else {
        dir.push(".local");
        dir.push("share");
        dir.push("fush");
    }
    Some(dir)
}

fn history_path() -> Option<PathBuf> {
    let dir = history_dir()?;
    if std::fs::create_dir_all(&dir).is_err() {
        return None;
    }
    Some(dir.join("history"))
}

fn expand_abbr(table: &std::collections::HashMap<String, String>, line: &str) -> String {
    let trimmed = line.trim_start();
    if trimmed.is_empty() {
        return line.to_string();
    }

    let first_word_start = line.len() - trimmed.len();
    let first_word_end = trimmed
        .find(|c: char| c.is_whitespace() || ";|&()<>".contains(c))
        .unwrap_or(trimmed.len());
    let word = &trimmed[..first_word_end];
    let Some(expansion) = table.get(word) else {
        return line.to_string();
    };

    let prefix = &line[..first_word_start];
    let suffix = &trimmed[first_word_end..];
    format!("{prefix}{expansion}{suffix}")
}

fn save_history_if_possible(editor: &mut FushEditor) {
    if let Some(path) = history_path() {
        let _ = editor.save_history(&path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustyline::Context;
    use rustyline::history::History;
    use std::ffi::OsStr;
    use std::path::Path;

    #[test]
    fn no_color_flag_disables_color_when_requested() {
        assert!(FushHelper::colors_disabled_for(Some(OsStr::new("1")), true));
        assert!(!FushHelper::colors_disabled_for(None, true));
        assert!(FushHelper::colors_disabled_for(None, false));
    }

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
    fn hostname_fallback_lowercases_computername() {
        assert_eq!(hostname_fallback(Some("COMPUTERNAME")), "computername");
        assert_eq!(hostname_fallback(Some("  HOST  ")), "host");
        assert_eq!(hostname_fallback(None), "localhost");
    }

    #[test]
    fn history_dir_uses_local_share_dir_on_unix() {
        let home = "/tmp/fush-history-home";
        let path = PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("fush");
        assert_eq!(path, history_dir_for(Some(home), None, false).unwrap());
    }

    #[test]
    fn history_dir_uses_userprofile_dot_fush_on_windows() {
        let home = r"C:\Users\alice";
        let path = PathBuf::from(home).join(".fush");
        assert_eq!(path, history_dir_for(None, Some(home), true).unwrap());
    }

    #[test]
    fn helper_tracks_known_builtins_and_history() {
        let mut suggester = Suggester::new();
        suggester.push("echo hello");
        let helper = FushHelper::new(suggester);

        assert!(helper.is_builtin("echo"));
        assert!(helper.is_builtin("true"));
        assert!(helper.is_builtin("false"));
        assert!(!helper.is_builtin("mystery"));
        assert!(BUILTIN_NAMES.contains(&"true"));
        assert!(BUILTIN_NAMES.contains(&"false"));
    }

    #[test]
    fn highlighter_returns_plain_input_when_color_is_disabled() {
        let helper = FushHelper::new(Suggester::new());
        let result = helper.highlight_for("echo hello", 11, false, None, true);
        assert_eq!(result, "echo hello");
    }

    #[test]
    fn hinter_returns_suffix_for_known_history_prefix() {
        let mut history = rustyline::history::DefaultHistory::new();
        history.add("echo hello").unwrap();
        let helper = FushHelper::from_history(&history);
        let ctx = Context::new(&history);

        assert_eq!(
            helper.hint_for("echo", 4, &ctx, true, None, true),
            Some(" hello".to_string())
        );
    }

    #[test]
    fn hinter_returns_none_for_unknown_prefix() {
        let mut history = rustyline::history::DefaultHistory::new();
        history.add("echo hello").unwrap();
        let helper = FushHelper::from_history(&history);
        let ctx = Context::new(&history);

        assert_eq!(helper.hint_for("ls", 2, &ctx, true, None, true), None);
    }

    #[test]
    fn expand_abbr_replaces_only_the_first_word() {
        let mut table = std::collections::HashMap::new();
        table.insert("gs".to_string(), "git status".to_string());
        table.insert("hi".to_string(), "hello".to_string());

        assert_eq!(expand_abbr(&table, "gs"), "git status");
        assert_eq!(expand_abbr(&table, "gs echo"), "git status echo");
        assert_eq!(expand_abbr(&table, "echo gs"), "echo gs");
        assert_eq!(expand_abbr(&table, "hi hi"), "hello hi");
        assert_eq!(expand_abbr(&table, "  gs  echo"), "  git status  echo");
        assert_eq!(expand_abbr(&table, ""), "");

        assert_eq!(expand_abbr(&table, "gs; echo hi"), "git status; echo hi");
        assert_eq!(expand_abbr(&table, "gs|wc"), "git status|wc");
        assert_eq!(expand_abbr(&table, "gs&&ls"), "git status&&ls");
        assert_eq!(expand_abbr(&table, "\tgs echo"), "\tgit status echo");
        assert_eq!(expand_abbr(&table, "gs;"), "git status;");
        assert_eq!(expand_abbr(&table, "(gs)"), "(gs)");
        assert_eq!(expand_abbr(&table, ">gs"), ">gs");
    }
}
