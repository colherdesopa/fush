use crate::ast::*;
use crate::lexer::{QuotedPart, TokKind, Token, lex};

pub fn parse(src: &str) -> Result<Program, ParseError> {
    let toks = lex(src)?;
    let mut p = Parser { toks, i: 0 };
    let stmts = p.parse_stmts(&[])?;
    if p.peek().kind != TokKind::Eof {
        return Err(p.unexpected("end of input"));
    }
    Ok(Program { stmts })
}

struct Parser {
    toks: Vec<Token>,
    i: usize,
}

fn parse_index_spec(raw: &str, pos: SourcePos) -> Result<crate::ast::IndexSpec, ParseError> {
    let src = raw.trim();
    if src.is_empty() {
        return Err(ParseError {
            message: "empty index spec".into(),
            pos,
            incomplete: false,
        });
    }
    if let Some((lhs, rhs)) = src.split_once("..") {
        let start = if lhs.is_empty() {
            None
        } else {
            Some(parse_index_int(lhs, pos)?)
        };
        let end = if rhs.is_empty() {
            None
        } else {
            Some(parse_index_int(rhs, pos)?)
        };
        Ok(crate::ast::IndexSpec::Range { start, end })
    } else {
        Ok(crate::ast::IndexSpec::At(parse_index_int(src, pos)?))
    }
}

fn parse_index_int(src: &str, pos: SourcePos) -> Result<isize, ParseError> {
    if src.is_empty() {
        return Err(ParseError {
            message: "index spec cannot be empty".into(),
            pos,
            incomplete: false,
        });
    }
    if src.starts_with('+') {
        return Err(ParseError {
            message: "positive index values are not supported; use a signed integer".into(),
            pos,
            incomplete: false,
        });
    }
    let parsed = src.parse::<isize>().map_err(|_| ParseError {
        message: format!(
            "invalid index '{}', expected integer or range like '1..2'",
            src
        ),
        pos,
        incomplete: false,
    })?;
    Ok(parsed)
}

fn attach_job_chain(target: &mut Option<(ChainOp, Box<Job>)>, op: ChainOp, next: Job) {
    match target {
        Some((_, job)) => {
            if job.chain.is_some() {
                attach_job_chain(&mut job.chain, op, next);
            } else {
                job.chain = Some((op, Box::new(next)));
            }
        }
        None => *target = Some((op, Box::new(next))),
    }
}

fn attach_stmt_chain(target: &mut Option<(ChainOp, Box<Stmt>)>, op: ChainOp, next: Stmt) {
    match target {
        Some((_, stmt)) => match stmt.as_mut() {
            Stmt::Job(job) => {
                if let Stmt::Job(next_job) = next {
                    if job.chain.is_some() {
                        attach_job_chain(&mut job.chain, op, next_job);
                    } else {
                        job.chain = Some((op, Box::new(next_job)));
                    }
                } else {
                    *target = Some((op, Box::new(next)));
                }
            }
            Stmt::If { chain, .. }
            | Stmt::For { chain, .. }
            | Stmt::While { chain, .. }
            | Stmt::Switch { chain, .. }
            | Stmt::Function { chain, .. } => {
                if chain.is_some() {
                    attach_stmt_chain(chain, op, next);
                } else {
                    *target = Some((op, Box::new(next)));
                }
            }
        },
        None => *target = Some((op, Box::new(next))),
    }
}

impl Parser {
    fn peek(&self) -> &Token {
        &self.toks[self.i]
    }

    fn next(&mut self) -> Token {
        let t = self.toks[self.i].clone();
        if self.i + 1 < self.toks.len() {
            self.i += 1;
        }
        t
    }

    fn unexpected(&self, expected: &str) -> ParseError {
        let t = self.peek();
        let what = match t.kind {
            TokKind::Eof => "end of input".to_string(),
            TokKind::Newline => "newline".to_string(),
            TokKind::Word if t.text.is_empty() => {
                let joined: String = t.parts.iter().map(|p| p.text.clone()).collect();
                format!("'\"{joined}\"'")
            }
            _ => format!("'{}'", t.text),
        };
        ParseError {
            message: format!("unexpected {what}, expected {expected}"),
            pos: t.pos,
            incomplete: t.kind == TokKind::Eof,
        }
    }

