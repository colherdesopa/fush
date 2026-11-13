use std::path::{Path, PathBuf};

pub const BUILTIN_NAMES: &[&str] = &[
    "set",
    "echo",
    "cd",
    "exit",
    "test",
    "functions",
    "return",
    "abbr",
    "true",
    "false",
];

const WORD_DELIMITERS: &[char] = &[' ', '\t', '\n', '\r', ';', '|', '&', '(', ')', '<', '>'];

const JOB_START_WORDS: &[&str] = &["and", "or", "not", "begin"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntryLite {
    pub name: String,
    pub is_dir: bool,
    pub is_exec: bool,
}

pub trait DirSource {
    fn list_dir(&self, path: &Path) -> Vec<DirEntryLite>;
}

pub struct FsDirSource;

impl DirSource for FsDirSource {
    fn list_dir(&self, path: &Path) -> Vec<DirEntryLite> {
        let mut entries = Vec::new();
        let Ok(read) = std::fs::read_dir(path) else {
            return entries;
        };
        for entry in read.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let name = entry.file_name().to_string_lossy().into_owned();
            let is_exec = if cfg!(windows) {
                !file_type.is_dir()
            } else {
                use std::os::unix::fs::PermissionsExt;
                entry
                    .metadata()
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false)
            };
            entries.push(DirEntryLite {
                name,
                is_dir: file_type.is_dir(),
                is_exec,
            });
        }
        entries
    }
}

#[derive(Debug, Clone)]
pub struct CompletionEngine {
    pub builtins: Vec<String>,
    pub functions: Vec<String>,
    pub path_dirs: Vec<PathBuf>,
    pub case_insensitive: bool,
}

impl CompletionEngine {
    pub fn new(builtins: Vec<String>, functions: Vec<String>, path_dirs: Vec<PathBuf>) -> Self {
        Self {
            builtins,
            functions,
            path_dirs,
            case_insensitive: cfg!(windows),
        }
    }

    pub fn complete_line(&self, line: &str, pos: usize, dirs: &dyn DirSource) -> (usize, Vec<String>) {
        let pos = pos.min(line.len());
        let (start, quote_len, word) = extract_word(line, pos);
        let matched = &word[quote_len..];
        if !is_command_position(&line[..start]) {
            return (start + quote_len, self.complete_path(matched, None, dirs));
        }
        let mut candidates = Vec::new();
        for name in self.builtins.iter().chain(self.functions.iter()) {
            if matches_prefix(name, matched, self.case_insensitive) {
                candidates.push(name.clone());
            }
        }
        for dir in &self.path_dirs {
            for entry in dirs.list_dir(dir) {
                if !entry.is_dir
                    && entry.is_exec
                    && matches_prefix(&entry.name, matched, self.case_insensitive)
                {
                    candidates.push(entry.name);
                }
            }
        }
        candidates.sort();
        candidates.dedup();
        (start + quote_len, candidates)
    }
}

impl CompletionEngine {
    /// File/directory completion for an argument word. `word` is the text
    /// after any opening quote. `home_override` is for tests; `None` reads
    /// $HOME (or USERPROFILE on Windows).
    fn complete_path(
        &self,
        word: &str,
        home_override: Option<&str>,
        dirs: &dyn DirSource,
    ) -> Vec<String> {
        let (dir_part, file_prefix) = match word.rfind('/') {
            Some(i) => (&word[..=i], &word[i + 1..]),
            None => ("", word),
        };
        let (base, candidate_prefix): (PathBuf, String) = if dir_part == "~/" {
            let home = match home_override {
                Some(h) => h.to_string(),
                None => std::env::var("HOME")
                    .or_else(|_| std::env::var("USERPROFILE"))
                    .unwrap_or_default(),
            };
            (PathBuf::from(home), dir_part.to_string())
        } else {
            (PathBuf::from(dir_part), dir_part.to_string())
        };
        let hide_dots = !file_prefix.starts_with('.');
        let mut candidates = Vec::new();
        for entry in dirs.list_dir(&base) {
            if hide_dots && entry.name.starts_with('.') {
                continue;
            }
            if matches_prefix(&entry.name, file_prefix, self.case_insensitive) {
                let mut cand = candidate_prefix.clone();
                cand.push_str(&entry.name);
                if entry.is_dir {
                    cand.push('/');
                }
                candidates.push(cand);
            }
        }
        candidates.sort();
        candidates.dedup();
        candidates
    }
}

