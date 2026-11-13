#![allow(dead_code)]

use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

static EXTERNAL_CACHE: Mutex<Option<HashMap<String, bool>>> = Mutex::new(None);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Style {
    Default,
    Bold,
    Red,
    Cyan,
    Yellow,
    Blue,
    Magenta,
    Gray,
}

#[allow(dead_code)]
pub fn highlight(src: &str) -> Vec<(String, Style)> {
    if crate::lexer::lex(src).is_err() {
        return vec![(src.to_string(), Style::Default)];
    }

    let mut spans = Vec::new();
    let mut i = 0usize;
    let mut command_pos = true;
    let bytes = src.as_bytes();

    while i < bytes.len() {
        let ch = src[i..].chars().next().unwrap();

        if ch.is_whitespace() {
            let start = i;
            while i < bytes.len() && src[i..].chars().next().is_some_and(char::is_whitespace) {
                i += src[i..].chars().next().unwrap().len_utf8();
            }
            spans.push((src[start..i].to_string(), Style::Default));
            continue;
        }

        if ch == '#' {
            let start = i;
            while i < bytes.len() {
                let next = src[i..].chars().next().unwrap();
                if next == '\n' {
                    break;
                }
                i += next.len_utf8();
            }
            spans.push((src[start..i].to_string(), Style::Gray));
            continue;
        }

        if matches!(ch, '|' | ';') {
            spans.push((ch.to_string(), Style::Blue));
            i += ch.len_utf8();
            command_pos = true;
            continue;
        }

        if matches!(ch, '(' | ')' | '<' | '>') {
            spans.push((ch.to_string(), Style::Default));
            i += ch.len_utf8();
            command_pos = false;
            continue;
        }

        if ch == '$' {
            let start = i;
            i += ch.len_utf8();
            while i < bytes.len() {
                let next = src[i..].chars().next().unwrap();
                if next.is_ascii_alphanumeric() || next == '_' {
                    i += next.len_utf8();
                    continue;
                }
                break;
            }
            let text = &src[start..i];
            spans.push((text.to_string(), Style::Cyan));
            command_pos = false;
            continue;
        }

        let start = i;
        while i < bytes.len() {
            let next = src[i..].chars().next().unwrap();
            if next.is_whitespace() || matches!(next, '|' | ';' | '(' | ')' | '<' | '>') || next == '#' {
                break;
            }
            if next == '"' || next == '\'' {
                let quote = next;
                i += next.len_utf8();
                while i < bytes.len() {
                    let inner = src[i..].chars().next().unwrap();
                    if inner == '\\' && i + 1 < bytes.len() {
                        i += 1;
                        let escaped = src[i..].chars().next().unwrap();
                        i += escaped.len_utf8();
                        continue;
                    }
                    if inner == quote {
                        i += inner.len_utf8();
                        break;
                    }
                    i += inner.len_utf8();
                }
                continue;
            }
            i += next.len_utf8();
        }

        let text = &src[start..i];
        let style = classify_token(text, command_pos);
        spans.push((text.to_string(), style));
        command_pos = next_command_pos(text, style);
    }

    spans
}

#[allow(dead_code)]
pub fn render_ansi(spans: &[(String, Style)]) -> String {
    let mut out = String::new();
    let mut active = None;

    for (text, style) in spans {
        if active != Some(*style) {
            if active.is_some_and(|s| s != Style::Default) {
                out.push_str("\x1b[0m");
            }
            if *style != Style::Default {
                out.push_str(ansi_for_style(*style));
            }
            active = Some(*style);
        }
        out.push_str(text);
    }

    if active.is_some_and(|style| style != Style::Default) {
        out.push_str("\x1b[0m");
    }

    out
}

pub fn is_external(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return false;
    }

    if let Ok(guard) = EXTERNAL_CACHE.lock()
        && let Some(cache) = guard.as_ref()
        && let Some(hit) = cache.get(trimmed)
    {
        return *hit;
    }

    let found = scan_path(trimmed);

    if let Ok(mut guard) = EXTERNAL_CACHE.lock() {
        guard
            .get_or_insert_with(HashMap::new)
            .insert(trimmed.to_string(), found);
    }

    found
}

fn scan_path(name: &str) -> bool {
    let candidates = if cfg!(windows) {
        vec![name.to_string(), format!("{name}.exe")]
    } else {
        vec![name.to_string()]
    };

    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };

    for dir in std::env::split_paths(&path_var) {
        for candidate in &candidates {
            let path = dir.join(candidate);
            if let Ok(meta) = std::fs::metadata(&path)
                && meta.is_file()
                && is_executable(&path)
            {
                return true;
            }
        }
    }

    false
}

pub fn reset_external_cache_for_tests() {
    if let Ok(mut guard) = EXTERNAL_CACHE.lock() {
        *guard = None;
    }
}

fn classify_token(raw: &str, command_pos: bool) -> Style {
    let bare = strip_outer_quotes(raw);

    if raw.starts_with('$') {
        return Style::Cyan;
    }

    if is_option(raw) {
        return Style::Magenta;
    }

    if raw.starts_with('"') || raw.starts_with('\'') {
        return Style::Yellow;
    }

    if is_keyword(bare) {
        return Style::Blue;
    }

    if command_pos {
        if matches!(bare, "set" | "echo" | "cd" | "exit" | "test") || is_external(bare) {
            return Style::Bold;
        }
        return Style::Red;
    }

    Style::Default
}

