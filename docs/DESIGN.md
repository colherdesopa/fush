# fush design

Friendly Unleashed Shell: fish-like syntax and UX, Windows-native, Rust.

## Intent

fish is loved for its language (no `$` sigils, `set` instead of `export`,
`and`/`or`, no backticks) and its interactive UX (autosuggestions, syntax
highlighting, universal variables). On Windows, real fish requires a POSIX
emulation layer whose fork emulation costs ~40 ms per external command and
~85 ms startup. fush keeps what people love about fish and drops the Unix
substrate: it talks to Windows APIs (CreateProcess, ConPTY) directly.

## Goals

- fish's core syntax: `set var value`, command substitution `(cmd)`, `and`/`or`
  chaining, `$var` expansion (no word splitting), quoting rules, `if`/`for`/
  `while`/`switch` blocks, functions, `abbr`-style abbreviations
- fish's interactive feel: autosuggestions (gray ghost text), syntax
  highlighting as you type, `Alt+.` history, right prompt
- Native speed: process spawn via CreateProcess (no fork emulation), target
  <5 ms spawn, <15 ms cold start
- Windows-first: ConPTY, `C:\` paths, `USERPROFILE` config home, `.exe`
  completion, PATH semantics; runs on any Windows 10/11 x64 without installs

## Non-goals

- Drop-in fish compatibility (this is a new shell, fish-*inspired*)
- POSIX/bash syntax anywhere
- Structured data piping (that's nushell's identity, not ours)
- Unix as a first-class target

## Anti-goals

- Emulating POSIX semantics (fork, signals, executable bits) — that's the
  layer fush exists to delete
- A config language that needs a manual before the first hour of use

## Approaches considered

1. **Package fish on Cygwin (greyfish)** — done, works, but the emulation tax
   is structural: fork emulation is the price of fish's POSIX semantics, not a
   bug to fix.
2. **Nushell engine + fish syntax** — rejected: fish's semantics *are* the
   string/fork model; bolting them onto nushell rebuilds the compat problem.
3. **New shell, fish-inspired, Windows-native (chosen)** — a fresh language
   implementation with fish's surface syntax, native exec, and none of the
   compatibility albatross.

## PoC scope (this afternoon)

A binary that:
1. Parses a fish subset: pipelines with `|`, command substitution `(cmd)`,
   `$var` expansion, quoting, `and`/`or`/`not`, `set`, `if/else`, `for`,
   comments
2. Executes builtins (`set`, `echo`, `cd`, `exit`, `test`) and externals via
   CreateProcess
3. Runs an interactive loop with a prompt, line editing, and history

Out of scope for the PoC: autosuggestions, highlighting, completions,
functions, `while`/`switch`, config files, right prompt.

## After the PoC (in order)

1. Autosuggestions + highlighting (the fish killer feature; reedline or custom)
2. Functions + `abbr`
3. `if`/`for`/`while`/`switch` complete
4. Config home (`%USERPROFILE%\.config\fush`), universal variables
5. Completion framework (manual completions first, then command introspection)
6. Profiles: named allow-list shells (see below)
7. Installer (Inno) + winget

## Profiles (agent allow-lists)

A shell profile is a named allow-list of what may run in that session. Start
fush with a profile (`fush --profile NAME`, or a launcher/shortcut that does);
the session then permits only:

- Builtins and externals listed in the profile (e.g. `git`, `cargo test`,
  `rg` — including argument constraints where cheap: prefix or fixed-argv
  rules)
- Variables, functions, and control flow freely (the language itself is safe;
  the danger is process spawning and file writes)

Anything not allowed fails closed with a clear message (`not allowed in this
profile: npm install`) and a non-zero status. Motivation: hand an agent (or a
junior, or a kiosk) a shell that is powerful in-language but cannot run
arbitrary commands or write outside sanctioned paths. Redirections and
command substitution inherit the profile check (the inner command is checked
too). Profiles are plain fish-syntax config files in the config home
(`%USERPROFILE%\.config\fush\profiles\NAME.fish`), so they use the same
language fush already speaks — define the allow-list with `set`, functions,
and a `profile allow` builtin or a simple declarative list; exact shape is a
design task when we get there. `--profile` without a name = read-only shell
(no externals at all).

## Constraints

- Rust, no C runtime beyond what Rust links
- Windows API via the `windows`/`windows-sys` crates or std where possible
- ConPTY for the terminal (or a line-editing crate that does)
- License GPL-2.0 (fish heritage)
