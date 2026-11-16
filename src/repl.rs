use crate::completion::{self, BUILTIN_NAMES, CompletionEngine, CompletionRule};
use crate::{exec, highlight, parser, runtime, suggest::Suggester};
use reedline::{
    ColumnarMenu, Completer, CompletionResult, CompletionStatus, FileBackedHistory, History,
    HistoryItem, Prompt, PromptEditMode, PromptHistorySearch, Reedline, ReedlineMenu, Signal, Span,
    StyledText, Suggestion,
};
use reedline::{Highlighter, Hinter, MenuBuilder};
use std::borrow::Cow;
use std::collections::HashMap;
use std::io::Write;
use std::io::{self, IsTerminal};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// History capacity shared with the previous rustyline-based editor.
const HISTORY_CAPACITY: usize = 1000;

type FushEditor = Reedline;

/// History wrapper that ignores reedline's automatic save of the raw
/// submitted line (which happens before abbreviation expansion), so the
/// REPL loop remains the only writer and stores the expanded line, exactly
/// like the rustyline implementation did.
struct HostHistory {
    inner: Box<dyn History>,
}

impl History for HostHistory {
    fn save(&mut self, h: HistoryItem) -> reedline::Result<HistoryItem> {
        // Suppress: the loop calls save() itself with the expanded line.
        Ok(h)
    }
    fn load(&self, id: reedline::HistoryItemId) -> reedline::Result<HistoryItem> {
        self.inner.load(id)
    }
    fn count(&self, query: reedline::SearchQuery) -> reedline::Result<i64> {
        self.inner.count(query)
    }
    fn search(&self, query: reedline::SearchQuery) -> reedline::Result<Vec<HistoryItem>> {
        self.inner.search(query)
    }
    fn update(
        &mut self,
        id: reedline::HistoryItemId,
        updater: &dyn Fn(HistoryItem) -> HistoryItem,
    ) -> reedline::Result<()> {
        self.inner.update(id, updater)
    }
    fn clear(&mut self) -> reedline::Result<()> {
        self.inner.clear()
    }
    fn delete(&mut self, id: reedline::HistoryItemId) -> reedline::Result<()> {
        self.inner.delete(id)
    }
    fn sync(&mut self) -> std::io::Result<()> {
        self.inner.sync()
    }
    fn session(&self) -> Option<reedline::HistorySessionId> {
        self.inner.session()
    }
}

#[derive(Debug, Clone, Default)]
struct RuntimeSnapshot {
    functions: Vec<String>,
    path_dirs: Vec<PathBuf>,
    completions: HashMap<String, Vec<CompletionRule>>,
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

/// Shared state for the reedline traits: the completion snapshot and the
/// history-backed autosuggestion engine, plus the color-disable flags
/// captured once at startup (they cannot change mid-session, matching the
/// previous rustyline behavior of reading them per callback).
#[derive(Debug, Clone)]
struct FushHelper {
    snapshot: Arc<Mutex<RuntimeSnapshot>>,
    suggester: Arc<Mutex<Suggester>>,
    /// None = colors enabled; Some(reason) = disabled (NO_COLOR or non-tty).
    colors_disabled: Option<ColorDisable>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorDisable {
    NoColor,
    NotTty,
}

impl FushHelper {
    fn with_color_state(suggester: Suggester, colors_disabled: Option<ColorDisable>) -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(RuntimeSnapshot::default())),
            suggester: Arc::new(Mutex::new(suggester)),
            colors_disabled,
        }
    }

    #[allow(dead_code)]
    fn is_builtin(&self, command: &str) -> bool {
        BUILTIN_NAMES.contains(&command)
    }

    fn refresh_snapshot(&self, rt: &runtime::Runtime) {
        *self.snapshot.lock().unwrap() = RuntimeSnapshot::from_runtime(rt);
    }

    fn detect_color_state() -> Option<ColorDisable> {
        Self::color_state_for(
            std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()),
            std::io::stdout().is_terminal(),
        )
    }

    fn color_state_for(no_color: bool, stdout_is_tty: bool) -> Option<ColorDisable> {
        if no_color {
            Some(ColorDisable::NoColor)
        } else if !stdout_is_tty {
            Some(ColorDisable::NotTty)
        } else {
            None
        }
    }

    /// The suggestion for the current line prefix, or None. Mirrors the
    /// rustyline-era guards: no suggestion when colors are disabled or the
    /// cursor is not at the end of the line.
    fn hint_for(&self, line: &str, pos: usize) -> Option<String> {
        if self.colors_disabled.is_some() || line.is_empty() || pos != line.len() {
            return None;
        }
        self.suggester.lock().unwrap().suggest(line)
    }
}

