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
                toks.push(Token { kind: TokKind::Newline, text: "\n".into(), parts: Vec::new(), pos, escaped: false });
                bump!();
            }
            '#' => {
                while i < chars.len() && chars[i] != '\n' {
                    bump!();
                }
            }
            '|' => {
                toks.push(Token { kind: TokKind::Pipe, text: "|".into(), parts: Vec::new(), pos, escaped: false });
                bump!();
            }
            ';' => {
                toks.push(Token { kind: TokKind::Semi, text: ";".into(), parts: Vec::new(), pos, escaped: false });
                bump!();
            }
            '(' => {
                toks.push(Token { kind: TokKind::LParen, text: "(".into(), parts: Vec::new(), pos, escaped: false });
                bump!();
            }
            ')' => {
                toks.push(Token { kind: TokKind::RParen, text: ")".into(), parts: Vec::new(), pos, escaped: false });
                bump!();
            }
            '<' => {
                toks.push(Token { kind: TokKind::Lt, text: "<".into(), parts: Vec::new(), pos, escaped: false });
                bump!();
            }
            '>' => {
                toks.push(Token { kind: TokKind::Gt, text: ">".into(), parts: Vec::new(), pos, escaped: false });
                bump!();
            }
            '0'..='9' if is_fd_redirect(&chars, i) => {
                let start = i;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    bump!();
                }
                let text: String = chars[start..i].iter().collect();
                toks.push(Token { kind: TokKind::FdNum, text, parts: Vec::new(), pos, escaped: false });
            }
            '$' => {
                bump!();
                if i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                    let start = i;
                    while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                        bump!();
                    }
                    let text: String = chars[start..i].iter().collect();
                    toks.push(Token { kind: TokKind::Dollar, text, parts: Vec::new(), pos, escaped: false });
                } else {
                    toks.push(Token {
                        kind: TokKind::Word,
                        text: "$".into(),
                        parts: vec![QuotedPart { quote: None, text: "$".into(), is_var: false }],
                        pos,
                        escaped: false,
                    });
                }
            }
            '"' | '\'' => {
                let quote = if c == '"' { QuoteKind::Double } else { QuoteKind::Single };
                let mut parts: Vec<QuotedPart> = Vec::new();
                bump!();
                let mut buf = String::new();
                loop {
                    if i >= chars.len() {
                        return Err(ParseError {
                            message: format!("unterminated {} quote", if quote == QuoteKind::Double { "double" } else { "single" }),
                            pos,
                        });
                    }
                    let ch = chars[i];
                    if ch == (if quote == QuoteKind::Double { '"' } else { '\'' }) {
                        bump!();
                        break;
                    }
                    if quote == QuoteKind::Double && ch == '\\' && i + 1 < chars.len()
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
                            parts.push(QuotedPart { quote: Some(quote), text: std::mem::take(&mut buf), is_var: false });
                        }
                        bump!();
                        let vstart = i;
                        while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                            bump!();
                        }
                        if i == vstart {
                            buf.push('$');
                        } else {
                            parts.push(QuotedPart {
                                quote: Some(quote),
                                text: chars[vstart..i].iter().collect(),
                                is_var: true,
                            });
                        }
                        continue;
                    }
                    buf.push(ch);
                    bump!();
                }
                if !buf.is_empty() || parts.is_empty() {
                    parts.push(QuotedPart { quote: Some(quote), text: buf, is_var: false });
                }
                toks.push(Token { kind: TokKind::Word, text: String::new(), parts, pos, escaped: false });
            }
            _ => {
                let mut parts: Vec<QuotedPart> = Vec::new();
                let mut buf = String::new();
                let mut esc = false;
                loop {
                    if i >= chars.len() || matches!(chars[i], ' ' | '\t' | '\r' | '\n' | '|' | ';' | '(' | ')' | '<' | '>')
                    {
                        break;
                    }
                    let ch = chars[i];
                    if ch == '"' || ch == '\'' {
                        if !buf.is_empty() {
                            parts.push(QuotedPart { quote: None, text: std::mem::take(&mut buf), is_var: false });
                        }
                        let quote = if ch == '"' { QuoteKind::Double } else { QuoteKind::Single };
                        let qpos = SourcePos { line, col };
                        bump!();
                        let mut qbuf = String::new();
                        let mut qparts: Vec<QuotedPart> = Vec::new();
                        loop {
                            if i >= chars.len() {
                                return Err(ParseError {
                                    message: format!("unterminated {} quote", if quote == QuoteKind::Double { "double" } else { "single" }),
                                    pos: qpos,
                                });
                            }
                            let qch = chars[i];
                            if qch == (if quote == QuoteKind::Double { '"' } else { '\'' }) {
                                bump!();
                                break;
                            }
                            if quote == QuoteKind::Double && qch == '\\' && i + 1 < chars.len()
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
                                    qparts.push(QuotedPart { quote: Some(quote), text: std::mem::take(&mut qbuf), is_var: false });
                                }
                                bump!();
                                let vstart = i;
                                while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                                    bump!();
                                }
                                if i == vstart {
                                    qbuf.push('$');
                                } else {
                                    qparts.push(QuotedPart {
                                        quote: Some(quote),
                                        text: chars[vstart..i].iter().collect(),
                                        is_var: true,
                                    });
                                }
                                continue;
                            }
                            qbuf.push(qch);
                            bump!();
                        }
                        if !qbuf.is_empty() || qparts.is_empty() {
                            qparts.push(QuotedPart { quote: Some(quote), text: qbuf, is_var: false });
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
                                parts.push(QuotedPart { quote: None, text: std::mem::take(&mut buf), is_var: false });
                            }
                            let vstart = i;
                            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                                bump!();
                            }
                            parts.push(QuotedPart {
                                quote: None,
                                text: chars[vstart..i].iter().collect(),
                                is_var: true,
                            });
                        } else {
                            buf.push('$');
                        }
                        continue;
                    }
                    buf.push(ch);
                    bump!();
                }
                if !buf.is_empty() || parts.is_empty() {
                    parts.push(QuotedPart { quote: None, text: buf, is_var: false });
                }
                let text: String = parts.iter().map(|p| p.text.clone()).collect();
                toks.push(Token { kind: TokKind::Word, text, parts, pos, escaped: esc });
            }
        }
    }
    toks.push(Token {
        kind: TokKind::Eof,
        text: String::new(),
        parts: Vec::new(),
        pos: SourcePos { line, col },
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
        assert_eq!(kinds("echo hi"), vec![TokKind::Word, TokKind::Word, TokKind::Eof]);
    }

    #[test]
    fn operators() {
        assert_eq!(kinds("a|b;c<d>e"), vec![
            TokKind::Word, TokKind::Pipe, TokKind::Word, TokKind::Semi,
            TokKind::Word, TokKind::Lt, TokKind::Word, TokKind::Gt, TokKind::Word, TokKind::Eof
        ]);
    }

    #[test]
    fn parens() {
        assert_eq!(kinds("(ls)"), vec![TokKind::LParen, TokKind::Word, TokKind::RParen, TokKind::Eof]);
    }

    #[test]
    fn dollar_var() {
        let toks = lex("$x").unwrap();
        assert_eq!(toks[0].kind, TokKind::Dollar);
        assert_eq!(toks[0].text, "x");
    }

    #[test]
    fn lone_dollar_is_word() {
        let toks = lex("$").unwrap();
        assert_eq!(toks[0].kind, TokKind::Word);
        assert_eq!(toks[0].text, "$");
    }

    #[test]
    fn single_quote_literal() {
        let toks = lex("'a $x b'").unwrap();
        assert_eq!(toks[0].parts[0].text, "a $x b");
        assert!(!toks[0].parts[0].is_var);
    }

    #[test]
    fn double_quote_interpolates() {
        let toks = lex("\"a$x\"").unwrap();
        assert_eq!(toks[0].parts.len(), 2);
        assert_eq!(toks[0].parts[0].text, "a");
        assert_eq!(toks[0].parts[1].text, "x");
        assert!(toks[0].parts[1].is_var);
    }

    #[test]
    fn mixed_quotes_concatenate() {
        let toks = lex("a\"b c\"d").unwrap();
        assert_eq!(toks[0].kind, TokKind::Word);
        assert_eq!(toks[0].text, "ab cd");
        assert_eq!(toks[0].parts.len(), 3);
    }

    #[test]
    fn comment_skipped() {
        assert_eq!(kinds("hi # note\nt here"), vec![
            TokKind::Word, TokKind::Newline, TokKind::Word, TokKind::Word, TokKind::Eof
        ]);
    }

    #[test]
    fn newline_token() {
        assert_eq!(kinds("a\nb"), vec![TokKind::Word, TokKind::Newline, TokKind::Word, TokKind::Eof]);
    }

    #[test]
    fn fd_redirect_token() {
        assert_eq!(kinds("2>err"), vec![TokKind::FdNum, TokKind::Gt, TokKind::Word, TokKind::Eof]);
    }

    #[test]
    fn plain_number_is_word() {
        assert_eq!(kinds("echo 42"), vec![TokKind::Word, TokKind::Word, TokKind::Eof]);
    }

    #[test]
    fn unterminated_quote_error() {
        let err = lex("echo 'oops").unwrap_err();
        assert_eq!(err.pos.line, 1);
        assert_eq!(err.pos.col, 6);
    }

    #[test]
    fn positions_track_lines() {
        let toks = lex("a\n  bb").unwrap();
        assert_eq!(toks[2].pos.line, 2);
        assert_eq!(toks[2].pos.col, 3);
    }

    #[test]
    fn escaped_chars_in_word() {
        let toks = lex(r"a\|b").unwrap();
        assert_eq!(toks[0].kind, TokKind::Word);
        assert_eq!(toks[0].text, "a|b");
    }

    #[test]
    fn escaped_word_is_marked_non_bare() {
        let toks = lex(r"e\nd").unwrap();
        assert_eq!(toks[0].text, "end");
        assert!(toks[0].escaped);
        let toks = lex("end").unwrap();
        assert!(!toks[0].escaped);
    }
}
