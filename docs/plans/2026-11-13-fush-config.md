# config.fush First-Run Import Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `config.fush` the single primary config: on first interactive startup, offer to import an existing `config.fish`; from then on only `config.fush` is ever sourced.

**Architecture:** `src/config.rs` grows the primary-path resolution (`config.fush`), a first-run check (does `config.fush` exist? does `config.fish`?), and an import routine (copy file contents). Startup hooks change from `source_config` (always config.fish) to `source_primary_config`: interactive TTY runs the first-run import prompt; `-c`/script/non-TTY never prompt and source `config.fush` only. No marker files — `config.fush` existence IS the "question answered" state.

**Tech Stack:** Rust 2024, std only (no new deps).

**Spec:** this document + user decisions in session: primary = `%USERPROFILE%\.config\fush\config.fush`; `config.fish` is only ever read by the one-time import; import = copy content (same syntax); decline = create empty `config.fush` anyway (never nag again, fish config NOT sourced); non-interactive never prompts.

## Global Constraints

- Rust 2024 edition, std only, no new dependencies, no `unsafe`
- No explanatory `//` comments (doc `///` and `//!` comments allowed)
- DCO-signed commits (`git commit -s`), no AI attribution trailers
- `cargo test`, `cargo clippy --all-targets -- -D warnings` green after every task
- All new path logic injectable for tests (env-var or parameter injection, following the `FUSH_CONFIG` / `config_dir_for` pattern)
- Interactive prompt only on a TTY REPL startup; `-c`, script files, `--ast`, and non-TTY stdin never prompt and never block

## Review Focus

- `config.fish` exists but user declines → `config.fush` created empty, fish config NOT sourced, no re-prompt on next launch
- `config.fush` already exists → no prompt ever, fish config ignored even if present
- Import must not lose data on failure: if copy fails (permissions, disk), leave state unchanged, print error, continue without config
- Non-interactive paths (`-c`, script, piped stdin) with no `config.fush` yet → no prompt, no config, exit status unaffected
- `config.fush` exists AND `FUSH_CONFIG` env override set → env override wins (test-injection path)

---

### Task 1: Primary config path + source_primary_config

**Files:**
- Modify: `src/config.rs`
- Test: `src/config.rs` (unit tests in existing `mod tests`)

**Interfaces:**
- Consumes: existing `config_dir_for`, `config_file`, `source_config_path`
- Produces:
  - `pub fn primary_config_path() -> Option<PathBuf>` — `<config_dir>/config.fush` (no env override)
  - `pub fn primary_config_file() -> Option<PathBuf>` — `FUSH_CONFIG` env override if set, else `primary_config_path()`
  - `pub fn source_primary_config(rt: &mut Runtime)` — sources `primary_config_file()` if it exists; missing file is silent (same error semantics as `source_config`)

- [ ] **Step 1: Write failing unit tests**

```rust
#[test]
fn primary_config_path_is_config_fush() {
    let dir = config_dir_for(Some("/h"), None, false).unwrap();
    let path = primary_config_path_for(Some("/h"), None, false).unwrap();
    assert_eq!(path, dir.join("config.fush"));
}
```

Add `primary_config_path_for(home, userprofile, windows)` mirroring `config_dir_for` composition, plus a test that `primary_config_file()` prefers `FUSH_CONFIG` when set (use `std::env::set_var`/`remove_var` — tests already run serially in this file).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib config`
Expected: compile error, `primary_config_path_for` not defined.

- [ ] **Step 3: Implement**

`primary_config_path_for` mirrors `config_dir_for` (same base resolution, push `.config/fush/config.fush`). `primary_config_path()` reads env like `config_dir()`. `primary_config_file()` = `FUSH_CONFIG` override else `primary_config_path()`. `source_primary_config(rt)` = `if let Some(p) = primary_config_file() { if p.exists() { source_config_path(rt, &p) } }`. Keep `source_config`/`config_path` intact for the import code in Task 2.

- [ ] **Step 4: Tests + clippy green, commit**

Run: `cargo test 2>&1 | tail -3` then `cargo clippy --all-targets -- -D warnings`
Expected: all pass. Commit: `config: primary config.fush path resolution`

### Task 2: First-run detection + import

**Files:**
- Modify: `src/config.rs`
- Test: `src/config.rs` (unit tests)

**Interfaces:**
- Consumes: Task 1's `primary_config_path_for`, existing `config_path`
- Produces:
  - `pub enum FirstRun { Import, Decline, Nothing }`
  - `pub fn first_run_state(fush: Option<&Path>, fish: Option<&Path>) -> FirstRun` — `Nothing` if `fush` exists or `fish` missing; `Import`/`Decline` recorded by caller after prompting
  - `pub fn import_fish_config(fish: &Path, fush: &Path) -> Result<(), String>` — copy contents; creates parent dirs; atomic (write tmp + rename, same pattern as `src/universal.rs::save`); error string starts `fush: config:`
  - `pub fn create_empty_config(fush: &Path) -> Result<(), String>` — create empty file + parent dirs, same error convention

- [ ] **Step 1: Write failing unit tests**

Tests with temp dirs (pattern from `src/universal.rs` tests): `first_run_state` returns `Nothing` when fush exists (fish present or not), `Nothing` when fish missing (fush present or not), `Import`-pending only when fush missing AND fish exists. `import_fish_config` copies bytes exactly; `create_empty_config` makes a 0-byte file; both create missing parent dirs; import to an existing fush target overwrites (caller only calls it when fush is absent, but the function must not corrupt on misuse — overwrite is fine).

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --lib config`
Expected: compile error, `FirstRun` not defined.

