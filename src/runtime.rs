use crate::ast::ExprKind::QuotedCmdSubst;
use crate::ast::{Expr, ExprKind, IndexSpec, Stmt};
use crate::completion::CompletionRule;
use crate::exec;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::Path;
use std::rc::Rc;

#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
}

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

    pub fn remove(&mut self, name: &str) {
        self.globals.remove(name);
        for scope in self.locals.iter_mut() {
            scope.remove(name);
        }
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

    pub fn child_overrides(&self) -> Vec<(String, String)> {
        self.globals
            .iter()
            .filter(|(name, _)| self.is_exported(name))
            .map(|(name, vals)| (name.clone(), vals.join(":")))
            .collect()
    }
}

/// Allow-list state collected from a profile file (`--profile NAME`).
/// Empty lists allow nothing: fail closed.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Profile {
    pub allow: BTreeSet<String>,
    pub allow_write: BTreeSet<String>,
}

pub struct Runtime {
    pub env: Env,
    pub status: i32,
    pub exited: Option<i32>,
    pub(crate) capture: Option<Vec<u8>>,
    pub functions: HashMap<String, Rc<FunctionDef>>,
    pub abbrs: HashMap<String, String>,
    pub function_depth: usize,
    pub function_return_stack: Vec<Option<i32>>,
    /// Manual per-command completion rules (`complete -c cmd ...`), session-only.
    pub completions: HashMap<String, Vec<CompletionRule>>,
    /// Active profile; None = no `--profile` flag, everything allowed.
    pub profile: Option<Profile>,
    /// True only while the profile file (or the read-only virtual load) is
    /// being sourced; gates the `profile` builtin.
    pub profile_loading: bool,
}

fn path_components(path: &Path) -> Vec<String> {
    let s = strip_verbatim_prefix(&path.as_os_str().to_string_lossy());
    Path::new(&s)
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect()
}

fn strip_verbatim_prefix(s: &str) -> String {
    if let Some(rest) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{rest}")
    } else {
        s.strip_prefix(r"\\?\")
            .map(String::from)
            .unwrap_or_else(|| s.to_string())
    }
}

impl Runtime {
    /// Whether the external command `cmd` may run. No profile = allowed;
    /// empty allow-list = nothing allowed.
    pub fn profile_allows(&self, cmd: &str) -> bool {
        match &self.profile {
            None => true,
            Some(p) => p.allow.contains(cmd),
        }
    }

    /// Whether writing to `path` is allowed: component-wise prefix match
    /// against the allow-write list, absolute paths only. Relative paths
    /// are rejected; on Windows components compare case-insensitively.
    pub fn profile_allows_write(&self, path: &Path) -> bool {
        let Some(profile) = &self.profile else {
            return true;
        };
        if !path.is_absolute() {
            return false;
        }
        let target = path_components(path);
        let windows = cfg!(windows);
        let eq = |a: &str, b: &str| {
            if windows {
                a.eq_ignore_ascii_case(b)
            } else {
                a == b
            }
        };
        profile.allow_write.iter().any(|prefix| {
            let prefix = Path::new(prefix.as_str());
            if !prefix.is_absolute() {
                return false;
            }
            let prefix = path_components(prefix);
            prefix.len() <= target.len() && prefix.iter().zip(target.iter()).all(|(p, t)| eq(p, t))
        })
    }

    pub fn new() -> Runtime {
        let mut rt = Runtime {
            env: Env::default(),
            status: 0,
            exited: None,
            capture: None,
            functions: HashMap::new(),
            abbrs: HashMap::new(),
            function_depth: 0,
            function_return_stack: Vec::new(),
            completions: HashMap::new(),
            profile: None,
            profile_loading: false,
        };
        rt.register_default_completions();
        rt
    }

    /// Built-in completion rules shipped at startup: `set` completes its
    /// option flags, `cd` completes directories only.
    fn register_default_completions(&mut self) {
        self.completions.insert(
            "set".to_string(),
            vec![CompletionRule {
                candidates: vec![
                    "-l".into(),
                    "-x".into(),
                    "-U".into(),
                    "-e".into(),
                    "-q".into(),
                ],
                ..Default::default()
            }],
        );
        self.completions.insert(
            "cd".to_string(),
            vec![CompletionRule {
                candidates: Vec::new(),
                suppress_files: true,
                dirs_only: true,
                description: None,
            }],
        );
    }

    pub fn register_function(&mut self, name: &str, params: Vec<String>, body: Vec<Stmt>) {
        self.functions
            .insert(name.to_string(), Rc::new(FunctionDef { params, body }));
    }

