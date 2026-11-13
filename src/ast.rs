use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SourcePos {
    pub line: u32,
    pub col: u32,
}

impl fmt::Display for SourcePos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}:{}", self.line, self.col)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub pos: SourcePos,
    pub incomplete: bool,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "fush: {}: {}", self.pos, self.message)
    }
}

impl std::error::Error for ParseError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainOp {
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectMode {
    In,
    Out,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Redirect {
    pub fd: u32,
    pub mode: RedirectMode,
    pub target: Expr,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub args: Vec<Expr>,
    pub redirects: Vec<Redirect>,
}

/// A job with an optional chained continuation. `and`/`or` at statement
/// start after `;` or a newline chain onto the previous statement's job.
#[derive(Debug, Clone, PartialEq)]
pub struct Job {
    pub negated: bool,
    pub pipeline: Vec<Command>,
    pub chain: Option<(ChainOp, Box<Job>)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    pub patterns: Vec<Expr>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Job(Job),
    If {
        cond: Job,
        then_body: Vec<Stmt>,
        else_body: Vec<Stmt>,
    },
    For {
        var: String,
        items: Vec<Expr>,
        body: Vec<Stmt>,
    },
    While {
        cond: Job,
        body: Vec<Stmt>,
    },
    Switch {
        value: Expr,
        cases: Vec<Case>,
    },
    Function {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    pub stmts: Vec<Stmt>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExprKind {
    Literal,
    Var,
    CmdSubst,
    /// Command substitution inside double quotes: expands to exactly one
    /// word with embedded newlines preserved (no word-splitting).
    QuotedCmdSubst,
    Concat,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Expr {
    pub pos: SourcePos,
    pub kind: ExprKind,
    pub text: String,
    pub parts: Vec<Expr>,
    pub stmts: Vec<Stmt>,
}

impl Expr {
    pub fn literal(pos: SourcePos, text: String) -> Expr {
        Expr { pos, kind: ExprKind::Literal, text, parts: Vec::new(), stmts: Vec::new() }
    }

    pub fn var(pos: SourcePos, name: String) -> Expr {
        Expr { pos, kind: ExprKind::Var, text: name, parts: Vec::new(), stmts: Vec::new() }
    }

    pub fn concat(pos: SourcePos, parts: Vec<Expr>) -> Expr {
        Expr { pos, kind: ExprKind::Concat, text: String::new(), parts, stmts: Vec::new() }
    }

    pub fn cmd_subst(pos: SourcePos, stmts: Vec<Stmt>) -> Expr {
        Expr { pos, kind: ExprKind::CmdSubst, text: String::new(), parts: Vec::new(), stmts }
    }

    pub fn quoted_cmd_subst(pos: SourcePos, stmts: Vec<Stmt>) -> Expr {
        Expr { pos, kind: ExprKind::QuotedCmdSubst, text: String::new(), parts: Vec::new(), stmts }
    }
}