- [ ] **Step 3: Implement**

`first_run_state`: `Nothing` unless `!fush.exists() && fish.exists()`. `import_fish_config`: `std::fs::read` fish → `create_dir_all` parent → write tmp → rename. `create_empty_config`: `create_dir_all` + write empty file (tmp+rename not needed, but harmless to skip).

- [ ] **Step 4: Tests + clippy green, commit**

Run: `cargo test 2>&1 | tail -3` then `cargo clippy --all-targets -- -D warnings`
Expected: all pass. Commit: `config: first-run detection and fish import`

### Task 3: Wire startup hooks + interactive prompt

**Files:**
- Modify: `src/main.rs:60-63` (the `config::source_config` call)
- Modify: `src/repl.rs` (both `source_config` call sites: TTY loop ~line 173, non-TTY ~line 240)
- Test: `tests/integration.rs` (new cases)

**Interfaces:**
- Consumes: Task 1 `source_primary_config`, Task 2 `first_run_state`/`import_fish_config`/`create_empty_config`, existing `crate::config::config_path`
- Produces: no new public API; behavior change — every startup sources `config.fush` only

- [ ] **Step 1: Write failing integration tests**

Using the `FUSH_CONFIG` injection (points at `config.fush`) plus a `FUSH_FISH_CONFIG` injection for the fish path (add to `config_path` resolution like `FUSH_CONFIG` — one small addition in this task):
  - `config.fush` exists with `set x 1` → `fush -c 'echo $x'` prints 1 even when a fish config also sets x=2 (fish ignored)
  - no `config.fush`, `-c` mode → no prompt, no output, exit 0 (non-interactive never imports)
  - unit test in `src/config.rs`: `fish_config_file()` prefers `FUSH_FISH_CONFIG` env else `config_path()`

- [ ] **Step 2: Run to verify failure**

Run: `cargo test --test integration`
Expected: new cases fail (fish config still sourced / config.fush ignored).

- [ ] **Step 3: Implement**

- `src/main.rs` and both `src/repl.rs` hooks: replace `config::source_config(&mut rt)` with `config::source_primary_config(&mut rt)`.
- TTY hook only (`run_repl`, before `load_universals`... actually after it, same position as now): if `first_run_state(fush_path, fish_path)` says pending → print prompt to stderr: `Found a fish config at <path>. Import it into config.fush? [Y/n]`, read one line from stdin; `y`/empty → `import_fish_config` then source the new file; `n` → `create_empty_config`; any error → print `fush: config: <msg>` and continue. After either action, source `config.fush`.
- Non-TTY hook, `-c`, script, `--ast`: never check `first_run_state` — just `source_primary_config`.
- Add `FUSH_FISH_CONFIG` env override + `fish_config_file()` in `src/config.rs`.

- [ ] **Step 4: Manual TTY probe (controller or implementer with a pty)**

Use `script -qec` or a Python pty to drive the prompt: first run with fish config present, no fush config → expect prompt, answer `y` → fush config exists with fish content; second run → no prompt. Answer `n` path: fush config exists empty, fish content not imported.

- [ ] **Step 5: Tests + clippy green, commit**

Run: `cargo test 2>&1 | tail -3` then `cargo clippy --all-targets -- -D warnings`
Expected: all pass. Commit: `config: startup uses config.fush; first-run import prompt`

### Task 4: Windows smoke + docs

**Files:**
- Modify: `ci/smoke-windows.md`, `README.md`
- Test: manual Windows smoke (no new automated tests)

**Interfaces:**
- Consumes: everything above
- Produces: docs only

- [ ] **Step 1: Cross-compile + smoke on Windows**

`cargo build --release --target x86_64-pc-windows-gnu`, copy to `/mnt/c/Users/Francisco/fush-smoke/fush.exe`, drive via `.cmd` batch files (nested quoting through `cmd.exe /c` mangles args):
  - `fush.exe -c "echo hi"` with NO configs → hi, no prompt, exit 0
  - config.fush with `set w x` → `-c "echo $w"` → x
  - config.fish present + no config.fush, `-c` mode → no prompt, no import (non-interactive), exit 0
  - universals round-trip still works (regression check)
- [ ] **Step 2: Append "Config.fush" section to `ci/smoke-windows.md`** (case, command, expected, actual, PASS/FAIL)

- [ ] **Step 3: README status update**

Status section: config is now `config.fush` (primary); existing `config.fish` is offered for one-time import on first interactive start.

- [ ] **Step 4: Commit**

`git commit -s -m "windows: config.fush smoke; readme status"`