    fn indexed_values(&self, name: &str, index: &IndexSpec) -> Vec<String> {
        let values = self.env.get(name).unwrap_or_default();
        self.indexed_from(values, index)
    }

    fn indexed_from(&self, values: Vec<String>, index: &IndexSpec) -> Vec<String> {
        let values = &values;
        if values.is_empty() {
            return Vec::new();
        }
        match index {
            IndexSpec::At(i) => {
                let idx = match *i {
                    v if v > 0 => v - 1,
                    v if v < 0 => (values.len() as isize) + v,
                    _ => return Vec::new(),
                };
                if idx < 0 || idx as usize >= values.len() {
                    Vec::new()
                } else {
                    vec![values[idx as usize].clone()]
                }
            }
            IndexSpec::Range { start, end } => {
                let len = values.len() as isize;
                let start_idx = match start {
                    Some(v) => match range_bound(*v, len) {
                        Some(i) => i,
                        None => return Vec::new(),
                    },
                    None => 0,
                };
                let end_idx = match end {
                    Some(v) => match range_bound(*v, len) {
                        Some(i) => i,
                        None => return Vec::new(),
                    },
                    None => len - 1,
                };
                if start_idx < 0 || end_idx < 0 || start_idx > end_idx {
                    Vec::new()
                } else {
                    values[start_idx as usize..=end_idx as usize].to_vec()
                }
            }
        }
    }

    pub fn expand(&mut self, e: &Expr) -> Result<Vec<String>, String> {
        match e.kind {
            ExprKind::Literal => Ok(vec![e.text.clone()]),
            ExprKind::Var => Ok(self.env.get(&e.text).unwrap_or_default()),
            ExprKind::Index => {
                Ok(self.indexed_values(&e.text, e.index.as_ref().expect("index expr has index")))
            }
            ExprKind::CmdSubst => {
                let out = self.capture_stmts(&e.stmts)?;
                Ok(cmdsubst_words(&out))
            }
            QuotedCmdSubst => {
                let mut out = self.capture_stmts(&e.stmts)?;
                if out.last() == Some(&b'\n') {
                    out.pop();
                }
                Ok(vec![String::from_utf8_lossy(&out).into_owned()])
            }
            ExprKind::Concat => {
                let mut words = vec![String::new()];
                for part in &e.parts {
                    let part_words = match part.kind {
                        ExprKind::Literal => vec![part.text.clone()],
                        ExprKind::Var => self.env.get(&part.text).unwrap_or_default(),
                        ExprKind::Index => self.indexed_values(
                            &part.text,
                            part.index.as_ref().expect("index expr has index"),
                        ),
                        ExprKind::CmdSubst => {
                            let out = self.capture_stmts(&part.stmts)?;
                            cmdsubst_words(&out)
                        }
                        QuotedCmdSubst => {
                            let mut out = self.capture_stmts(&part.stmts)?;
                            if out.last() == Some(&b'\n') {
                                out.pop();
                            }
                            vec![String::from_utf8_lossy(&out).into_owned()]
                        }
                        ExprKind::Concat => self.expand(part)?,
                    };

                    if e.quoted {
                        let joined = part_words.join(" ");
                        for word in &mut words {
                            word.push_str(&joined);
                        }
                    } else {
                        if part_words.is_empty() {
                            return Ok(Vec::new());
                        }
                        let mut next = Vec::new();
                        for value in &part_words {
                            for prefix in &words {
                                next.push(format!("{prefix}{value}"));
                            }
                        }
                        words = next;
                    }
                }
                Ok(words)
            }
        }
    }

    pub fn expand_one(&mut self, e: &Expr) -> Result<String, String> {
        let words = self.expand(e)?;
        match words.len() {
            1 => Ok(words.into_iter().next().expect("checked len")),
            0 => Ok(String::new()),
            _ => Err(format!(
                "{}: expanded to {} words, expected one",
                e.pos,
                words.len()
            )),
        }
    }

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

pub(crate) fn cmdsubst_words(out: &[u8]) -> Vec<String> {
    let s = String::from_utf8_lossy(out);
    let trimmed = s.trim_end_matches('\n');
    if trimmed.is_empty() {
        Vec::new()
    } else {
        trimmed.split('\n').map(str::to_string).collect()
    }
}

pub fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern_chars: Vec<char> = pattern.chars().collect();
    let text_chars: Vec<char> = text.chars().collect();
    glob_inner(&pattern_chars, &text_chars)
}

