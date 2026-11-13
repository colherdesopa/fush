use crate::ast::{Expr, ExprKind, Stmt};
use crate::exec;
use std::collections::{HashMap, HashSet};

/// Variable environment: a global table plus a stack of local scopes.
/// Lookup checks the innermost local scope first, then globals.
#[derive(Default)]
pub struct Env {
    globals: HashMap<String, Vec<String>>,
    locals: Vec<HashMap<String, Vec<String>>>,
    exported: HashSet<String>,
}

impl Env {
    pub fn get(&self, name: &str) -> Option<Vec<String>> {
        for scope in self.locals.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v.clone());
            }
        }
        self.globals.get(name).cloned()
    }

    pub fn set_global(&mut self, name: &str, values: Vec<String>) {
        self.globals.insert(name.to_string(), values);
    }

    pub fn set_local(&mut self, name: &str, values: Vec<String>) {
        if self.locals.is_empty() {
            self.locals.push(HashMap::new());
        }
        let top = self.locals.last_mut().expect("scope pushed above");
        top.insert(name.to_string(), values);
    }

    pub fn export(&mut self, name: &str) {
        self.exported.insert(name.to_string());
    }

    pub fn is_exported(&self, name: &str) -> bool {
        self.exported.contains(name)
    }

    pub fn push_scope(&mut self) {
        self.locals.push(HashMap::new());
    }

    pub fn pop_scope(&mut self) {
        self.locals.pop();
    }

    /// Name/value pairs to overlay on the inherited process environment
    /// when spawning child processes.
    pub fn child_overrides(&self) -> Vec<(String, String)> {
        self.globals
            .iter()
            .filter(|(name, _)| self.is_exported(name))
            .map(|(name, vals)| (name.clone(), vals.join(":")))
            .collect()
    }
}

/// Interpreter state: the environment, the last pipeline status ($status),
/// and an optional stdout capture buffer used for command substitution and
/// builtin-in-pipeline capture.
pub struct Runtime {
    pub env: Env,
    pub status: i32,
    pub exited: Option<i32>,
    pub(crate) capture: Option<Vec<u8>>,
}

impl Runtime {
    pub fn new() -> Runtime {
        Runtime { env: Env::default(), status: 0, exited: None, capture: None }
    }

    /// Expand one Expr into fish-style words.
    ///
    /// - Literal: the text as a single word.
    /// - Var: the variable's values as separate words; undefined expands to
    ///   zero words.
    /// - CmdSubst: run the statements, capture stdout, trim trailing
    ///   newlines, split on newlines into words.
    /// - Concat: parts are joined into a single word; var parts contribute
    ///   their values joined with spaces (documented simplification of fish's
    ///   cartesian product for multi-value vars inside concatenation).
    pub fn expand(&mut self, e: &Expr) -> Result<Vec<String>, String> {
        match e.kind {
            ExprKind::Literal => Ok(vec![e.text.clone()]),
            ExprKind::Var => Ok(self.env.get(&e.text).unwrap_or_default()),
            ExprKind::CmdSubst => {
                let out = self.capture_stmts(&e.stmts)?;
                Ok(cmdsubst_words(&out))
            }
            ExprKind::Concat => {
                let mut word = String::new();
                for part in &e.parts {
                    match part.kind {
                        ExprKind::Literal => word.push_str(&part.text),
                        ExprKind::Var => {
                            let vals = self.env.get(&part.text).unwrap_or_default();
                            word.push_str(&vals.join(" "));
                        }
                        ExprKind::CmdSubst => {
                            let out = self.capture_stmts(&part.stmts)?;
                            let joined = cmdsubst_words(&out).join(" ");
                            word.push_str(&joined);
                        }
                        ExprKind::Concat => {
                            let words = self.expand(part)?;
                            word.push_str(&words.join(" "));
                        }
                    }
                }
                Ok(vec![word])
            }
        }
    }

    /// Expand an Expr and require exactly one word.
    pub fn expand_one(&mut self, e: &Expr) -> Result<String, String> {
        let words = self.expand(e)?;
        match words.len() {
            1 => Ok(words.into_iter().next().expect("checked len")),
            0 => Ok(String::new()),
            _ => Err(format!("{}: expanded to {} words, expected one", e.pos, words.len())),
        }
    }

    /// Run statements with stdout captured; restores the previous capture.
    pub(crate) fn capture_stmts(&mut self, stmts: &[Stmt]) -> Result<Vec<u8>, String> {
        let saved = self.capture.take();
        self.capture = Some(Vec::new());
        let result = exec::run_stmts(self, stmts);
        let buf = self.capture.take().unwrap_or_default();
        self.capture = saved;
        result?;
        Ok(buf)
    }
}

