# Installer + CI milestone

Date: 2026-11-14
Status: approved (approach 2, greyfish-parity; user confirmed "2" at m04739)

## Goal

Ship fush as a double-click installer (Inno Setup), with CI on every push and
an automated release pipeline on tags. Windows Terminal integration optional
at install time (profile + optional default), same proven pattern as the
archived greyfish installer.

## Decisions

- **Approach**: greyfish-parity port. Per-user install to
  `%LOCALAPPDATA%\Programs\fush`, no admin (PrivilegesRequired=lowest).
- **WT options**: both offered as components (profile + default), like
  greyfish. User confirmed approach 2; WT-default included per my lean in
  m04733 (user approved the approach wholesale).
- **winget**: out of this milestone. Needs repo public + first release with
  stable URLs. Follow-up milestone.
- **CI comes first**: fush has no workflows; the release workflow depends on
  the build being reproducible in CI.
- **Reuse**: `wt-profile.ps1` is ported from greyfish (archived repo at
  /home/francisco/personal/greyfish/installer/wt-profile.ps1, 94 lines,
  scanner-clean, tested live there); parameterized for fush (profile name
  "fush", GUID, icon path, commandline).

## Design

### installer/fush.iss (new)

- AppId: new GUID, stable forever: `{{7E4A2C15-8B3F-4D92-9A6B-FUSH00000001}`
- AppVersion read from Cargo.toml at build time (CI passes /DAppVersion)
- DefaultDirName: `{localappdata}\Programs\fush`
- PrivilegesRequired=lowest; ArchitecturesAllowed=x64compatible;
  ArchitecturesInstallIn64BitMode=x64compatible
- OutputBaseFilename: `fush-setup-{#AppVersion}`
- [Components]: `shell` (fixed, always installed), `wtp` (WT profile),
  `wtdefault` (WT default profile; implies wtp)
- [Tasks]: `addpath` (add install dir to user PATH, checked once)
- [Files]:
  - `dist\fush.exe` → `{app}` (Components: shell)
  - `installer\wt-profile.ps1` → `{app}` (Components: shell — always, so
    uninstall works regardless of what was chosen)
  - `assets\logo.png` → `{app}` (Components: shell) — fush has no logo yet;
    use a placeholder path and skip the file entry if absent at build time
    (CI guard: only pass logo if assets/logo.png exists)
- [Code] ported from greyfish fish.iss:
  - `WTSettingsPath()` — FindFirst over
    `%LOCALAPPDATA%\Packages\Microsoft.WindowsTerminal*\LocalState\settings.json`
  - `AddUserPath` / `RemoveUserPath` — HKCU Environment Path, case-insensitive
    containment check, `;;` / edge-`;` cleanup
  - `RunWTProfile()` — exec powershell.exe -NoProfile -ExecutionPolicy Bypass
    -File {app}\wt-profile.ps1 (args via string concat; Format() with '[' at
    line start breaks the Inno parser — greyfish lesson)
  - `CurStepChanged(ssPostInstall)`: addpath → AddUserPath; wtdefault →
    RunWTProfile(-Commandline ... -SetDefault); else wtp → RunWTProfile
  - `CurUninstallStepChanged(usUninstall)`: RemoveUserPath + RunWTProfile
    (-Remove)
- WT profile: GUID `{8f4d1a2b-9c3e-4f7a-b5d6-e1f2a3b4c5d6}` (new, not
  greyfish's), name "fush", commandline `"{app}\fush.exe"`, icon `{app}\logo.png`
  if shipped

### installer/wt-profile.ps1 (ported from greyfish)

Parameterized: profile GUID, name, icon filename passed via script constants
at top (edit in place, not via args — keep greyfish's arg surface: SettingsPath,
Commandline, SetDefault, Remove). Behavior identical to greyfish's tested
version: backup once to `.greyfish-bak` (rename to `.fush-bak`), Add-Member
-Force for missing profiles/list props, ConvertTo-Json -Depth 100, UTF8.

### .github/workflows/ci.yml (new)

On push + PR to main:
- job `test` (ubuntu-latest): rustfmt check, `cargo clippy --all-targets -- -D
  warnings`, `cargo test`
- job `windows` (windows-latest): `cargo build --release` (native MSVC? NO —
  keep the tested toolchain: rustup target add x86_64-pc-windows-gnu and build
  with the GNU target; if MSVC is simpler on windows-latest and tests pass,
  GNU is still the shipped artifact — decide in task: try native `cargo test`
  on windows-latest first; if the suite passes, run tests natively and build
  the release exe with the GNU target for the artifact; if native tests fail
  for toolchain reasons, fall back to ubuntu cross-compile check only)
- Both jobs must pass; no coverage, no caching beyond actions/cache defaults.

### .github/workflows/release.yml (new)

On tag push `v*`:
- job `build` (windows-latest): checkout, rustup GNU target, `cargo build
  --release --target x86_64-pc-windows-gnu`, copy exe to dist/
- job `installer` (needs build, windows-latest): download exe artifact, choco
  install innosetup (pinned version), ISCC compile fush.iss with /DAppVersion
  from the tag, upload fush-setup-*.exe artifact
- job `release` (needs [build, installer]): create GitHub release (softprops
  action or gh CLI), attach fush.exe zip + installer + SHA256SUMS file

### README

Update Status: installer + CI live, release channel described (tag → release
with setup.exe). Installation section: winget note as "coming".

## Task breakdown

### Task 1: CI workflow

- .github/workflows/ci.yml exactly as designed; validate with `act` if
  available, else push and watch the run
- Pin: actions/checkout@v4, rust toolchain via dtolnay/rust-toolchain@stable
  with target override
- Acceptance: green run on main

### Task 2: wt-profile.ps1 port

- Copy from greyfish, rename constants (.fush-bak, fush GUID, name, icon),
  keep arg surface; comment-free (scanner policy does not apply to fush repo,
  but keep it clean anyway)
- Acceptance: script exists; PowerShell syntax check (pwsh -NoProfile -Command
  "Get-Command -Syntax" or AST parse); manual review

### Task 3: fush.iss + local compile

- installer/fush.iss exactly as designed; compile locally if Inno available
  (greyfish lesson: Defender quarantined SetupLdr.e32 on this machine — if
  compilation is blocked locally, CI compiles it; the iss must at least pass
  ISCC syntax validation, or fall back to CI-only validation)
- Acceptance: setup.exe builds (locally or in CI)

### Task 4: release workflow + end-to-end

- .github/workflows/release.yml as designed; README status update
- Tag v0.1.0, watch the release run, verify artifacts (installer + zip +
  checksums)
- Acceptance: a GitHub release exists with working artifacts; user
  double-click-tests the installer on Windows

## Review focus

- PATH manipulation: idempotent (reinstall does not duplicate), uninstall
  removes exactly what install added
- WT integration: profile added under correct GUID; default set only when
  chosen; uninstall removes profile and nulls default only if it points at
  fush
- release.yml: artifacts immutable (SHA256SUMS), installer version matches
  tag, no secrets in workflow
- ci.yml: fast (under ~5 min), both jobs required