/// Normalizes a fish-style 1-based range bound to a 0-based index.
/// Positive: v-1; negative: len+v; zero or out of range: None (empty).
fn range_bound(v: isize, len: isize) -> Option<isize> {
    if v > 0 {
        if v - 1 < len { Some(v - 1) } else { None }
    } else if v < 0 {
        let i = len + v;
        if i >= 0 { Some(i) } else { None }
    } else {
        None
    }
}

fn glob_inner(p: &[char], t: &[char]) -> bool {
    if p.is_empty() {
        return t.is_empty();
    }
    match p[0] {
        '*' => {
            for i in 0..=t.len() {
                if glob_inner(&p[1..], &t[i..]) {
                    return true;
                }
            }
            false
        }
        '?' => !t.is_empty() && glob_inner(&p[1..], &t[1..]),
        c => !t.is_empty() && t[0] == c && glob_inner(&p[1..], &t[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rt_with(vars: &[(&str, &[&str])]) -> Runtime {
        let mut rt = Runtime::new();
        for (name, vals) in vars {
            rt.env
                .set_global(name, vals.iter().map(|s| s.to_string()).collect());
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
    fn bare_concat_with_multi_word_var_expands_cartesian() {
        let mut rt = rt_with(&[("x", &["a", "b"])]);
        let e = Expr::concat(
            Default::default(),
            vec![
                Expr::literal(Default::default(), "pre-".into()),
                Expr::var(Default::default(), "x".into()),
                Expr::literal(Default::default(), "-post".into()),
            ],
        );
        assert_eq!(rt.expand(&e).unwrap(), vec!["pre-a-post", "pre-b-post"]);
    }

    #[test]
    fn index_and_slice_expand_as_list_selection() {
        let mut rt = rt_with(&[("x", &["a", "b", "c"])]);
        assert_eq!(
            rt.expand(&Expr::index(
                Default::default(),
                "x".into(),
                IndexSpec::At(2)
            ))
            .unwrap(),
            vec!["b"]
        );
        assert_eq!(
            rt.expand(&Expr::index(
                Default::default(),
                "x".into(),
                IndexSpec::At(-1)
            ))
            .unwrap(),
            vec!["c"]
        );
        assert_eq!(
            rt.expand(&Expr::index(
                Default::default(),
                "x".into(),
                IndexSpec::Range {
                    start: Some(1),
                    end: Some(2)
                }
            ))
            .unwrap(),
            vec!["a", "b"]
        );
        assert_eq!(
            rt.expand(&Expr::index(
                Default::default(),
                "x".into(),
                IndexSpec::Range {
                    start: None,
                    end: Some(2)
                }
            ))
            .unwrap(),
            vec!["a", "b"]
        );
        assert_eq!(
            rt.expand(&Expr::index(
                Default::default(),
                "x".into(),
                IndexSpec::Range {
                    start: Some(2),
                    end: None
                }
            ))
            .unwrap(),
            vec!["b", "c"]
        );
        assert_eq!(
            rt.expand(&Expr::index(
                Default::default(),
                "x".into(),
                IndexSpec::At(9)
            ))
            .unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            rt.expand(&Expr::index(
                Default::default(),
                "missing".into(),
                IndexSpec::At(1)
            ))
            .unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            rt.expand(&Expr::index(
                Default::default(),
                "x".into(),
                IndexSpec::Range {
                    start: None,
                    end: Some(0)
                }
            ))
            .unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            rt.expand(&Expr::index(
                Default::default(),
                "x".into(),
                IndexSpec::Range {
                    start: Some(0),
                    end: None
                }
            ))
            .unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            rt.expand(&Expr::index(
                Default::default(),
                "x".into(),
                IndexSpec::Range {
                    start: Some(-9),
                    end: Some(2)
                }
            ))
            .unwrap(),
            Vec::<String>::new()
        );
        assert_eq!(
            rt.expand(&Expr::index(
                Default::default(),
                "x".into(),
                IndexSpec::Range {
                    start: Some(-2),
                    end: Some(-1)
                }
            ))
            .unwrap(),
            vec!["b", "c"]
        );
        assert_eq!(
            rt.expand(&Expr::index(
                Default::default(),
                "x".into(),
                IndexSpec::Range {
                    start: Some(9),
                    end: Some(2)
                }
            ))
            .unwrap(),
            Vec::<String>::new()
        );
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
        rt.env
            .set_global("PATH", vec!["/bin".to_string(), "/usr/bin".to_string()]);
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
    fn glob_multibyte_matches_char_wise() {
        assert!(glob_match("*é*", "café"));
        assert!(glob_match("caf?", "café"));
        assert!(glob_match("?", "é"));
        assert!(!glob_match("??", "é"));
        assert!(glob_match("a?c", "aéc"));
        assert!(!glob_match("a?c", "ac"));
    }

    #[test]
    fn cmdsubst_words_trim_and_split() {
        assert_eq!(cmdsubst_words(b"one\ntwo\n\n"), vec!["one", "two"]);
        assert_eq!(cmdsubst_words(b"single"), vec!["single"]);
        assert_eq!(cmdsubst_words(b""), Vec::<String>::new());
        assert_eq!(cmdsubst_words(b"\n"), Vec::<String>::new());
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;

    fn rt_with_profile(allow: &[&str], allow_write: &[&str]) -> Runtime {
        let mut rt = Runtime::new();
        let mut p = Profile::default();
        for a in allow {
            p.allow.insert(a.to_string());
        }
        for w in allow_write {
            p.allow_write.insert(w.to_string());
        }
        rt.profile = Some(p);
        rt
    }

    #[test]
    fn no_profile_allows_everything() {
        let rt = Runtime::new();
        assert!(rt.profile_allows("git"));
        assert!(rt.profile_allows_write(std::path::Path::new("/etc/passwd")));
    }

    #[test]
    fn empty_profile_allows_nothing() {
        let rt = rt_with_profile(&[], &[]);
        assert!(!rt.profile_allows("git"));
        assert!(!rt.profile_allows_write(std::path::Path::new("/tmp/x")));
    }

    #[test]
    fn allow_list_exact_match() {
        let rt = rt_with_profile(&["git", "ls"], &[]);
        assert!(rt.profile_allows("git"));
        assert!(!rt.profile_allows("gitk"));
        assert!(!rt.profile_allows("gi"));
    }

    #[test]
    fn write_prefix_is_component_wise() {
        let root = if cfg!(windows) { "C:\\work" } else { "/work" };
        let rt = rt_with_profile(&[], &[root]);
        assert!(rt.profile_allows_write(std::path::Path::new(&format!("{root}/a.txt"))));
        assert!(rt.profile_allows_write(std::path::Path::new(root)));
        assert!(!rt.profile_allows_write(std::path::Path::new(&format!("{root}-evil/a.txt"))));
        assert!(!rt.profile_allows_write(std::path::Path::new(&format!(
            "{}/wo",
            if cfg!(windows) { "C:\\" } else { "/" }
        ))));
    }

    #[test]
    fn write_relative_paths_rejected() {
        let rt = rt_with_profile(&[], &["/work"]);
        assert!(!rt.profile_allows_write(std::path::Path::new("work/a.txt")));
        assert!(!rt.profile_allows_write(std::path::Path::new("a.txt")));
    }

    #[test]
    fn write_relative_prefix_rejected() {
        let rt = rt_with_profile(&[], &["work"]);
        assert!(!rt.profile_allows_write(std::path::Path::new("/work/a.txt")));
    }

    #[cfg(windows)]
    #[test]
    fn write_case_insensitive_on_windows() {
        let rt = rt_with_profile(&[], &["C:\\Work"]);
        assert!(rt.profile_allows_write(std::path::Path::new("C:\\work\\a.txt")));
        assert!(rt.profile_allows_write(std::path::Path::new("C:\\WORK\\sub\\b.txt")));
        assert!(!rt.profile_allows_write(std::path::Path::new("C:\\work-evil\\a.txt")));
    }

    #[test]
    fn strip_verbatim_prefix_removes_drive_form() {
        assert_eq!(
            strip_verbatim_prefix(r"\\?\C:\Work\a.txt"),
            r"C:\Work\a.txt"
        );
        assert_eq!(strip_verbatim_prefix(r"C:\Work\a.txt"), r"C:\Work\a.txt");
    }

    #[test]
    fn strip_verbatim_prefix_restores_unc_form() {
        assert_eq!(
            strip_verbatim_prefix(r"\\?\UNC\server\share\dir"),
            r"\\server\share\dir"
        );
    }

    #[cfg(not(windows))]
    #[test]
    fn write_case_sensitive_on_unix() {
        let rt = rt_with_profile(&[], &["/Work"]);
        assert!(!rt.profile_allows_write(std::path::Path::new("/work/a.txt")));
        assert!(rt.profile_allows_write(std::path::Path::new("/Work/a.txt")));
    }
}
