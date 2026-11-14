use crate::ast::{ParseError, SourcePos};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokKind {
    Word,
    Dollar,
    LParen,
    RParen,
    Pipe,
    Lt,
    Gt,
    FdNum,
    Semi,
    Newline,
    Eof,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: TokKind,
    pub text: String,
    pub parts: Vec<QuotedPart>,
    pub pos: SourcePos,
    pub index: Option<String>,
    /// true when any backslash escape was consumed while lexing this
    /// token, so `e\nd` is distinguishable from a bare `end` keyword
    pub escaped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteKind {
    Single,
    Double,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuotedPart {
    pub quote: Option<QuoteKind>,
    pub text: String,
    pub is_var: bool,
    pub index: Option<String>,
    /// true when this part came from a command substitution inside double
    /// quotes: text holds the raw source between the parentheses and the
    /// parser re-lexes it into statements
    pub is_subst: bool,
}

fn read_var_name_and_index(
    chars: &[char],
    i: &mut usize,
    line: &mut u32,
    col: &mut u32,
) -> Result<(String, Option<String>), ParseError> {
    let start = *i;
    while *i < chars.len() && (chars[*i].is_alphanumeric() || chars[*i] == '_') {
        if matches!(
            chars[*i],
            '\n' | '\r' | ' ' | '\t' | '|' | ';' | '(' | ')' | '<' | '>' | '[' | ']' | '$'
        ) {
            break;
        }
        *col += 1;
        *i += 1;
    }

    let name: String = chars[start..*i].iter().collect();
    if *i < chars.len() && chars[*i] == '[' {
        let pos = SourcePos {
            line: *line,
            col: *col,
        };
        *col += 1;
        *i += 1;
        let inner_start = *i;
        while *i < chars.len() {
            if chars[*i] == '[' {
                return Err(ParseError {
                    message: "nested index brackets are not allowed".into(),
                    pos,
                    incomplete: false,
                });
            }
            if chars[*i] == ']' {
                break;
            }
            if chars[*i] == '\n' {
                *line += 1;
                *col = 1;
            } else {
                *col += 1;
            }
            *i += 1;
        }
        if *i >= chars.len() || chars[*i] != ']' {
            return Err(ParseError {
                message: "unterminated index spec, expected ']'".into(),
                pos,
                incomplete: false,
            });
        }
        let raw: String = chars[inner_start..*i].iter().collect();
        if raw.trim().is_empty() {
            return Err(ParseError {
                message: "empty index spec".into(),
                pos,
                incomplete: false,
            });
        }
        *col += 1;
        *i += 1;
        if *i < chars.len() && chars[*i] == '[' {
            return Err(ParseError {
                message: "nested index brackets are not allowed".into(),
                pos,
                incomplete: false,
            });
        }
        return Ok((name, Some(raw)));
    }

    Ok((name, None))
}

pub fn lex(src: &str) -> Result<Vec<Token>, ParseError> {
    let mut toks = Vec::new();
    let chars: Vec<char> = src.chars().collect();
    let mut i = 0usize;
    let mut line = 1u32;
    let mut col = 1u32;

    macro_rules! bump {
        () => {{
            if chars[i] == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            i += 1;
        }};
    }

    while i < chars.len() {
        let c = chars[i];
        let pos = SourcePos { line, col };
        match c {
            ' ' | '\t' | '\r' => bump!(),
            '\n' => {
                toks.push(Token {
                    kind: TokKind::Newline,
                    text: "\n".into(),
                    parts: Vec::new(),
                    pos,
                    index: None,
                    escaped: false,
                });
                bump!();
            }
            '#' => {
                while i < chars.len() && chars[i] != '\n' {
                    bump!();
                }
            }
            '|' => {
                toks.push(Token {
                    kind: TokKind::Pipe,
                    text: "|".into(),
                    parts: Vec::new(),
                    pos,
                    index: None,
                    escaped: false,
                });
                bump!();
            }
            ';' => {
                toks.push(Token {
                    kind: TokKind::Semi,
                    text: ";".into(),
                    parts: Vec::new(),
                    pos,
                    index: None,
                    escaped: false,
                });
                bump!();
            }
            '(' => {
                toks.push(Token {
                    kind: TokKind::LParen,
                    text: "(".into(),
                    parts: Vec::new(),
                    pos,
                    index: None,
                    escaped: false,
                });
                bump!();
            }
            ')' => {
                toks.push(Token {
                    kind: TokKind::RParen,
                    text: ")".into(),
                    parts: Vec::new(),
                    pos,
                    index: None,
                    escaped: false,
                });
                bump!();
            }
            '<' => {
                toks.push(Token {
                    kind: TokKind::Lt,
                    text: "<".into(),
                    parts: Vec::new(),
                    pos,
                    index: None,
                    escaped: false,
                });
                bump!();
            }
            '>' => {
                toks.push(Token {
                    kind: TokKind::Gt,
                    text: ">".into(),
                    parts: Vec::new(),
                    pos,
                    index: None,
                    escaped: false,
                });
                bump!();
            }
            '0'..='9' if is_fd_redirect(&chars, i) => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    bump!();
                }
                let text: String = chars[start..i].iter().collect();
                toks.push(Token {
                    kind: TokKind::FdNum,
                    text,
                    parts: Vec::new(),
                    pos,
                    index: None,
                    escaped: false,
                });
            }
            '$' => {
                bump!();
                if i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    let (name, index) =
                        read_var_name_and_index(&chars, &mut i, &mut line, &mut col)?;
                    toks.push(Token {
                        kind: TokKind::Dollar,
                        text: name,
                        parts: Vec::new(),
                        pos,
                        index,
                        escaped: false,
                    });
                } else {
                    toks.push(Token {
                        kind: TokKind::Word,
                        text: "$".into(),
                        parts: vec![QuotedPart {
                            quote: None,
                            text: "$".into(),
                            is_var: false,
                            index: None,
                            is_subst: false,
                        }],
                        pos,
                        index: None,
                        escaped: false,
                    });
                }
            }
            '"' | '\'' => {
                let quote = if c == '"' {
                    QuoteKind::Double
                } else {
                    QuoteKind::Single
                };
                let mut parts: Vec<QuotedPart> = Vec::new();
                bump!();
                let mut buf = String::new();
                loop {
                    if i >= chars.len() {
                        return Err(ParseError {
                            message: format!(
                                "unterminated {} quote",
                                if quote == QuoteKind::Double {
                                    "double"
                                } else {
                                    "single"
                                }
                            ),
                            pos,
                            incomplete: false,
                        });
                    }
                    let ch = chars[i];
                    if ch
                        == (if quote == QuoteKind::Double {
                            '"'
                        } else {
                            '\''
                        })
                    {
                        bump!();
                        break;
                    }
                    if quote == QuoteKind::Double
                        && ch == '\\'
                        && i + 1 < chars.len()
                        && matches!(chars[i + 1], '"' | '\\' | '$' | '\n')
                    {
                        bump!();
                        if chars[i] == '\n' {
                            bump!();
                            continue;
                        }
                        buf.push(chars[i]);
                        bump!();
                        continue;
                    }
                    if quote == QuoteKind::Double && ch == '$' {
                        if !buf.is_empty() {
                            parts.push(QuotedPart {
                                quote: Some(quote),
                                text: std::mem::take(&mut buf),
                                is_var: false,
                                index: None,
                                is_subst: false,
                            });
                        }
                        bump!();
                        if i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                            let (name, index) =
                                read_var_name_and_index(&chars, &mut i, &mut line, &mut col)?;
                            parts.push(QuotedPart {
                                quote: Some(quote),
                                text: name,
                                is_var: true,
                                index,
                                is_subst: false,
                            });
                        } else {
                            buf.push('$');
                        }
                        continue;
                    }
                    if quote == QuoteKind::Double && ch == '(' {
                        if !buf.is_empty() {
                            parts.push(QuotedPart {
                                quote: Some(quote),
                                text: std::mem::take(&mut buf),
                                is_var: false,
                                index: None,
                                is_subst: false,
                            });
                        }
                        bump!();
                        let mut depth = 1usize;
                        let start = i;
                        while i < chars.len() && depth > 0 {
                            match chars[i] {
                                '(' => depth += 1,
                                ')' => depth -= 1,
                                _ => {}
                            }
                            if depth > 0 {
                                bump!();
                            }
                        }
                        if depth > 0 {
                            return Err(ParseError {
                                message: "unterminated command substitution inside double quotes, expected ')'".into(),
                                pos,
                                incomplete: false,
                            });
                        }
                        parts.push(QuotedPart {
                            quote: Some(quote),
                            text: chars[start..i].iter().collect(),
                            is_var: false,
                            index: None,
                            is_subst: true,
                        });
                        bump!();
                        continue;
                    }
                    buf.push(ch);
                    bump!();
                }
                if !buf.is_empty() || parts.is_empty() {
                    parts.push(QuotedPart {
                        quote: Some(quote),
                        text: buf,
                        is_var: false,
                        index: None,
                        is_subst: false,
                    });
                }
                toks.push(Token {
                    kind: TokKind::Word,
                    text: String::new(),
                    parts,
                    pos,
                    index: None,
                    escaped: false,
                });
            }
            _ => {
                let mut parts: Vec<QuotedPart> = Vec::new();
                let mut buf = String::new();
                let mut esc = false;
                loop {
                    if i >= chars.len()
                        || matches!(
                            chars[i],
                            ' ' | '\t' | '\r' | '\n' | '|' | ';' | '(' | ')' | '<' | '>'
                        )
                    {
                        break;
                    }
                    let ch = chars[i];
                    if ch == '"' || ch == '\'' {
                        if !buf.is_empty() {
                            parts.push(QuotedPart {
                                quote: None,
                                text: std::mem::take(&mut buf),
                                is_var: false,
                                index: None,
                                is_subst: false,
                            });
                        }
                        let quote = if ch == '"' {
                            QuoteKind::Double
                        } else {
                            QuoteKind::Single
                        };
                        let qpos = SourcePos { line, col };
                        bump!();
                        let mut qbuf = String::new();
                        let mut qparts: Vec<QuotedPart> = Vec::new();
                        loop {
                            if i >= chars.len() {
                                return Err(ParseError {
                                    message: format!(
                                        "unterminated {} quote",
                                        if quote == QuoteKind::Double {
                                            "double"
                                        } else {
                                            "single"
                                        }
                                    ),
                                    pos: qpos,
                                    incomplete: false,
                                });
                            }
                            let qch = chars[i];
                            if qch
                                == (if quote == QuoteKind::Double {
                                    '"'
                                } else {
                                    '\''
                                })
                            {
                                bump!();
                                break;
                            }
                            if quote == QuoteKind::Double
                                && qch == '\\'
                                && i + 1 < chars.len()
                                && matches!(chars[i + 1], '"' | '\\' | '$' | '\n')
                            {
                                bump!();
                                if chars[i] == '\n' {
                                    bump!();
                                    continue;
                                }
                                qbuf.push(chars[i]);
                                bump!();
                                continue;
                            }
                            if quote == QuoteKind::Double && qch == '$' {
                                if !qbuf.is_empty() {
                                    qparts.push(QuotedPart {
                                        quote: Some(quote),
                                        text: std::mem::take(&mut qbuf),
                                        is_var: false,
                                        index: None,
                                        is_subst: false,
                                    });
                                }
                                bump!();
                                if i < chars.len()
                                    && (chars[i].is_alphanumeric() || chars[i] == '_')
                                {
                                    let (name, index) = read_var_name_and_index(
                                        &chars, &mut i, &mut line, &mut col,
                                    )?;
                                    qparts.push(QuotedPart {
                                        quote: Some(quote),
                                        text: name,
                                        is_var: true,
                                        index,
                                        is_subst: false,
                                    });
                                } else {
                                    qbuf.push('$');
                                }
                                continue;
                            }
                            if quote == QuoteKind::Double && qch == '(' {
                                if !qbuf.is_empty() {
                                    qparts.push(QuotedPart {
                                        quote: Some(quote),
                                        text: std::mem::take(&mut qbuf),
                                        is_var: false,
                                        index: None,
                                        is_subst: false,
                                    });
                                }
                                bump!();
                                let mut depth = 1usize;
                                let start = i;
                                while i < chars.len() && depth > 0 {
                                    match chars[i] {
                                        '(' => depth += 1,
                                        ')' => depth -= 1,
                                        _ => {}
                                    }
                                    if depth > 0 {
                                        bump!();
                                    }
                                }
                                if depth > 0 {
                                    return Err(ParseError {
                                        message: "unterminated command substitution inside double quotes, expected ')'".into(),
                                        pos: qpos,
                                        incomplete: false,
                                    });
                                }
                                qparts.push(QuotedPart {
                                    quote: Some(quote),
                                    text: chars[start..i].iter().collect(),
                                    is_var: false,
                                    index: None,
                                    is_subst: true,
                                });
                                bump!();
                                continue;
                            }
                            qbuf.push(qch);
                            bump!();
                        }
                        if !qbuf.is_empty() || qparts.is_empty() {
                            qparts.push(QuotedPart {
                                quote: Some(quote),
                                text: qbuf,
                                is_var: false,
                                index: None,
                                is_subst: false,
                            });
                        }
                        parts.extend(qparts);
                        continue;
                    }
                    if ch == '\\' && i + 1 < chars.len() {
                        bump!();
                        buf.push(chars[i]);
                        bump!();
                        esc = true;
                        continue;
                    }
                    if ch == '$' {
                        bump!();
                        if i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                            if !buf.is_empty() {
                                parts.push(QuotedPart {
                                    quote: None,
                                    text: std::mem::take(&mut buf),
                                    is_var: false,
                                    index: None,
                                    is_subst: false,
                                });
                            }
                            let (name, index) =
                                read_var_name_and_index(&chars, &mut i, &mut line, &mut col)?;
                            if name.is_empty() {
                                buf.push('$');
                            } else {
                                parts.push(QuotedPart {
                                    quote: None,
                                    text: name,
                                    is_var: true,
                                    index,
                                    is_subst: false,
                                });
                            }
                        } else {
                            buf.push('$');
                        }
                        continue;
                    }
                    buf.push(ch);
                    bump!();
                }
                if !buf.is_empty() || parts.is_empty() {
                    parts.push(QuotedPart {
                        quote: None,
                        text: buf,
                        is_var: false,
                        index: None,
                        is_subst: false,
                    });
                }
                let text: String = parts.iter().map(|p| p.text.clone()).collect();
                toks.push(Token {
                    kind: TokKind::Word,
                    text,
                    parts,
                    pos,
                    index: None,
                    escaped: esc,
                });
            }
        }
    }
    toks.push(Token {
        kind: TokKind::Eof,
        text: String::new(),
        parts: Vec::new(),
        pos: SourcePos { line, col },
        index: None,
        escaped: false,
    });
    Ok(toks)
}