    fn skip_terminators(&mut self) {
        while matches!(self.peek().kind, TokKind::Newline | TokKind::Semi) {
            self.next();
        }
    }

    fn parse_stmts(&mut self, stop: &[&str]) -> Result<Vec<Stmt>, ParseError> {
        let mut stmts = Vec::new();
        loop {
            self.skip_terminators();
            let t = self.peek().clone();
            match t.kind {
                TokKind::Eof | TokKind::RParen => break,
                TokKind::Word if stop.iter().any(|kw| self.bare_word(&t, kw)) => break,
                TokKind::Word
                    if (self.bare_word(&t, "and") || self.bare_word(&t, "or"))
                        && !stmts.is_empty() =>
                {
                    let op = if self.bare_word(&t, "and") {
                        ChainOp::And
                    } else {
                        ChainOp::Or
                    };
                    self.next();
                    let prev = stmts.last_mut().expect("checked above");
                    match prev {
                        Stmt::Job(job) => {
                            let next = self.parse_job()?;
                            attach_job_chain(&mut job.chain, op, next);
                        }
                        Stmt::If { chain, .. }
                        | Stmt::For { chain, .. }
                        | Stmt::While { chain, .. }
                        | Stmt::Switch { chain, .. }
                        | Stmt::Function { chain, .. } => {
                            let next = self.parse_stmt()?;
                            attach_stmt_chain(chain, op, next);
                        }
                    }
                }
                _ => stmts.push(self.parse_stmt()?),
            }
        }
        Ok(stmts)
    }

    fn expect_keyword(&mut self, kw: &str) -> Result<(), ParseError> {
        if self.at_keyword(kw) {
            self.next();
            Ok(())
        } else {
            Err(self.unexpected(&format!("'{kw}'")))
        }
    }

    fn at_keyword(&self, kw: &str) -> bool {
        self.bare_word(self.peek(), kw)
    }

    fn bare_word(&self, t: &Token, kw: &str) -> bool {
        t.kind == TokKind::Word
            && !t.escaped
            && t.parts.len() == 1
            && t.parts[0].quote.is_none()
            && !t.parts[0].is_var
            && t.parts[0].text == kw
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        if self.at_keyword("function") {
            self.next();
            return self.parse_function();
        }
        if self.at_keyword("if") {
            self.next();
            return self.parse_if();
        }
        if self.at_keyword("for") {
            self.next();
            return self.parse_for();
        }
        if self.at_keyword("while") {
            self.next();
            return self.parse_while();
        }
        if self.at_keyword("switch") {
            self.next();
            return self.parse_switch();
        }
        Ok(Stmt::Job(self.parse_job()?))
    }

    fn parse_function(&mut self) -> Result<Stmt, ParseError> {
        let t = self.peek().clone();
        if t.kind != TokKind::Word
            || t.escaped
            || t.parts.len() != 1
            || t.parts[0].quote.is_some()
            || t.parts[0].is_var
        {
            return Err(ParseError {
                message: "expected function name".into(),
                pos: t.pos,
                incomplete: t.kind == TokKind::Eof,
            });
        }
        let name = self.next().text;

        let mut params = Vec::new();
        while !matches!(
            self.peek().kind,
            TokKind::Newline | TokKind::Semi | TokKind::Eof
        ) {
            let t = self.peek().clone();
            match t.kind {
                TokKind::Word => params.push(self.next().text),
                _ => break,
            }
        }

        let body = self.parse_block_start()?;
        self.expect_keyword("end")?;
        Ok(Stmt::Function {
            name,
            params,
            body,
            chain: None,
        })
    }

    fn parse_if(&mut self) -> Result<Stmt, ParseError> {
        let cond = self.parse_job()?;
        let then_body = self.parse_block_start()?;
        let mut else_body = Vec::new();
        if self.at_keyword("else") {
            self.next();
            if self.at_keyword("if") {
                self.next();
                let nested = self.parse_if_no_end()?;
                self.expect_keyword("end")?;
                else_body.push(nested);
            } else {
                else_body = self.parse_block_start()?;
                self.expect_keyword("end")?;
            }
        } else {
            self.expect_keyword("end")?;
        }
        Ok(Stmt::If {
            cond,
            then_body,
            else_body,
            chain: None,
        })
    }

