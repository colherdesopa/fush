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
