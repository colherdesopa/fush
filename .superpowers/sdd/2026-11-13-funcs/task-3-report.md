# Task 3 report: `abbr` builtin + interactive first-word expansion

## Scope and decisions

- Added a runtime-level abbreviation table: `Runtime::abbrs: HashMap<String, String>`.
- Implemented the `abbr` builtin in `src/builtins.rs` with the expected behaviors:
  - no args: list `name value...` lines sorted by key
  - `abbr NAME`: show one abbreviation
  - `abbr -a NAME VALUE...` / `abbr NAME VALUE...`: add or replace silently
  - `abbr -e NAME`: erase, returning status 1 if the name is missing
  - `abbr -l`: list names only, one per line
- Kept the REPL expansion limited to the interactive TTY path. The non-TTY loop (`run_non_tty_repl`) intentionally does not expand abbreviations.
- Used a pure helper `expand_abbr(table, line) -> String` in `src/repl.rs` to keep the expansion logic testable without requiring a fake TTY.
- Expansion is intentionally first-word-only and happens before parse, as required for the Fish-like interactive workflow.

## Design tradeoff

The plan calls for completion on Enter in the interactive REPL, not exact fish cursor-aware expansion after a space. We matched the simpler shell contract: the accepted line is expanded before parsing, and the expanded text is what is parsed and stored in history. This keeps the logic predictable while still providing the interactive abbreviation behavior.

## Tests added

- `abbr_adds_lists_and_overwrites`
- `abbr_list_names_only`
- `abbr_erase_removes_and_statuses_missing`
- `expand_abbr_replaces_only_the_first_word`

## Verification

- `cargo test --quiet`
- `cargo clippy --all-targets --all-features -- -D warnings`

## Concerns

- Abbreviation expansion is intentionally interactive-only and not persisted across sessions; this matches the task scope and leaves persistence to a later milestone.
- The expansion is a whole-line replacement on Enter rather than fish's per-cursor expansion after a word boundary; this is the deliberate simplification called for by the design.