    fn parse_if_no_end(&mut self) -> Result<Stmt, ParseError> {
        let cond = self.parse_job()?;
        let then_body = self.parse_block_start()?;
        let mut else_body = Vec::new();
        if self.at_keyword("else") {
            self.next();
            if self.at_keyword("if") {
                self.next();
                else_body.push(self.parse_if_no_end()?);
            } else {
                else_body = self.parse_block_start()?;
            }
        }
        Ok(Stmt::If {
            cond,
            then_body,
            else_body,
            chain: None,
        })
    }

    fn parse_block_start(&mut self) -> Result<Vec<Stmt>, ParseError> {
        self.skip_terminators();
        self.parse_stmts(&["else", "case", "end"])
    }

    fn parse_for(&mut self) -> Result<Stmt, ParseError> {
        let t = self.next();
        if t.kind != TokKind::Word
            || t.parts.len() != 1
            || t.parts[0].quote.is_some()
            || t.parts[0].is_var
        {
            return Err(ParseError {
                message: "expected variable name".into(),
                pos: t.pos,
                incomplete: false,
            });
        }
        let var = t.text;
        self.expect_keyword("in")?;
        let mut items = Vec::new();
        while !matches!(
            self.peek().kind,
            TokKind::Newline | TokKind::Semi | TokKind::Eof
        ) {
            items.push(self.parse_expr()?);
        }
        let body = self.parse_block_start()?;
        self.expect_keyword("end")?;
        Ok(Stmt::For {
            var,
            items,
            body,
            chain: None,
        })
    }

    fn parse_while(&mut self) -> Result<Stmt, ParseError> {
        let cond = self.parse_job()?;
        let body = self.parse_block_start()?;
        self.expect_keyword("end")?;
        Ok(Stmt::While {
            cond,
            body,
            chain: None,
        })
    }

    fn parse_switch(&mut self) -> Result<Stmt, ParseError> {
        let value = self.parse_expr()?;
        let mut cases = Vec::new();
        loop {
            self.skip_terminators();
            if self.at_keyword("end") {
                self.next();
                break;
            }
            if self.peek().kind == TokKind::Eof {
                return Err(self.unexpected("'case' or 'end'"));
            }
            self.expect_keyword("case")?;
            let mut patterns = Vec::new();
            while !matches!(
                self.peek().kind,
                TokKind::Newline | TokKind::Semi | TokKind::Eof
            ) {
                patterns.push(self.parse_expr()?);
            }
            if patterns.is_empty() {
                return Err(self.unexpected("at least one pattern"));
            }
            let body = self.parse_block_start()?;
            cases.push(Case { patterns, body });
        }
        Ok(Stmt::Switch {
            value,
            cases,
            chain: None,
        })
    }

    fn parse_job(&mut self) -> Result<Job, ParseError> {
        let mut negated = false;
        while self.at_keyword("not") {
            self.next();
            negated = !negated;
        }
        let mut pipeline = vec![self.parse_command()?];
        while self.peek().kind == TokKind::Pipe {
            self.next();
            pipeline.push(self.parse_command()?);
        }
        let mut chain = None;
        let kw = if self.at_keyword("and") {
            Some(ChainOp::And)
        } else if self.at_keyword("or") {
            Some(ChainOp::Or)
        } else {
            None
        };
        if let Some(op) = kw {
            self.next();
            let rest = self.parse_job()?;
            chain = Some((op, Box::new(rest)));
        }
        Ok(Job {
            negated,
            pipeline,
            chain,
        })
    }