/// Turn captured command-substitution stdout into words: trim trailing
/// newlines, then split on newlines (fish behavior).
pub(crate) fn cmdsubst_words(out: &[u8]) -> Vec<String> {
    let s = String::from_utf8_lossy(out);
    let trimmed = s.trim_end_matches('\n');
    if trimmed.is_empty() {
        Vec::new()
    } else {
        trimmed.split('\n').map(str::to_string).collect()
    }
}

/// Minimal fnmatch-style glob: `*` (any run) and `?` (any single char).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    glob_inner(pattern.as_bytes(), text.as_bytes())
}

fn glob_inner(p: &[u8], t: &[u8]) -> bool {
    if p.is_empty() {
        return t.is_empty();
    }
    match p[0] {
        b'*' => {
            for i in 0..=t.len() {
                if glob_inner(&p[1..], &t[i..]) {
                    return true;
                }
            }
            false
        }
        b'?' => !t.is_empty() && glob_inner(&p[1..], &t[1..]),
        c => !t.is_empty() && t[0] == c && glob_inner(&p[1..], &t[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt_with(vars: &[(&str, &[&str])]) -> Runtime {
        let mut rt = Runtime::new();
        for (name, vals) in vars {
            rt.env.set_global(
                name,
                vals.iter().map(|s| s.to_string()).collect(),
            );
        }
        rt
    }

    #[test]
    fn literal_is_one_word() {
        let mut rt = Runtime::new();
        let e = Expr::literal(Default::default(), "hello".into());
        assert_eq!(rt.expand(&e).unwrap(), vec!["hello"]);
    }

    #[test]
    fn var_expands_to_separate_words() {
        let mut rt = rt_with(&[("x", &["a", "b", "c"])]);
        let e = Expr::var(Default::default(), "x".into());
        assert_eq!(rt.expand(&e).unwrap(), vec!["a", "b", "c"]);
    }

    #[test]
    fn undefined_var_expands_to_zero_words() {
        let mut rt = Runtime::new();
        let e = Expr::var(Default::default(), "nope".into());
        assert_eq!(rt.expand(&e).unwrap(), Vec::<String>::new());
    }

    #[test]
    fn concat_joins_var_values_with_spaces() {
        let mut rt = rt_with(&[("x", &["a", "b"])]);
        let e = Expr::concat(
            Default::default(),
            vec![
                Expr::literal(Default::default(), "pre-".into()),
                Expr::var(Default::default(), "x".into()),
                Expr::literal(Default::default(), "-post".into()),
            ],
        );
        assert_eq!(rt.expand(&e).unwrap(), vec!["pre-a b-post"]);
    }

    #[test]
    fn local_shadows_global_and_pop_restores() {
        let mut rt = rt_with(&[("x", &["global"])]);
        rt.env.push_scope();
        rt.env.set_local("x", vec!["local".into()]);
        assert_eq!(rt.env.get("x"), Some(vec!["local".into()]));
        rt.env.pop_scope();
        assert_eq!(rt.env.get("x"), Some(vec!["global".into()]));
    }

    #[test]
    fn nested_local_scopes_resolve_innermost_first() {
        let mut rt = Runtime::new();
        rt.env.push_scope();
        rt.env.set_local("a", vec!["outer".into()]);
        rt.env.push_scope();
        rt.env.set_local("a", vec!["inner".into()]);
        assert_eq!(rt.env.get("a"), Some(vec!["inner".into()]));
        rt.env.pop_scope();
        assert_eq!(rt.env.get("a"), Some(vec!["outer".into()]));
    }

    #[test]
    fn set_local_without_scope_creates_one() {
        let mut rt = Runtime::new();
        rt.env.set_local("y", vec!["1".into()]);
        assert_eq!(rt.env.get("y"), Some(vec!["1".into()]));
    }

    #[test]
    fn exported_vars_become_child_overrides() {
        let mut rt = Runtime::new();
        rt.env.set_global("PATH", vec!["/bin", "/usr/bin"].iter().map(|s| s.to_string()).collect());
        rt.env.export("PATH");
        rt.env.set_global("PRIVATE", vec!["x".into()]);
        let overrides = rt.env.child_overrides();
        assert_eq!(overrides, vec![("PATH".into(), "/bin:/usr/bin".into())]);
    }

    #[test]
    fn glob_star_and_question() {
        assert!(glob_match("*", "anything"));
        assert!(glob_match("a*", "abc"));
        assert!(!glob_match("a*", "bac"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "ac"));
        assert!(glob_match("*", ""));
        assert!(!glob_match("?", ""));
    }

    #[test]
    fn cmdsubst_words_trim_and_split() {
        assert_eq!(cmdsubst_words(b"one\ntwo\n\n"), vec!["one", "two"]);
        assert_eq!(cmdsubst_words(b"single"), vec!["single"]);
        assert_eq!(cmdsubst_words(b""), Vec::<String>::new());
        assert_eq!(cmdsubst_words(b"\n"), Vec::<String>::new());
    }
}