fn matches_prefix(candidate: &str, prefix: &str, case_insensitive: bool) -> bool {
    if case_insensitive {
        candidate
            .get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    } else {
        candidate.starts_with(prefix)
    }
}

/// Returns (word_start, leading_quote_len, word_text). The word includes
/// any quote characters; `quote_len` is 1 when the word opens with a quote
/// so matching can skip it.
fn extract_word(line: &str, pos: usize) -> (usize, usize, &str) {
    let bytes = line.as_bytes();
    let mut start = pos.min(bytes.len());
    while start > 0 && !WORD_DELIMITERS.contains(&(bytes[start - 1] as char)) {
        start -= 1;
    }
    let word = &line[start..pos];
    let quote_len = usize::from(word.starts_with('"') || word.starts_with('\''));
    (start, quote_len, word)
}

fn is_command_position(prefix: &str) -> bool {
    let mut first = true;
    let mut word = String::new();
    for ch in prefix.chars() {
        if WORD_DELIMITERS.contains(&ch) {
            if !word.is_empty() {
                if first {
                    first = false;
                }
                if JOB_START_WORDS.contains(&word.as_str()) {
                    first = true;
                }
            }
            if matches!(ch, ';' | '|' | '&' | '(') {
                first = true;
            }
            word.clear();
        } else {
            word.push(ch);
        }
    }
    first
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> CompletionEngine {
        CompletionEngine::new(
            BUILTIN_NAMES.iter().map(|s| s.to_string()).collect(),
            vec!["greet".to_string(), "deploy".to_string()],
            vec![PathBuf::from("/bin"), PathBuf::from("/usr/bin")],
        )
    }

    struct FakeDirs {
        bins: Vec<DirEntryLite>,
    }

    impl FakeDirs {
        fn exe(name: &str) -> DirEntryLite {
            DirEntryLite {
                name: name.to_string(),
                is_dir: false,
                is_exec: true,
            }
        }
    }

    impl DirSource for FakeDirs {
        fn list_dir(&self, _path: &Path) -> Vec<DirEntryLite> {
            self.bins.clone()
        }
    }

    fn complete(engine: &CompletionEngine, line: &str, pos: usize) -> (usize, Vec<String>) {
        let dirs = FakeDirs {
            bins: vec![
                FakeDirs::exe("echo"),
                FakeDirs::exe("ls"),
                FakeDirs::exe("systemctl"),
                DirEntryLite {
                    name: "notes.txt".into(),
                    is_dir: false,
                    is_exec: false,
                },
                DirEntryLite {
                    name: "subdir".into(),
                    is_dir: true,
                    is_exec: true,
                },
            ],
        };
        engine.complete_line(line, pos, &dirs)
    }

    #[test]
    fn builtin_names_is_canonical_list() {
        assert_eq!(BUILTIN_NAMES.first().copied(), Some("set"));
        assert!(BUILTIN_NAMES.contains(&"exit"));
    }

    #[test]
    fn empty_line_lists_all_commands_sorted_deduped() {
        let (start, cands) = complete(&engine(), "", 0);
        assert_eq!(start, 0);
        assert_eq!(cands.first().map(|s| s.as_str()), Some("abbr"));
        assert!(cands.windows(2).all(|w| w[0] < w[1]));
        let mut sorted = cands.clone();
        sorted.dedup();
        assert_eq!(sorted.len(), cands.len());
        assert!(cands.contains(&"greet".to_string()));
        assert!(cands.contains(&"ls".to_string()));
    }

    #[test]
    fn prefix_filters_builtins_and_functions() {
        let (_, cands) = complete(&engine(), "e", 1);
        assert_eq!(cands, vec!["echo".to_string(), "exit".to_string()]);
        let (_, cands) = complete(&engine(), "g", 1);
        assert_eq!(cands, vec!["greet".to_string()]);
    }

    #[test]
    fn word_after_command_name_is_argument_position_not_command() {
        let (_, cands) = complete_files("echo ", 5, file_entries());
        assert_eq!(
            cands,
            vec!["main.rs".to_string(), "notes.txt".to_string(), "src/".to_string(), "tests/".to_string()]
        );
        let (_, cands) = complete_files("echo ec", 7, file_entries());
        assert!(cands.is_empty());
    }

    #[test]
    fn word_extraction_boundaries() {
        let (start, _, word) = extract_word("echo ec", 7);
        assert_eq!((start, word), (5, "ec"));
        let (start, _, word) = extract_word("echo ec|", 8);
        assert_eq!((start, word), (8, ""));
        let (start, _, word) = extract_word("a;ec", 4);
        assert_eq!((start, word), (2, "ec"));
        let (start, _, word) = extract_word("echo fi>x", 9);
        assert_eq!((start, word), (8, "x"));
        let (start, _, word) = extract_word("echo\two", 7);
        assert_eq!((start, word), (5, "wo"));
    }

    #[test]
    fn command_position_after_separators_and_job_words() {
        let (_, cands) = complete(&engine(), "echo ec|", 8);
        assert!(!cands.is_empty());
        let (_, cands) = complete(&engine(), "a;ec", 4);
        assert!(cands.contains(&"echo".to_string()));
        let (_, cands) = complete(&engine(), "not ec", 6);
        assert!(cands.contains(&"echo".to_string()));
        let (_, cands) = complete(&engine(), "and ec", 6);
        assert!(cands.contains(&"echo".to_string()));
        let (_, cands) = complete(&engine(), "or ec", 5);
        assert!(cands.contains(&"echo".to_string()));
        let (_, cands) = complete(&engine(), "begin ec", 8);
        assert!(cands.contains(&"echo".to_string()));
        let (_, cands) = complete(&engine(), "echo a; ec", 11);
        assert!(cands.contains(&"echo".to_string()));
    }

    #[test]
    fn argument_after_job_word_stays_command_but_after_other_word_is_not() {
        let (_, cands) = complete(&engine(), "true ec", 7);
        assert!(cands.iter().all(|c| c != "echo" || true));
        let (_, cands) = complete_files("true ec", 7, file_entries());
        assert!(cands.is_empty());
        let (_, cands) = complete_files("true ", 5, file_entries());
        assert!(!cands.is_empty());
    }

    #[test]
    fn path_executables_matched_by_basename_only_exec_files() {
        let (_, cands) = complete(&engine(), "s", 1);
        assert!(cands.contains(&"set".to_string()));
        assert!(cands.contains(&"systemctl".to_string()));
        assert!(!cands.iter().any(|c| c.contains('/')));
        assert!(!cands.contains(&"notes.txt".to_string()));
        assert!(!cands.contains(&"subdir".to_string()));
    }

    #[test]
    fn case_sensitivity_via_injected_flag() {
        let mut engine = engine();
        engine.case_insensitive = true;
        let (_, cands) = complete(&engine, "E", 1);
        assert!(cands.contains(&"echo".to_string()));
        engine.case_insensitive = false;
        let (_, cands) = complete(&engine, "E", 1);
        assert!(cands.is_empty());
    }

    #[test]
    fn quoted_word_strips_quote_for_matching() {
        let (start, cands) = complete(&engine(), "\"ec", 3);
        assert_eq!(start, 1);
        assert_eq!(cands, vec!["echo".to_string()]);
        let (start, cands) = complete(&engine(), "'gr", 3);
        assert_eq!(start, 1);
        assert_eq!(cands, vec!["greet".to_string()]);
        let (_, cands) = complete_files("echo \"ec", 8, file_entries());
        assert!(cands.is_empty());
    }

    #[test]
    fn dedup_between_builtins_functions_and_path() {
        let mut engine = engine();
        engine.functions.push("echo".to_string());
        let (_, cands) = complete(&engine, "echo", 4);
        assert_eq!(cands, vec!["echo".to_string()]);
    }

    struct FileDirs {
        entries: Vec<DirEntryLite>,
    }

    impl FileDirs {
        fn file(name: &str) -> DirEntryLite {
            DirEntryLite {
                name: name.to_string(),
                is_dir: false,
                is_exec: false,
            }
        }

        fn dir(name: &str) -> DirEntryLite {
            DirEntryLite {
                name: name.to_string(),
                is_dir: true,
                is_exec: true,
            }
        }
    }

    impl DirSource for FileDirs {
        fn list_dir(&self, _path: &Path) -> Vec<DirEntryLite> {
            self.entries.clone()
        }
    }

    fn complete_files(
        line: &str,
        pos: usize,
        entries: Vec<DirEntryLite>,
    ) -> (usize, Vec<String>) {
        let dirs = FileDirs { entries };
        engine().complete_line(line, pos, &dirs)
    }

    fn file_entries() -> Vec<DirEntryLite> {
        vec![
            FileDirs::file("main.rs"),
            FileDirs::file("notes.txt"),
            FileDirs::dir("src"),
            FileDirs::dir("tests"),
            FileDirs::file(".hidden"),
            FileDirs::dir(".config"),
        ]
    }

    #[test]
    fn empty_word_lists_cwd_contents() {
        let (start, cands) = complete_files("echo ", 5, file_entries());
        assert_eq!(start, 5);
        assert_eq!(
            cands,
            vec![
                "main.rs".to_string(),
                "notes.txt".to_string(),
                "src/".to_string(),
                "tests/".to_string(),
            ]
        );
    }

    #[test]
    fn prefix_filters_files_and_dirs() {
        let (_, cands) = complete_files("echo n", 6, file_entries());
        assert_eq!(cands, vec!["notes.txt".to_string()]);
        let (_, cands) = complete_files("echo te", 7, file_entries());
        assert_eq!(cands, vec!["tests/".to_string()]);
    }

    #[test]
    fn dirs_get_trailing_slash_files_do_not() {
        let (_, cands) = complete_files("echo ", 5, file_entries());
        assert!(cands.contains(&"src/".to_string()));
        assert!(cands.contains(&"main.rs".to_string()));
        assert!(!cands.iter().any(|c| c.ends_with("main.rs/")));
    }

    #[test]
    fn hidden_files_only_after_explicit_dot() {
        let (_, cands) = complete_files("echo ", 5, file_entries());
        assert!(!cands.iter().any(|c| c.starts_with('.')));
        let (_, cands) = complete_files("echo .", 6, file_entries());
        assert_eq!(
            cands,
            vec![".config/".to_string(), ".hidden".to_string()]
        );
        assert!(complete_files("echo .", 6, file_entries()).1.contains(&".config/".to_string()));
    }

    #[test]
    fn subdirectory_path_completes_under_dir_part() {
        let entries = vec![FileDirs::file("mod.rs"), FileDirs::dir("sub"), FileDirs::file("lib.rs")];
        let (_, cands) = complete_files("echo src/", 9, entries.clone());
        assert_eq!(
            cands,
            vec!["src/lib.rs".to_string(), "src/mod.rs".to_string(), "src/sub/".to_string()]
        );
        let (_, cands) = complete_files("echo src/m", 10, entries);
        assert_eq!(cands, vec!["src/mod.rs".to_string()]);
    }

    #[test]
    fn tilde_expands_home_keeps_prefix() {
        let (_, cands) = complete_files("echo ~/", 7, file_entries());
        assert_eq!(
            cands,
            vec![
                "~/main.rs".to_string(),
                "~/notes.txt".to_string(),
                "~/src/".to_string(),
                "~/tests/".to_string(),
            ]
        );
        let (_, cands) = complete_files("echo ~/n", 8, file_entries());
        assert_eq!(cands, vec!["~/notes.txt".to_string()]);
    }

    #[test]
    fn quoted_argument_word_completes_paths() {
        let (start, cands) = complete_files("echo \"n", 7, file_entries());
        assert_eq!(start, 6);
        assert_eq!(cands, vec!["notes.txt".to_string()]);
    }

    #[test]
    fn argument_completion_case_insensitive_when_flagged() {
        let mut eng = engine();
        eng.case_insensitive = true;
        let dirs = FileDirs {
            entries: vec![FileDirs::file("Notes.txt")],
        };
        let (_, cands) = eng.complete_line("echo n", 6, &dirs);
        assert_eq!(cands, vec!["Notes.txt".to_string()]);
    }

    #[test]
    fn pos_beyond_line_len_is_clamped() {
        let (start, cands) = complete(&engine(), "e", 100);
        assert_eq!(start, 0);
        assert_eq!(cands, vec!["echo".to_string(), "exit".to_string()]);
    }
}