    fn parse_command(&mut self) -> Result<Command, ParseError> {
        let mut args = Vec::new();
        let mut redirects = Vec::new();
        loop {
            let t = self.peek().clone();
            match t.kind {
                TokKind::Word if self.bare_word(&t, "and") || self.bare_word(&t, "or") => {
                    break;
                }
                TokKind::Word | TokKind::Dollar | TokKind::LParen => {
                    let mut expr = {
                        self.next();
                        self.token_to_expr(&t)?
                    };
                    while matches!(
                        self.peek().kind,
                        TokKind::Word | TokKind::Dollar | TokKind::LParen
                    ) && self.adjacent_concat_arg(&t, self.peek())
                    {
                        let next_t = self.peek().clone();
                        self.next();
                        let next_expr = self.token_to_expr(&next_t)?;
                        expr = self.concat_expr(expr, next_expr);
                    }
                    args.push(expr);
                }
                TokKind::FdNum | TokKind::Lt | TokKind::Gt => {
                    self.next();
                    let (fd, mode) = match t.kind {
                        TokKind::FdNum => {
                            let n: u32 = t.text.parse().map_err(|_| ParseError {
                                message: format!("bad file descriptor '{}'", t.text),
                                pos: t.pos,
                                incomplete: false,
                            })?;
                            let mode = match self.peek().kind {
                                TokKind::Lt => RedirectMode::In,
                                TokKind::Gt => RedirectMode::Out,
                                _ => return Err(self.unexpected("'<' or '>'")),
                            };
                            self.next();
                            (n, mode)
                        }
                        TokKind::Lt => (0, RedirectMode::In),
                        TokKind::Gt => (1, RedirectMode::Out),
                        _ => unreachable!(),
                    };
                    let target = self.parse_expr()?;
                    redirects.push(Redirect { fd, mode, target });
                }
                _ => break,
            }
        }
        if args.is_empty() {
            return Err(self.unexpected("a command"));
        }
        Ok(Command { args, redirects })
    }

    fn adjacent_concat_arg(&self, prev: &Token, next: &Token) -> bool {
        if prev.kind != TokKind::Dollar {
            return false;
        }
        if next.pos.line != prev.pos.line {
            return false;
        }
        let prev_end = prev.pos.col + prev.text.len() as u32 + 1;
        next.pos.col <= prev_end
    }

    fn concat_expr(&self, left: Expr, right: Expr) -> Expr {
        let left_quoted = left.quoted;
        let right_quoted = right.quoted;
        let mut parts = match left.kind {
            ExprKind::Concat => left.parts,
            _ => vec![left],
        };
        match right.kind {
            ExprKind::Concat => parts.extend(right.parts),
            _ => parts.push(right),
        }
        let mut expr = Expr::concat(parts.first().map(|p| p.pos).unwrap_or_default(), parts);
        expr.quoted = left_quoted || right_quoted;
        expr
    }

    fn token_to_expr(&mut self, t: &Token) -> Result<Expr, ParseError> {
        match t.kind {
            TokKind::Dollar => {
                if let Some(raw) = &t.index {
                    let index = parse_index_spec(raw, t.pos)?;
                    Ok(Expr::index(t.pos, t.text.clone(), index))
                } else {
                    Ok(Expr::var(t.pos, t.text.clone()))
                }
            }
            TokKind::LParen => {
                let stmts = self.parse_stmts(&[])?;
                if stmts.is_empty() {
                    return Err(ParseError {
                        message: "empty command substitution, expected a command inside '()'"
                            .into(),
                        pos: t.pos,
                        incomplete: false,
                    });
                }
                let close = self.peek().clone();
                if close.kind != TokKind::RParen {
                    return Err(ParseError {
                        message: "unterminated command substitution, expected ')'".into(),
                        pos: t.pos,
                        incomplete: false,
                    });
                }
                self.next();
                Ok(Expr::cmd_subst(t.pos, stmts))
            }
            TokKind::Word => {
                if t.parts.len() == 1 && t.parts[0].quote.is_none() && !t.parts[0].is_var {
                    return Ok(Expr::literal(t.pos, t.parts[0].text.clone()));
                }
                let mut parts = Vec::new();
                for p in &t.parts {
                    self.push_part(t.pos, p, &mut parts)?;
                }
                let mut expr = Expr::concat(t.pos, parts);
                expr.quoted = t.parts.iter().any(|p| p.quote.is_some());
                Ok(expr)
            }
            _ => Err(self.unexpected("a word")),
        }
    }