fn is_option(raw: &str) -> bool {
    if !raw.starts_with('-') || raw.len() == 1 {
        return false;
    }
    let body = &raw[1..];
    !body.chars().all(|c| c.is_ascii_digit())
}

fn next_command_pos(raw: &str, _style: Style) -> bool {
    is_keyword(strip_outer_quotes(raw))
}

fn is_keyword(value: &str) -> bool {
    matches!(value, "and" | "or" | "not" | "if" | "else" | "for" | "while" | "switch" | "end" | "case")
}

fn strip_outer_quotes(raw: &str) -> &str {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 {
        let mut chars = trimmed.chars();
        let first = chars.next().unwrap();
        let last = chars.next_back().unwrap();
        if (first == '"' || first == '\'') && first == last {
            return &trimmed[1..trimmed.len() - 1];
        }
    }
    trimmed
}

fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::metadata(path)
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }
    #[cfg(not(unix))]
    {
        std::fs::metadata(path)
            .map(|meta| meta.is_file())
            .unwrap_or(false)
    }
}

fn ansi_for_style(style: Style) -> &'static str {
    match style {
        Style::Default => "\x1b[0m",
        Style::Bold => "\x1b[1m",
        Style::Red => "\x1b[31m",
        Style::Cyan => "\x1b[36m",
        Style::Yellow => "\x1b[33m",
        Style::Blue => "\x1b[34m",
        Style::Magenta => "\x1b[35m",
        Style::Gray => "\x1b[90m",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_bold() {
        let spans = highlight("echo");
        assert_eq!(spans, vec![("echo".to_string(), Style::Bold)]);
    }

    #[test]
    fn external_bold() {
        let external = if cfg!(windows) { "cmd.exe" } else { "sh" };
        let spans = highlight(external);
        assert_eq!(spans, vec![(external.to_string(), Style::Bold)]);
    }

    #[test]
    fn invalid_red() {
        let spans = highlight("definitely_not_a_cmd");
        assert_eq!(spans, vec![("definitely_not_a_cmd".to_string(), Style::Red)]);
    }

    #[test]
    fn var_cyan() {
        let spans = highlight("$HOME");
        assert_eq!(spans, vec![("$HOME".to_string(), Style::Cyan)]);
    }

    #[test]
    fn string_yellow() {
        let spans = highlight("\"hi\"");
        assert_eq!(spans, vec![("\"hi\"".to_string(), Style::Yellow)]);
    }

    #[test]
    fn keyword_blue() {
        let spans = highlight("if");
        assert_eq!(spans, vec![("if".to_string(), Style::Blue)]);
    }

    #[test]
    fn option_magenta() {
        let spans = highlight("-n");
        assert_eq!(spans, vec![("-n".to_string(), Style::Magenta)]);
    }

    #[test]
    fn negative_number_is_not_option() {
        let spans = highlight("echo -42");
        assert_eq!(spans[2], ("-42".to_string(), Style::Default));
    }

    #[test]
    fn lone_dash_is_not_option() {
        let spans = highlight("echo -");
        assert_eq!(spans[1], (" ".to_string(), Style::Default));
        assert_eq!(spans[2], ("-".to_string(), Style::Default));
    }

    #[test]
    fn external_lookup_is_cached() {
        reset_external_cache_for_tests();
        assert!(is_external("ls"));
        assert!(is_external("ls"));
        assert_eq!(cached("ls"), Some(true));
        reset_external_cache_for_tests();
    }

    #[test]
    fn unknown_external_is_cached_negative() {
        reset_external_cache_for_tests();
        assert!(!is_external("definitely_not_a_cmd_xyzzy"));
        assert_eq!(cached("definitely_not_a_cmd_xyzzy"), Some(false));
        reset_external_cache_for_tests();
    }

    fn cached(name: &str) -> Option<bool> {
        let Ok(guard) = EXTERNAL_CACHE.lock() else { panic!("cache poisoned") };
        guard.as_ref().and_then(|cache| cache.get(name).copied())
    }

    #[test]
    fn comment_gray() {
        let spans = highlight("echo # note");
        assert_eq!(spans[0], ("echo".to_string(), Style::Bold));
        assert_eq!(spans[1], (" ".to_string(), Style::Default));
        assert_eq!(spans[2], ("# note".to_string(), Style::Gray));
    }

    #[test]
    fn lex_error_falls_back_plain() {
        let spans = highlight("\"unterminated");
        assert_eq!(spans, vec![("\"unterminated".to_string(), Style::Default)]);
    }

    #[test]
    fn multi_token_line_mixes_correctly() {
        let spans = highlight("echo -n hi > file");
        let styles: Vec<_> = spans.iter().map(|(_, style)| *style).collect();
        assert_eq!(styles[0], Style::Bold);
        assert_eq!(styles[1], Style::Default);
        assert_eq!(styles[2], Style::Magenta);
        assert_eq!(styles[3], Style::Default);
        assert_eq!(styles[4], Style::Default);
        assert_eq!(styles[5], Style::Default);
    }
}
