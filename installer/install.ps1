# fush one-line installer (PowerShell 5.1+)
# Usage: irm https://raw.githubusercontent.com/colherdesopa/fush/main/installer/install.ps1 | iex
# Or with options:
#   $install = irm https://raw.githubusercontent.com/colherdesopa/fush/main/installer/install.ps1
#   & ([scriptblock]::Create($install)) -NoWT   # skip Windows Terminal profile
param(
    [switch]$NoWT,
    [switch]$NoPath
)
$ErrorActionPreference = 'Stop'

$repo = 'colherdesopa/fush'
$installDir = Join-Path $env:LOCALAPPDATA 'Programs\fush'
$wtGuid = '{8f4d1a2b-9c3e-4f7a-b5d6-e1f2a3b4c5d6}'

function Info($msg) { Write-Host $msg -ForegroundColor Cyan }
function Ok($msg)   { Write-Host $msg -ForegroundColor Green }

# --- resolve latest version ---
Info 'Fetching latest release...'
$release = Invoke-RestMethod -Uri "https://api.github.com/repos/$repo/releases/latest" -Headers @{ 'User-Agent' = 'fush-installer' }
$version = $release.tag_name.TrimStart('v')
$asset = $release.assets | Where-Object { $_.name -eq "fush-v$version-win64.zip" } | Select-Object -First 1
if (-not $asset) { throw "No fush-v$version-win64.zip asset on release $($release.tag_name)" }

# --- download and extract ---
$tmp = Join-Path $env:TEMP "fush-install-$version"
New-Item -ItemType Directory -Force $tmp | Out-Null
$zip = Join-Path $tmp 'fush.zip'
Info "Downloading fush $version..."
Invoke-WebRequest -Uri $asset.browser_download_url -OutFile $zip -UseBasicParsing
Expand-Archive -Path $zip -DestinationPath $tmp -Force

# --- install ---
New-Item -ItemType Directory -Force $installDir | Out-Null
Copy-Item (Join-Path $tmp 'fush.exe') (Join-Path $installDir 'fush.exe') -Force
Ok "Installed $installDir\fush.exe"

# --- PATH ---
if (-not $NoPath) {
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($userPath -notlike "*$installDir*") {
        [Environment]::SetEnvironmentVariable('Path', "$userPath;$installDir", 'User')
        $env:Path += ";$installDir"
        Ok 'Added to user PATH'
    }
}

# --- Windows Terminal profile ---
if (-not $NoWT) {
    $pkgRoot = Join-Path $env:LOCALAPPDATA 'Packages'
    $wtSettings = Get-ChildItem -Path $pkgRoot -Filter 'Microsoft.WindowsTerminal*' -Directory |
        ForEach-Object { Join-Path $_.FullName 'LocalState\settings.json' } |
        Where-Object { Test-Path $_ } | Select-Object -First 1
    if ($wtSettings) {
        Info 'Adding Windows Terminal profile...'
        $wtScript = Join-Path $tmp 'wt-profile.ps1'
        Invoke-WebRequest -Uri "https://raw.githubusercontent.com/colherdesopa/fush/main/installer/wt-profile.ps1" -OutFile $wtScript -UseBasicParsing
        Copy-Item $wtScript (Join-Path $installDir 'wt-profile.ps1') -Force
        Copy-Item $wtSettings "$wtSettings.fush-bak" -Force -ErrorAction SilentlyContinue
        if (-not (Test-Path "$wtSettings.fush-bak")) { Copy-Item $wtSettings "$wtSettings.fush-bak" }
        # Run wt-profile in the same session (works under both Windows PowerShell 5.1 and pwsh 7)
        & $wtScript -SettingsPath $wtSettings -Commandline (Join-Path $installDir 'fush.exe')
        if ($LASTEXITCODE -eq 0) { Ok 'Windows Terminal profile added' }
        else { Write-Warning "Windows Terminal profile not modified (exit $LASTEXITCODE). See %TEMP%\fush-wt-profile.log" }
    } else {
        Write-Host 'Windows Terminal not found; skipped profile setup.' -ForegroundColor Yellow
    }
}

Remove-Item $tmp -Recurse -Force -ErrorAction SilentlyContinue
Ok "Done. fush $version installed. Open a new terminal and run: fush"