/// reedline Hinter: fish-like autosuggestions from history, gray-painted
/// when ANSI coloring is on (reedline asks for an ANSI-styled string).
struct FushHinter {
    helper: FushHelper,
    last_hint: String,
}

impl Hinter for FushHinter {
    fn handle(
        &mut self,
        line: &str,
        pos: usize,
        _history: &dyn History,
        use_ansi_coloring: bool,
        _cwd: &str,
    ) -> String {
        let hint = self.helper.hint_for(line, pos).unwrap_or_default();
        self.last_hint = hint.clone();
        if hint.is_empty() {
            String::new()
        } else if use_ansi_coloring {
            nu_ansi_term::Style::new()
                .fg(nu_ansi_term::Color::LightGray)
                .paint(&hint)
                .to_string()
        } else {
            hint
        }
    }

    fn complete_hint(&self) -> String {
        self.last_hint.clone()
    }

    fn next_hint_token(&self) -> String {
        self.last_hint
            .split_whitespace()
            .next()
            .map(str::to_string)
            .unwrap_or_default()
    }
}

/// reedline Highlighter: ANSI syntax highlighting via the highlight module,
/// disabled under NO_COLOR or when stdout is not a terminal.
struct FushHighlighter {
    helper: FushHelper,
}

impl Highlighter for FushHighlighter {
    fn highlight(&self, line: &str, _cursor: usize) -> StyledText {
        if self.helper.colors_disabled.is_some() || line.is_empty() {
            return StyledText::new();
        }
        let mut styled = StyledText::new();
        for (text, style) in highlight::highlight(line) {
            styled.push((nu_style_for(style), text));
        }
        styled
    }
}

fn nu_style_for(style: highlight::Style) -> nu_ansi_term::Style {
    use highlight::Style as S;
    use nu_ansi_term::Style as N;
    match style {
        S::Default => N::default(),
        S::Bold => N::new().bold(),
        S::Red => N::new().fg(nu_ansi_term::Color::Red),
        S::Cyan => N::new().fg(nu_ansi_term::Color::Cyan),
        S::Yellow => N::new().fg(nu_ansi_term::Color::Yellow),
        S::Blue => N::new().fg(nu_ansi_term::Color::Blue),
        S::Magenta => N::new().fg(nu_ansi_term::Color::Magenta),
        S::Gray => N::new().fg(nu_ansi_term::Color::DarkGray),
    }
}

/// reedline Completer: TAB completion via the fush CompletionEngine over
/// the runtime snapshot (commands, functions, PATH executables, files and
/// per-command rules).
struct FushCompleter {
    helper: FushHelper,
}

impl Completer for FushCompleter {
    fn complete(&mut self, line: &str, pos: usize) -> CompletionResult {
        let snapshot = self.helper.snapshot.lock().unwrap();
        let mut engine = CompletionEngine::new(
            BUILTIN_NAMES.iter().map(|name| name.to_string()).collect(),
            snapshot.functions.clone(),
            snapshot.path_dirs.clone(),
        );
        engine.completions = snapshot.completions.clone();
        drop(snapshot);
        let (start, candidates) = engine.complete_line(line, pos, &completion::FsDirSource);
        let suggestions: Vec<Suggestion> = candidates
            .into_iter()
            .map(|value| Suggestion {
                value: value.clone(),
                display_override: None,
                description: None,
                style: None,
                extra: None,
                span: Span::new(start, start + value.len()),
                append_whitespace: false,
                match_indices: None,
            })
            .collect();
        CompletionResult::fresh(suggestions)
    }

