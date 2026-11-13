# Windows smoke checklist

Environment:
- Build host: WSL CachyOS (Linux x86_64), rustc 1.97.0, target x86_64-pc-windows-gnu, mingw-w64-gcc toolchain
- Windows target: Microsoft Windows [Version 10.0.26300.9974]
- Binary: `cargo build --release --target x86_64-pc-windows-gnu`, copied to `C:\Users\Francisco\fush-smoke\fush.exe`
- Smoke driven from WSL via cmd.exe batch files in the fush-smoke directory

Smoke matrix:
| Command | Expected | Result | Notes |
| --- | --- | --- | --- |
| `-c "echo hi"` | `hi` | PASS | |
| `-c "set x a b; echo $x"` | `a b` | PASS | |
| `-c "if test 1 -lt 2; echo yes; else; echo no; end"` | `yes` | PASS | |
| `-c "echo (echo one; echo two)"` | `one two` | PASS | |
| `-c "cmd /c exit 0; echo done"` | `done` | PASS | external process spawn works |
| `-c "cmd /c exit 3; or echo fallback"` | `fallback` | PASS | $status + or works |
| piped stdin REPL (`fush.exe < input.txt` with `echo piped\nexit`) | `piped`, exit 0 | PASS | non-tty fallback loop works on Windows |
| `-c "cd C:\\Windows; cmd /c cd"` | `C:\Windows` | FAIL | `cd` rejects `C:\Windows`: lexer eats `\W` as an escape → `C:Windows` |

Deviations:
- `cd C:\Windows` fails: unquoted backslash is the escape character, so `C:\Windows` lexes to `C:Windows` and `cd` cannot find it. Same behavior as fish/POSIX shells (backslash escapes the next char); workaround on Windows is quoting: `cd "C:\Windows"` works. Deferred: not a blocker for the PoC (quoting is the documented path), but a Windows-first shell should eventually accept unquoted backslash paths inside drive-letter words.
- Hostname prompt uses COMPUTERNAME (lowercased); history lives at `%USERPROFILE%\.fush\history`; both cfg-split and unit-tested on Linux.
- 142 tests green on Linux (95 unit + 47 integration); cross-compile warning-free; clippy clean.

## UX milestone — autosuggestions + highlighting

Environment:
- Build host: WSL CachyOS (Linux x86_64), rustc 1.97.0, target x86_64-pc-windows-gnu, mingw-w64-gcc toolchain
- Windows target: Microsoft Windows [Version 10.0.26300.9974]
- Binary: `cargo build --release --target x86_64-pc-windows-gnu`, copied to `C:\Users\Francisco\fush-smoke\fush.exe`
- Smoke driven from WSL via `.cmd` files in `C:\Users\Francisco\fush-smoke`

Smoke matrix for the UX milestone:
| Command | Expected | Result | Notes |
| --- | --- | --- | --- |
| `fush.exe -c "echo hi"` | `hi` | PASS | no ANSI escape bytes in `-c` output |
| `fush.exe -c "set x a b; echo $x"` | `a b` | PASS | |
| `fush.exe -c "if test 1 -lt 2; echo yes; else; echo no; end"` | `yes` | PASS | |
| `fush.exe -c "cmd /c exit 3; or echo fallback"` | `fallback` | PASS | |
| piped stdin REPL (`fush.exe < input.txt` with `echo piped\nexit`) | `piped`, exit 0 | PASS | |
| `set NO_COLOR=1` then `fush.exe -c "echo hi"` | `hi` | PASS | `NO_COLOR` disables colors in `-c` mode |

Human check still required on a real Windows terminal:
- interactive ghost text / autosuggestion should appear as gray dim text when typing a prefix such as `ec`
- pressing Right/End should accept the suggestion
- command highlighting should show blue keywords, yellow strings, bold builtins, red invalid commands, and gray comments in a real TTY
- ANSI color output is intentionally suppressed in `-c` mode because stdout is a pipe, not a TTY; this is expected behavior

This milestone verifies the release binary and the non-TTY regression behavior for the UX features, while the final visual fidelity of the live TTY scrollback and color rendering still requires a human check on an actual Windows terminal.

## Functions + abbr milestone

Environment:
- Build host: WSL CachyOS (Linux x86_64), rustc 1.97.0, target x86_64-pc-windows-gnu, mingw-w64-gcc toolchain
- Windows target: Microsoft Windows [Version 10.0.26300.9974]
- Binary: `cargo build --release --target x86_64-pc-windows-gnu`, copied to `C:\Users\Francisco\fush-smoke\fush.exe`
- Smoke driven from WSL via `.cmd` files in `C:\Users\Francisco\fush-smoke`
- `-c` mode is intentionally non-TTY; abbreviation expansion must not fire there, and `abbr` still works as a builtin in a script session.