    fn push_part(
        &mut self,
        pos: SourcePos,
        p: &QuotedPart,
        parts: &mut Vec<Expr>,
    ) -> Result<(), ParseError> {
        if p.is_var {
            if let Some(raw) = &p.index {
                parts.push(Expr::index(
                    pos,
                    p.text.clone(),
                    parse_index_spec(raw, pos)?,
                ));
            } else {
                parts.push(Expr::var(pos, p.text.clone()));
            }
        } else if p.is_subst {
            let sub_toks = crate::lexer::lex(&p.text)?;
            let mut sub = Parser {
                toks: sub_toks,
                i: 0,
            };
            let stmts = sub.parse_stmts(&[])?;
            if stmts.is_empty() {
                return Err(ParseError {
                    message: "empty command substitution, expected a command inside '()'".into(),
                    pos,
                    incomplete: false,
                });
            }
            if sub.peek().kind != TokKind::Eof {
                return Err(sub.unexpected("end of command substitution"));
            }
            parts.push(Expr::quoted_cmd_subst(pos, stmts));
        } else {
            parts.push(Expr::literal(pos, p.text.clone()));
        }
        Ok(())
    }

    fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        let t = self.peek().clone();
        match t.kind {
            TokKind::Word | TokKind::Dollar | TokKind::LParen => {
                self.next();
                self.token_to_expr(&t)
            }
            _ => Err(self.unexpected("an argument")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(src: &str) -> Program {
        parse(src).unwrap_or_else(|e| panic!("parse failed: {e}"))
    }

    #[test]
    fn eof_inside_block_marks_error_incomplete() {
        let err = parse("if true").expect_err("must fail");
        assert!(err.incomplete);
        assert!(err.message.contains("end of input"));
    }

    #[test]
    fn terminal_parse_error_is_not_incomplete() {
        let err = parse("echo ) bad").expect_err("must fail");
        assert!(!err.incomplete);
    }

    fn first_job(src: &str) -> Job {
        match parse_ok(src).stmts.remove(0) {
            Stmt::Job(j) => j,
            other => panic!("expected job, got {other:?}"),
        }
    }

    fn lit(e: &Expr) -> &str {
        assert_eq!(e.kind, ExprKind::Literal);
        &e.text
    }

    #[test]
    fn simple_command() {
        let j = first_job("echo hello");
        assert_eq!(j.pipeline.len(), 1);
        assert_eq!(lit(&j.pipeline[0].args[0]), "echo");
        assert_eq!(lit(&j.pipeline[0].args[1]), "hello");
    }

    #[test]
    fn set_is_plain_command() {
        let j = first_job("set x 1 2 3");
        assert_eq!(lit(&j.pipeline[0].args[0]), "set");
        assert_eq!(j.pipeline[0].args.len(), 5);
    }

    #[test]
    fn pipeline() {
        let j = first_job("cat foo | grep bar | wc -l");
        assert_eq!(j.pipeline.len(), 3);
        assert_eq!(lit(&j.pipeline[2].args[0]), "wc");
    }

    #[test]
    fn and_chain() {
        let j = first_job("mkdir d and cd d");
        let (_, rest) = j.chain.unwrap();
        assert_eq!(lit(&rest.pipeline[0].args[0]), "cd");
    }

    #[test]
    fn or_chain() {
        let j = first_job("test -f x or echo missing");
        assert!(matches!(j.chain.unwrap().0, ChainOp::Or));
    }

    #[test]
    fn not_negates() {
        let j = first_job("not test -f x");
        assert!(j.negated);
    }

    #[test]
    fn var_expr() {
        let j = first_job("echo $x");
        let a = &j.pipeline[0].args[1];
        assert_eq!(a.kind, ExprKind::Var);
        assert_eq!(a.text, "x");
    }

    #[test]
    fn var_index_expr() {
        let j = first_job("echo $x[2]");
        let a = &j.pipeline[0].args[1];
        assert_eq!(a.kind, ExprKind::Index);
        assert_eq!(a.text, "x");
        assert!(matches!(a.index, Some(crate::ast::IndexSpec::At(2))));
    }

    #[test]
    fn var_slice_expr() {
        let j = first_job("echo $x[1..3]");
        let a = &j.pipeline[0].args[1];
        assert_eq!(a.kind, ExprKind::Index);
        assert!(matches!(
            a.index,
            Some(crate::ast::IndexSpec::Range {
                start: Some(1),
                end: Some(3)
            })
        ));
    }

    #[test]
    fn invalid_var_index_rejected() {
        assert!(parse("echo $x[a]").is_err());
        assert!(parse("echo $x[1").is_err());
        assert!(parse("echo $x[1][2]").is_err());
    }

    #[test]
    fn command_substitution() {
        let j = first_job("echo (ls)");
        let a = &j.pipeline[0].args[1];
        assert_eq!(a.kind, ExprKind::CmdSubst);
        match &a.stmts[0] {
            Stmt::Job(sub) => assert_eq!(lit(&sub.pipeline[0].args[0]), "ls"),
            other => panic!("expected job in subst, got {other:?}"),
        }
    }

    #[test]
    fn nested_command_substitution() {
        let j = first_job("echo (echo (echo hi))");
        let a = &j.pipeline[0].args[1];
        assert_eq!(a.kind, ExprKind::CmdSubst);
        let Stmt::Job(sub1) = &a.stmts[0] else {
            panic!()
        };
        let inner = &sub1.pipeline[0].args[1];
        assert_eq!(inner.kind, ExprKind::CmdSubst);
        let Stmt::Job(sub2) = &inner.stmts[0] else {
            panic!()
        };
        assert_eq!(lit(&sub2.pipeline[0].args[1]), "hi");
    }

    #[test]
    fn unterminated_cmd_subst_error() {
        let err = parse("echo (ls").unwrap_err();
        assert_eq!(err.pos.line, 1);
        assert_eq!(err.pos.col, 6);
    }

    #[test]
    fn if_else_end() {
        let stmts = parse_ok("if test $x = 1; echo yes; else; echo no; end").stmts;
        let Stmt::If {
            cond,
            then_body,
            else_body,
            ..
        } = &stmts[0]
        else {
            panic!()
        };
        assert_eq!(lit(&cond.pipeline[0].args[0]), "test");
        assert_eq!(then_body.len(), 1);
        assert_eq!(else_body.len(), 1);
    }

    #[test]
    fn if_multiline() {
        let stmts = parse_ok("if true\n  echo a\n  echo b\nend").stmts;
        let Stmt::If { then_body, .. } = &stmts[0] else {
            panic!()
        };
        assert_eq!(then_body.len(), 2);
    }

    #[test]
    fn else_if() {
        let stmts = parse_ok("if a; x; else if b; y; else; z; end").stmts;
        let Stmt::If { else_body, .. } = &stmts[0] else {
            panic!()
        };
        assert!(matches!(else_body[0], Stmt::If { .. }));
    }

    #[test]
    fn missing_end_error() {
        let err = parse("if true; echo x").unwrap_err();
        assert!(err.message.contains("end"));
        assert_eq!(err.pos.line, 1);
    }

    #[test]
    fn for_loop() {
        let stmts = parse_ok("for f in *.txt $files; echo $f; end").stmts;
        let Stmt::For {
            var, items, body, ..
        } = &stmts[0]
        else {
            panic!()
        };
        assert_eq!(var, "f");
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].kind, ExprKind::Literal);
        assert_eq!(items[1].kind, ExprKind::Var);
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn while_loop() {
        let stmts = parse_ok("while test $i -lt 10; end").stmts;
        assert!(matches!(&stmts[0], Stmt::While { .. }));
    }

    #[test]
    fn switch_case() {
        let stmts = parse_ok("switch $x; case a; echo A; case b c; echo BC; end").stmts;
        let Stmt::Switch { cases, .. } = &stmts[0] else {
            panic!()
        };
        assert_eq!(cases.len(), 2);
        assert_eq!(cases[1].patterns.len(), 2);
    }

    #[test]
    fn keywords_only_when_bare_words() {
        let j = first_job("echo end");
        assert_eq!(lit(&j.pipeline[0].args[1]), "end");
        let j = first_job("'if' x");
        let a = &j.pipeline[0].args[0];
        assert_eq!(a.kind, ExprKind::Concat);
        assert_eq!(a.parts[0].text, "if");
    }

    #[test]
    fn quoted_keywords_stay_arguments() {
        let j = first_job("echo 'and' end");
        assert_eq!(j.pipeline[0].args.len(), 3);
        let a = &j.pipeline[0].args[1];
        assert_eq!(a.kind, ExprKind::Concat);
        assert_eq!(a.parts[0].text, "and");
    }

    #[test]
    fn redirect_out() {
        let j = first_job("echo hi > out.txt");
        let cmd = &j.pipeline[0];
        assert!(cmd.args.len() == 2);
        assert_eq!(cmd.redirects.len(), 1);
        assert_eq!(cmd.redirects[0].fd, 1);
        assert!(matches!(cmd.redirects[0].mode, RedirectMode::Out));
        assert_eq!(lit(&cmd.redirects[0].target), "out.txt");
    }

    #[test]
    fn redirect_in_and_fd() {
        let j = first_job("sort < in.txt 2> err.txt");
        let r = &j.pipeline[0].redirects;
        assert_eq!(r.len(), 2);
        assert!(matches!(r[0].mode, RedirectMode::In));
        assert_eq!(r[0].fd, 0);
        assert_eq!(r[1].fd, 2);
    }

    #[test]
    fn redirect_target_can_be_var() {
        let j = first_job("echo hi > $out");
        assert_eq!(j.pipeline[0].redirects[0].target.kind, ExprKind::Var);
    }

    #[test]
    fn multiple_statements_and_empty_lines() {
        let p = parse_ok("echo a\n\nset x 1; echo $x\n");
        assert_eq!(p.stmts.len(), 3);
    }

    #[test]
    fn empty_input() {
        let p = parse_ok("");
        assert!(p.stmts.is_empty());
        let p = parse_ok("   \n  ; ");
        assert!(p.stmts.is_empty());
    }

    #[test]
    fn concatenation_expr() {
        let j = first_job("echo pre$x");
        let a = &j.pipeline[0].args[1];
        assert_eq!(a.kind, ExprKind::Concat);
        assert_eq!(a.parts.len(), 2);
        assert_eq!(a.parts[0].kind, ExprKind::Literal);
        assert_eq!(a.parts[1].kind, ExprKind::Var);
    }

    #[test]
    fn quoted_string_is_literal() {
        let j = first_job("echo \"a $x b\" 'c d'");
        let a = &j.pipeline[0].args[1];
        assert_eq!(a.kind, ExprKind::Concat);
        assert_eq!(a.parts.len(), 3);
        let b = &j.pipeline[0].args[2];
        assert_eq!(b.kind, ExprKind::Concat);
        assert_eq!(b.parts.len(), 1);
        assert_eq!(b.parts[0].text, "c d");
    }

    #[test]
    fn error_position_on_second_line() {
        let err = parse("echo ok\nfoo |").unwrap_err();
        assert_eq!(err.pos.line, 2);
    }

    #[test]
    fn pipe_without_command_error() {
        let err = parse("echo |").unwrap_err();
        assert_eq!(err.pos.line, 1);
        assert!(err.message.contains("unexpected"));
    }

    #[test]
    fn comment_only_line() {
        let p = parse_ok("# just a comment");
        assert!(p.stmts.is_empty());
    }

    #[test]
    fn escaped_end_is_plain_argument() {
        let mut p = parse_ok("if true\n    echo e\\nd\nend");
        let Stmt::If { then_body, .. } = p.stmts.remove(0) else {
            panic!()
        };
        let Stmt::Job(sub) = &then_body[0] else {
            panic!()
        };
        assert_eq!(lit(&sub.pipeline[0].args[0]), "echo");
        assert_eq!(lit(&sub.pipeline[0].args[1]), "end");
    }

    #[test]
    fn stacked_not() {
        let j = first_job("not not true");
        assert!(!j.negated);
        let j = first_job("not true");
        assert!(j.negated);
    }

    #[test]
    fn empty_parens_error() {
        let err = parse("echo ()").unwrap_err();
        assert_eq!(err.pos.line, 1);
        assert_eq!(err.pos.col, 6);
        assert!(err.message.contains("empty command substitution"));
    }

    #[test]
    fn escaped_end_does_not_terminate_block() {
        let mut p = parse_ok("if true\ne\\nd\necho hi\nend");
        let Stmt::If { then_body, .. } = p.stmts.remove(0) else {
            panic!()
        };
        assert_eq!(then_body.len(), 2);
        let Stmt::Job(sub) = &then_body[0] else {
            panic!()
        };
        assert_eq!(lit(&sub.pipeline[0].args[0]), "end");
        let Stmt::Job(sub) = &then_body[1] else {
            panic!()
        };
        assert_eq!(lit(&sub.pipeline[0].args[0]), "echo");
        assert_eq!(lit(&sub.pipeline[0].args[1]), "hi");
    }

    #[test]
    fn escaped_not_is_command_name() {
        let j = first_job("not no\\t true");
        assert!(j.negated);
        assert_eq!(j.pipeline[0].args.len(), 2);
        assert_eq!(lit(&j.pipeline[0].args[0]), "not");
        assert_eq!(lit(&j.pipeline[0].args[1]), "true");
    }

    #[test]
    fn escaped_if_word_is_condition_job() {
        let mut p = parse_ok("if tru\\e\necho hi\nend");
        let Stmt::If {
            cond, then_body, ..
        } = p.stmts.remove(0)
        else {
            panic!()
        };
        assert_eq!(lit(&cond.pipeline[0].args[0]), "true");
        assert_eq!(then_body.len(), 1);
    }

    #[test]
    fn empty_cmdsubst_with_newline_error() {
        let err = parse("echo (\n)").unwrap_err();
        assert_eq!(err.pos.line, 1);
        assert_eq!(err.pos.col, 6);
        assert!(err.message.contains("empty command substitution"));
    }

    #[test]
    fn quoted_token_error_message() {
        let err = parse("switch x\n'end'").unwrap_err();
        assert!(err.message.contains("\"end\""), "message: {}", err.message);
    }

    #[test]
    fn cmd_subst_with_pipeline_inside() {
        let j = first_job("echo (cat f | wc -l)");
        let a = &j.pipeline[0].args[1];
        let Stmt::Job(sub) = &a.stmts[0] else {
            panic!()
        };
        assert_eq!(sub.pipeline.len(), 2);
    }

    #[test]
    fn function_definition_parses() {
        let p = parse_ok("function greet --description x\n  echo hi\nend");
        let Stmt::Function {
            name, params, body, ..
        } = &p.stmts[0]
        else {
            panic!()
        };
        assert_eq!(name, "greet");
        assert_eq!(params, &["--description", "x"]);
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn function_requires_name() {
        let err = parse("function\nend").unwrap_err();
        assert!(err.message.contains("function name"));
    }

    #[test]
    fn missing_end_in_function_is_incomplete() {
        let err = parse("function greet\n  echo hi").unwrap_err();
        assert!(err.incomplete);
        assert!(err.message.contains("end"));
    }

    #[test]
    fn nested_function_definition() {
        let p = parse_ok("function outer\n  function inner\n    echo hi\n  end\nend");
        let Stmt::Function { body, .. } = &p.stmts[0] else {
            panic!()
        };
        assert!(matches!(&body[0], Stmt::Function { name, .. } if name == "inner"));
    }

    #[test]
    fn function_params_with_dashes() {
        let p = parse_ok("function foo --description x\n  echo hi\nend");
        let Stmt::Function { params, body, .. } = &p.stmts[0] else {
            panic!()
        };
        assert_eq!(params, &["--description", "x"]);
        assert_eq!(body.len(), 1);
    }

    #[test]
    fn function_keyword_is_not_plain_arg() {
        let j = first_job("echo function");
        assert_eq!(lit(&j.pipeline[0].args[1]), "function");
    }

    #[test]
    fn quoted_function_name_is_error() {
        let err = parse("function \"foo\"").unwrap_err();
        assert!(err.message.contains("function name"));
    }
}
