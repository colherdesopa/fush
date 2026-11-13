# fush

**Friendly Unleashed Shell** — a fish-like shell, Windows-native, written in Rust.

No Cygwin. No MSYS2. No WSL. No fork. Just fish's friendly syntax and UX
(autosuggestions, sane scripting) talking to Windows directly.

## Status

Language completion done: indexing, and/or after blocks, cartesian concat, and char-wise glob are in place and smoke-checked on the Windows release build. See [docs/DESIGN.md](docs/DESIGN.md)
for the goal and the road.

## Why

fish is the friendliest shell ever built, but on Windows it only exists inside
a POSIX emulation layer (Cygwin/MSYS2/WSL), which costs ~40 ms per external
process and a heavy startup. fush is the bet that fish's *language and UX* —
not its Unix substrate — are what people actually love, and that a shell built
Windows-native from day one can deliver the same friendliness at native speed.

## Non-goals

- Being a drop-in fish replacement (scripts, config, completions)
- POSIX sh / bash compatibility, on any platform
- Unix as a first-class target (it may compile there; Windows comes first)

## License

GPL-2.0 (same as fish; fush borrows its syntax and UX ideas)