Smoke matrix for the functions + abbr milestone:
| Command | Expected | Result | Notes |
| --- | --- | --- | --- |
| `fush.exe -c "function g; echo hi from fush; end; g"` | `hi from fush` | PASS | function definition + call works in Windows release build |
| `fush.exe -c "function t; return 5; end; t; echo $status"` | `5` | PASS | return code sets `$status` |
| `fush.exe -c "function r; r; end; r"` | recursion error; exit 1 | PASS | returns promptly with no hang |
| `fush.exe -c "function g; echo hi from fush; end; function t; return 5; end; functions"` | `g` and `t` lines | PASS | `functions` lists defined names |
| `fush.exe -c "abbr gs 'git status'; abbr"` | `gs git status` | PASS | abbreviation list output |
| `fush.exe -c "abbr gs 'git status'; gs"` | command-not-found for `gs` and exit 1 | PASS | no interactive expansion in `-c` mode |
| `fush.exe -c "echo hi; exit 3"` | exit code 3 | PASS | explicit exit status propagates |
| `fush.exe -c "set x a b; echo $x"` | `a b` | PASS | baseline variable regression |
| `fush.exe -c "if test 1 -lt 2; echo yes; else; echo no; end"` | `yes` | PASS | baseline if/else regression |
| `fush.exe -c "cmd /c exit 3; or echo fallback"` | `fallback` | PASS | baseline `or` fallback regression |

Verification notes:
- The Windows release build was also checked with the Linux host test suite (`cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo build --release`, `cargo build --release --target x86_64-pc-windows-gnu`).
- The recursive function case was run under a shell timeout to confirm it exits promptly instead of hanging.
- `abbr` expansion is intentionally disabled in `-c` / non-interactive mode, matching fish-style interactive semantics.

## Language completion milestone

Environment:
- Build host: WSL CachyOS (Linux x86_64), rustc 1.97.0, target x86_64-pc-windows-gnu, mingw-w64-gcc toolchain
- Windows target: Microsoft Windows [Version 10.0.26300.9974]
- Binary: `cargo build --release --target x86_64-pc-windows-gnu`, copied to `C:\Users\Francisco\fush-smoke\fush.exe`
- Smoke driven from WSL via `.cmd` files in `C:\Users\Francisco\fush-smoke`, using `cmd.exe /u /c "C:\Users\Francisco\fush-smoke\NAME.cmd"`

Smoke matrix for the language completion milestone:
| Case | Expected | Result | Notes |
| --- | --- | --- | --- |
| `set x a b; echo $x[2]` | `b` | PASS | indexed set/echo works on the Windows release build |
| `function f; echo $argv[1]; end; f a b` | `a` | PASS | list indexing in function context works |
| `if true; echo a; end; and echo yes` | `a`, `yes` | FAIL | `fush: failed to run 'true': program not found` |
| `set x a b; set y c d; echo $x$y` | `ac bc ad bd` | PASS | cartesian concat works on the release binary |
| multibyte glob | `glob` | PASS | verified via UTF-8 `.cmd` file and `.fish` script fallback; batch-file UTF-8 handling is limited when launched from a WSL UNC path |
| abbr + function listing baselines | lists defined names/abbrs | PASS | earlier function + abbr behavior still works |
| exit propagation | exit code 3 | PASS | `fush.exe -c "exit 3"` exits 3 |

Notes:
- The Windows smoke checks were executed with full Windows paths to the batch files (`cmd.exe /u /c "C:\Users\Francisco\fush-smoke\NAME.cmd"`) to avoid WSL UNC-path startup issues that otherwise default to `C:\Windows` and fail before the shell starts.
- For the multibyte glob probe, the batch-file method is subject to the UTF-8/UNC path limitation observed under WSL, so the canonical validation used a UTF-8 `.fish` script file instead: `C:\Users\Francisco\fush-smoke\multibyte.fish` containing `set x café; switch $x; case caf?; echo glob; end`.
- Native Linux verification remained green: `cargo test`, `cargo clippy --all-targets -- -D warnings`, and `cargo build --release` all passed.
- The final milestone status is `DONE_WITH_CONCERNS` because the `and/or after block` smoke fails due to the shell not recognizing a `true` builtin, and the observed output is `fush: failed to run 'true': program not found`.