    fn poll_completion(&mut self) -> CompletionStatus {
        CompletionStatus::Idle
    }
}

/// The fush prompt: the user's `fish_prompt` function output on the left
/// (ANSI escapes allowed), the user's `fish_right_prompt` output on the
/// right, and `"...> "` while a multi-line buffer is being accumulated.
struct FushPrompt {
    /// Rendered left prompt for the current loop iteration; the prompt
    /// object is rebuilt (cheaply) each iteration because it needs
    /// `&mut Runtime`, which the Prompt trait cannot pass through.
    left: String,
    right: String,
    /// True while a multi-line buffer is pending: shows the continuation
    /// indicator used by the previous implementation.
    continuation: bool,
}

impl Prompt for FushPrompt {
    fn render_prompt_left(&self) -> Cow<'_, str> {
        if self.continuation {
            Cow::Borrowed("...> ")
        } else {
            Cow::Borrowed(&self.left)
        }
    }

    fn render_prompt_right(&self) -> Cow<'_, str> {
        Cow::Borrowed(&self.right)
    }

    fn render_prompt_indicator(&self, _edit_mode: PromptEditMode) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_multiline_indicator(&self) -> Cow<'_, str> {
        Cow::Borrowed("")
    }

    fn render_prompt_history_search_indicator(
        &self,
        _history_search: PromptHistorySearch,
    ) -> Cow<'_, str> {
        Cow::Borrowed("(reverse-i-search): ")
    }
}

impl FushPrompt {
    fn new(rt: &mut runtime::Runtime, continuation: bool) -> Self {
        let left = if continuation {
            String::new()
        } else {
            render_prompt(rt)
        };
        let right = if continuation {
            String::new()
        } else {
            render_right_prompt(rt)
        };
        Self {
            left,
            right,
            continuation,
        }
    }
}

/// Build the right prompt: run the user's `fish_right_prompt` function if
/// one is defined (capturing its output), stripping the trailing newline.
/// Empty string when the function is not defined.
fn render_right_prompt(rt: &mut runtime::Runtime) -> String {
    let func = rt.functions.get("fish_right_prompt").map(Rc::clone);
    let Some(func) = func else {
        return String::new();
    };
    let saved = rt.capture.take();
    rt.capture = Some(Vec::new());
    let _ = exec::call_function(rt, "fish_right_prompt", &func, &[]);
    let buf = rt.capture.take().unwrap_or_default();
    rt.capture = saved;
    String::from_utf8_lossy(&buf)
        .trim_end_matches('\n')
        .to_string()
}