fn is_fd_redirect(chars: &[char], i: usize) -> bool {
    let mut j = i;
    while j < chars.len() && chars[j].is_ascii_digit() {
        j += 1;
    }
    j < chars.len() && matches!(chars[j], '<' | '>')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokKind> {
        lex(src).unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn simple_words() {
        assert_eq!(
            kinds("echo hi"),
            vec![TokKind::Word, TokKind::Word, TokKind::Eof]
        );
    }

    #[test]
    fn dollar_var() {
        let toks = lex("$x").unwrap();
        assert_eq!(toks[0].kind, TokKind::Dollar);
        assert_eq!(toks[0].text, "x");
    }

    #[test]
    fn indexed_var_is_single_var_token() {
        let toks = lex("$x[2]").unwrap();
        assert_eq!(toks[0].kind, TokKind::Dollar);
        assert_eq!(toks[0].text, "x");
        assert_eq!(toks[0].index.as_deref(), Some("2"));
    }

    #[test]
    fn bracket_text_stays_literal() {
        let toks = lex("echo [x]").unwrap();
        assert_eq!(toks[0].kind, TokKind::Word);
        assert_eq!(toks[0].text, "echo");
        assert_eq!(toks[1].kind, TokKind::Word);
        assert_eq!(toks[1].text, "[x]");
    }
}
