# Plan: Completions milestone (manual completions + PATH/file completion)

Date: 2026-11-13
Branch: main
Base: d6e7445

## Goal

Tab completion in the REPL: command-name completion (builtins, functions,
PATH executables), and file/directory path completion for arguments.
Manual per-command completions via `complete` builtin (fish syntax) for
a first set of builtins. This is DESIGN.md step 5, "manual completions
first".

## Non-goals

- Command introspection (parsing `--help` output) — later milestone
- `complete -C` (dynamic command-line completion) — later milestone
- Windows-specific path separators in completion (backslash handling
  beyond the minimum: forward-slash paths must complete cleanly)
- Pager UI (multi-page menus); single-line candidate cycling is enough
  for this milestone

## Task 1: completion core (candidates + word extraction)

Files: `src/completion.rs` (new), `src/repl.rs` (wire Completer impl)

- `src/completion.rs`:
  - `pub struct CompletionEngine { builtins: Vec<String>, functions: Vec<String>, path_dirs: Vec<PathBuf> }`
    - builtins: the same BUILTIN_NAMES list as src/repl.rs (move the list
      to completion.rs or share it — implementer's choice, avoid two
      copies drifting: one canonical `pub const BUILTIN_NAMES` in
      completion.rs, repl.rs imports it)
    - functions: names from `Runtime::functions` keys (passed in at
      call time — engine does NOT own a Runtime)
    - path_dirs: parsed from $PATH at call time (passed in)
  - `pub fn complete_line(line: &str, pos: usize, engine-ish inputs) -> (usize, Vec<String>)`
    returns (start_index_of_word_being_completed, candidates)
  - Word extraction: scan backwards from pos over the current word
    (delimiters: space, and the operators `;|&()<>` — same boundaries
    the lexer uses for bare words; quoted words complete too when the
    cursor is inside a quoted string: strip the quote for matching,
    re-add on insert — if this proves hairy, complete only unquoted
    words and note the limitation in the report)
  - First word of the line (or first word after `;`, `|`, `and`, `or`,
    `not`, `begin`): command position → complete against
    builtins + functions + PATH executables (basename match; on
    candidates, PATH executables are listed by basename; when inserted
    they insert the basename, not the full path)
  - Other positions: file completion (see Task 2) UNLESS the command has
    manual completions (Task 3)
  - Matching is prefix + case-insensitive on Windows, prefix +
    case-sensitive on Unix (cfg(windows)); candidates sorted, deduped
  - Exports a pure-testable core: no I/O in matching logic; directory
    listing and PATH scan are injected (`fn list_dir(path) -> Vec<DirEntry-lite>`)
- `src/repl.rs`: implement `Completer::complete` for FushHelper by
  delegating to completion.rs. FushHelper gains the runtime snapshot
  inputs (builtins const, function names, $PATH values) — refreshed
  per completion call from the Runtime via a captured snapshot updated
  after each executed line (Rc<RefCell<...>> or a plain struct rebuilt
  after each `run_program`; implementer's choice, but the Runtime
  itself must not be reachable from the helper — borrow rules)
- Unit tests: word extraction boundaries (`ec`, `echo ec`, `echo ec|`,
  `echo "fi`, after `;`/`and`), prefix filtering, case sensitivity via
  the injectable cfg flag, dedup, sort, command-vs-argument position

## Task 2: file/directory completion

Files: `src/completion.rs`

- Argument position completes files+dirs relative to the typed path:
  empty word → contents of cwd; `src/` → entries under `src/`; `~/`
  expands $HOME for listing (inserted candidate keeps the `~/` prefix)
- Directories get a trailing `/` in the candidate; files none
- Hidden files (dot-prefixed) only match when the typed word already
  starts with `.`
- The injected dir-lister returns (name, is_dir); the engine handles
  filtering, sorting, trailing slash, quote re-attachment
- Unit tests with a fake lister: prefix filter, dir slash, hidden-file
  rule, `~` expansion, empty-word = cwd listing

## Task 3: `complete` builtin (manual per-command completions)

Files: `src/builtins.rs` (registration), `src/completion.rs` (storage + apply)

- `complete -c cmd [-e|-f] [-a CANDIDATES] [-d DESC]` (fish syntax):
  - `-c cmd`: the command the rule applies to
  - `-e`: erase all rules for cmd
  - `-f`: suppress default file completion for cmd
  - `-a cand`: space-separated candidate list (fish splits on spaces;
    quoted candidates in the -a value are NOT required for this
    milestone, but the value must be word-split on unquoted spaces)
  - `-d desc`: description, stored but only shown in a later pager
    milestone (accept + store)
  - Multiple `complete` calls for the same cmd accumulate (fish
    semantics)
- Rules live in the Runtime (persisted? NO — session-only for this
    milestone; fish persists them in completions files, we have no
    completion-file loading yet)
- At completion time: if the current command word has rules, the
  argument position completes against the union of -a candidates
  (prefix match); if `-f` was given, file completion is suppressed
  (empty candidates when no -a match)
- Builtins shipped: `complete -c set -a -l -x -U -e -q` style rule for
  `set` and `complete -c cd -f -a` with directory completion for
  `cd` (cd completes dirs only: suppress files) — ship these two as
  built-in rules registered at startup, plus `complete` itself usable
  from config.fush
- Unit tests: rule registration/accumulation, -e erase, -f suppression,
  -a word splitting, cd dirs-only, set option completion

## Task 4: Windows smoke + docs

- Cross-compile, copy to /mnt/c/Users/Francisco/fush-smoke/fush.exe
- Batch-file smoke: `fush.exe -c "complete -c probe -a one two; echo done"`
  (rule accepted, exit 0); interactive spot-check documented in
  ci/smoke-windows.md (completion is interactive — verify via a pty
  probe on Linux and document; on Windows verify the binary starts,
  `complete` parses, PATH scan works: `fush.exe -c "complete ..."`
  plus a scripted echo pipeline)
- ci/smoke-windows.md: Completions section
- README.md Status: tab completion (commands, files, manual `complete`
  rules)
- Full suite green, clippy clean

## Review Focus

- Word-boundary extraction must agree with the lexer's bare-word
  boundaries; test `echo ec|`, `a;ec`, `not ec`, `and ec`
- Command position after `|`, `;`, `and`, `or`, `not` (each is a new
  job start)
- Runtime borrow rules: helper must not hold a &mut Runtime across
  readline (Rc<RefCell> snapshot or rebuild-after-run; no re-entrant
  borrow panics)
- Hidden-file rule: only after explicit dot
- `complete` with no prior rules and no -a: pure registration, no error
- Windows case-insensitivity must not break Unix case-sensitivity (cfg
  flag, both tested)