pub fn run_repl(profile: Option<Option<String>>) -> i32 {
    if !std::io::stdin().is_terminal() {
        return run_non_tty_repl(profile);
    }

    let color_state = FushHelper::detect_color_state();
    let suggester = Suggester::new();
    let history_box: Option<Box<dyn History>> = history_path().and_then(|path| {
        FileBackedHistory::with_file(HISTORY_CAPACITY, path)
            .ok()
            .map(|h| Box::new(h) as Box<dyn History>)
    });

    // Seed the autosuggestion engine from the persisted history file.
    let helper = match &history_box {
        Some(history) => {
            let mut suggester = suggester;
            if let Ok(entries) = history.search(reedline::SearchQuery::everything(
                reedline::SearchDirection::Forward,
                None,
            )) {
                for entry in entries {
                    suggester.push(entry.command_line.as_str());
                }
            }
            FushHelper::with_color_state(suggester, color_state)
        }
        None => FushHelper::with_color_state(suggester, color_state),
    };

    let mut editor: FushEditor = Reedline::create()
        .with_ansi_colors(color_state.is_none())
        .with_hinter(Box::new(FushHinter {
            helper: helper.clone(),
            last_hint: String::new(),
        }))
        .with_highlighter(Box::new(FushHighlighter {
            helper: helper.clone(),
        }))
        .with_completer(Box::new(FushCompleter {
            helper: helper.clone(),
        }))
        .with_menu(ReedlineMenu::EngineCompleter(Box::new(
            ColumnarMenu::default().with_name("completion_menu"),
        )));
    if let Some(history) = history_box {
        editor = editor.with_history(Box::new(HostHistory { inner: history }));
    }
    // TAB opens the completion menu (first press inserts the common
    // prefix / single match, further presses cycle), matching the previous
    // TAB behavior as closely as reedline allows.
    let mut keybindings = reedline::default_emacs_keybindings();
    keybindings.add_binding(
        reedline::KeyModifiers::NONE,
        reedline::KeyCode::Tab,
        reedline::ReedlineEvent::UntilFound(vec![
            reedline::ReedlineEvent::Menu("completion_menu".to_string()),
            reedline::ReedlineEvent::MenuNext,
        ]),
    );
    editor = editor.with_edit_mode(Box::new(reedline::Emacs::new(keybindings)));

    let mut rt = runtime::Runtime::new();
    rt.interactive = true;
    if let Err(e) = match &profile {
        Some(name) => crate::profile::load_profile(&mut rt, name.as_deref()),
        None => Ok(()),
    } {
        eprintln!("fush: profile: {e}");
        std::process::exit(2);
    }
    load_universals(&mut rt);
    first_run_prompt();
    crate::config::source_primary_config(&mut rt);
    helper.refresh_snapshot(&rt);
    let mut buffer = String::new();
    loop {
        let prompt = FushPrompt::new(&mut rt, !buffer.is_empty());
        let signal = match editor.read_line(&prompt) {
            Ok(signal) => signal,
            Err(_) => {
                sync_history_if_possible(&mut editor);
                return 0;
            }
        };
        match signal {
            Signal::Success(line) => {
                if line.trim().is_empty() && buffer.is_empty() {
                    continue;
                }
                let expanded = expand_abbr(&rt.abbrs, &line);
                if !buffer.is_empty() {
                    buffer.push('\n');
                }
                buffer.push_str(&expanded);
                if !expanded.trim().is_empty() {
                    push_history(&mut editor, &expanded);
                    helper.suggester.lock().unwrap().push(&expanded);
                }
                match parser::parse(&buffer) {
                    Ok(program) => {
                        let status = exec::run_program(&mut rt, &program.stmts);
                        helper.refresh_snapshot(&rt);
                        buffer.clear();
                        if rt.exited.is_some() {
                            sync_history_if_possible(&mut editor);
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
            Signal::CtrlC(_) => {
                buffer.clear();
                continue;
            }
            Signal::CtrlD => {
                sync_history_if_possible(&mut editor);
                if let Err(err) = parser::parse(&buffer)
                    && err.incomplete
                {
                    eprintln!("{err}");
                    return 1;
                }
                return 0;
            }
            Signal::HostCommand(_) | Signal::ExternalBreak(_) | _ => {
                sync_history_if_possible(&mut editor);
                return 0;
            }
        }
    }
}

fn push_history(editor: &mut FushEditor, line: &str) {
    let _ = editor
        .history_mut()
        .save(HistoryItem::from_command_line(line));
}

fn sync_history_if_possible(editor: &mut FushEditor) {
    let _ = editor.sync_history();
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

fn run_non_tty_repl(profile: Option<Option<String>>) -> i32 {
    let mut rt = runtime::Runtime::new();
    if let Err(e) = match &profile {
        Some(name) => crate::profile::load_profile(&mut rt, name.as_deref()),
        None => Ok(()),
    } {
        eprintln!("fush: profile: {e}");
        std::process::exit(2);
    }
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

/// Build the interactive prompt: run the user's `fish_prompt` function
/// if one is defined (capturing its output), otherwise the built-in
/// `user@host cwd> ` text. A trailing newline on the captured output is
/// stripped.
fn render_prompt(rt: &mut runtime::Runtime) -> String {
    let func = rt.functions.get("fish_prompt").map(Rc::clone);
    match func {
        Some(func) => {
            let saved = rt.capture.take();
            rt.capture = Some(Vec::new());
            let _ = exec::call_function(rt, "fish_prompt", &func, &[]);
            let buf = rt.capture.take().unwrap_or_default();
            rt.capture = saved;
            let text = String::from_utf8_lossy(&buf)
                .trim_end_matches('\n')
                .to_string();
            if text.is_empty() { prompt_text() } else { text }
        }
        None => prompt_text(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn no_color_flag_disables_color_when_requested() {
        assert_eq!(
            FushHelper::color_state_for(true, true),
            Some(ColorDisable::NoColor)
        );
        assert_eq!(FushHelper::color_state_for(false, true), None);
        assert_eq!(
            FushHelper::color_state_for(false, false),
            Some(ColorDisable::NotTty)
        );
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
    fn render_prompt_uses_fish_prompt_output_when_defined() {
        let mut rt = runtime::Runtime::new();
        let program = parser::parse(
            "function fish_prompt; echo -n hello; set_color brblue; echo -n '> '; end",
        )
        .unwrap();
        exec::run_program(&mut rt, &program.stmts);
        assert!(rt.functions.contains_key("fish_prompt"));
        let prompt = render_prompt(&mut rt);
        assert_eq!(prompt, "hello\x1b[94m> ");
    }

    #[test]
    fn render_prompt_falls_back_when_fish_prompt_undefined() {
        let mut rt = runtime::Runtime::new();
        let prompt = render_prompt(&mut rt);
        assert!(prompt.ends_with("> "));
    }

    #[test]
    fn render_right_prompt_empty_when_undefined() {
        let mut rt = runtime::Runtime::new();
        assert_eq!(render_right_prompt(&mut rt), "");
    }

    #[test]
    fn render_right_prompt_uses_function_output() {
        let mut rt = runtime::Runtime::new();
        let program = parser::parse("function fish_right_prompt; echo -n right; end").unwrap();
        exec::run_program(&mut rt, &program.stmts);
        assert_eq!(render_right_prompt(&mut rt), "right");
    }

    #[test]
    fn fush_prompt_shows_continuation_for_pending_buffer() {
        let mut rt = runtime::Runtime::new();
        let prompt = FushPrompt::new(&mut rt, true);
        assert_eq!(prompt.render_prompt_left(), "...> ");
        assert_eq!(prompt.render_prompt_right(), "");
        let prompt = FushPrompt::new(&mut rt, false);
        assert!(prompt.render_prompt_left().ends_with("> "));
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
        let helper = FushHelper::with_color_state(suggester, None);

        assert!(helper.is_builtin("echo"));
        assert!(helper.is_builtin("true"));
        assert!(helper.is_builtin("false"));
        assert!(!helper.is_builtin("mystery"));
        assert!(BUILTIN_NAMES.contains(&"true"));
        assert!(BUILTIN_NAMES.contains(&"false"));
    }

    #[test]
    fn highlighter_returns_plain_input_when_color_is_disabled() {
        let helper = FushHelper::with_color_state(Suggester::new(), Some(ColorDisable::NotTty));
        let result = helper.hint_for("echo hello", 11);
        assert_eq!(result, None);
    }

    #[test]
    fn hinter_returns_suffix_for_known_history_prefix() {
        let mut suggester = Suggester::new();
        suggester.push("echo hello");
        let helper = FushHelper::with_color_state(suggester, None);

        assert_eq!(helper.hint_for("echo", 4), Some(" hello".to_string()));
    }

    #[test]
    fn hinter_returns_none_for_unknown_prefix() {
        let mut suggester = Suggester::new();
        suggester.push("echo hello");
        let helper = FushHelper::with_color_state(suggester, None);

        assert_eq!(helper.hint_for("ls", 2), None);
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
