param(
    [Parameter(Mandatory = $true)]
    [string]$SettingsPath,
    [AllowEmptyString()]
    [string]$Commandline,
    [switch]$SetDefault,
    [switch]$Remove
)

$ErrorActionPreference = 'Stop'
$profileGuid = '{8f4d1a2b-9c3e-4f7a-b5d6-e1f2a3b4c5d6}'
$profileName = 'fush'

if (-not (Test-Path $SettingsPath)) {
    Write-Error "settings.json not found: $SettingsPath"
    exit 1
}

$backup = "$SettingsPath.fush-bak"
if (-not (Test-Path $backup)) {
    Copy-Item $SettingsPath $backup
}

$raw = Get-Content $SettingsPath -Raw -Encoding UTF8
# settings.json is JSONC: strip // and /* */ comments outside strings, plus trailing commas,
# because Windows PowerShell 5.1 ConvertFrom-Json rejects them.
$clean = New-Object System.Text.StringBuilder
$inString = $false
$inLineComment = $false
$inBlockComment = $false
$i = 0
while ($i -lt $raw.Length) {
    $c = $raw[$i]
    $next = if ($i + 1 -lt $raw.Length) { $raw[$i + 1] } else { '' }
    if ($inLineComment) {
        if ($c -eq "`n") { $inLineComment = $false; [void]$clean.Append($c) }
    }
    elseif ($inBlockComment) {
        if ($c -eq '*' -and $next -eq '/') { $inBlockComment = $false; $i++ }
    }
    elseif ($inString) {
        [void]$clean.Append($c)
        if ($c -eq '\') { if ($i + 1 -lt $raw.Length) { [void]$clean.Append($raw[$i + 1]); $i++ } }
        elseif ($c -eq '"') { $inString = $false }
    }
    elseif ($c -eq '/' -and $next -eq '/') { $inLineComment = $true; $i++ }
    elseif ($c -eq '/' -and $next -eq '*') { $inBlockComment = $true; $i++ }
    elseif ($c -eq '"') { $inString = $true; [void]$clean.Append($c) }
    else { [void]$clean.Append($c) }
    $i++
}
$json = ([regex]::Replace($clean.ToString(), ',(\s*[}\]])', '$1')) | ConvertFrom-Json

if ($Remove) {
    if ($json.profiles.list) {
        $json.profiles.list = @($json.profiles.list | Where-Object { $_.guid -ne $profileGuid })
    }
    if ($json.defaultProfile -eq $profileGuid) {
        $json.defaultProfile = $null
    }
    $json | ConvertTo-Json -Depth 100 | Set-Content $SettingsPath -Encoding UTF8
    exit 0
}

if ($SetDefault -and (-not $Commandline)) {
    $existingCount = 0
    if ($json.profiles -and $json.profiles.list) {
        $existingCount = @($json.profiles.list | Where-Object { $_.guid -eq $profileGuid }).Count
    }
    if ($existingCount -eq 0) {
        Write-Error 'No existing fush profile to set as default; use -Commandline to add one'
        exit 1
    }
    $json | Add-Member -NotePropertyName defaultProfile -NotePropertyValue $profileGuid -Force
    $json | ConvertTo-Json -Depth 100 | Set-Content $SettingsPath -Encoding UTF8
    exit 0
}
if (-not $Commandline) {
    Write-Error 'Commandline required when adding a profile'
    exit 1
}

$icon = $null
if ($Commandline) {
    $icon = Join-Path (Split-Path $Commandline -Parent) 'logo.png'
}
$newProfile = [pscustomobject][ordered]@{
    guid            = $profileGuid
    name            = $profileName
    commandline     = $Commandline
    startingDirectory = '%USERPROFILE%'
}
if (Test-Path $icon) {
    $newProfile | Add-Member -NotePropertyName icon -NotePropertyValue $icon
}

$match = @($json.profiles.list | Where-Object { $_.guid -eq $profileGuid })
if ($match.Count -gt 0) {
    for ($i = 0; $i -lt $json.profiles.list.Count; $i++) {
        if ($json.profiles.list[$i].guid -eq $profileGuid) {
            $json.profiles.list[$i] = $newProfile
        }
    }
}
else {
    if (-not $json.profiles) {
        $json | Add-Member -NotePropertyName profiles -NotePropertyValue ([pscustomobject]@{ list = @() }) -Force
    }
    if (-not $json.profiles.list) {
        $json.profiles | Add-Member -NotePropertyName list -NotePropertyValue @() -Force
    }
    $list = @($json.profiles.list)
    $list += $newProfile
    $json.profiles.list = $list
}

if ($SetDefault) {
    $json | Add-Member -NotePropertyName defaultProfile -NotePropertyValue $profileGuid -Force
}

$json | ConvertTo-Json -Depth 100 | Set-Content $SettingsPath -Encoding UTF8
exit 0
