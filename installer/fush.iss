#ifndef AppVersion
#define AppVersion "0.1.0"
#endif
#define AppName "fush"
#define AppPublisher "fush (unofficial fish-like shell)"
#define ProfileGuid "{8f4d1a2b-9c3e-4f7a-b5d6-e1f2a3b4c5d6}"

[Setup]
AppId={{7E4A2C15-8B3F-4D92-9A6B-FUSH00000001}
AppName={#AppName}
AppVersion={#AppVersion}
AppPublisher={#AppPublisher}
DefaultDirName={localappdata}\Programs\fush
PrivilegesRequired=lowest
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
UninstallDisplayName={#AppName}
OutputBaseFilename=fush-setup-{#AppVersion}
SourceDir=..

[Components]
Name: "shell"; Description: "fush shell (required)"; Flags: fixed
Name: "wtp"; Description: "Windows Terminal profile"; Flags: checkablealone
Name: "wtdefault"; Description: "Make fush the default profile in Windows Terminal"; Flags: checkablealone

[Tasks]
Name: "addpath"; Description: "Add fush to PATH (current user)"; Flags: checkedonce

[Files]
Source: "dist\fush.exe"; DestDir: "{app}"; Flags: ignoreversion; Components: shell
Source: "installer\wt-profile.ps1"; DestDir: "{app}"; Flags: ignoreversion; Components: shell

[Code]
function WTSettingsPath(): string;
var
  FindRec: TFindRec;
  BaseDir: string;
begin
  Result := '';
  BaseDir := ExpandConstant('{localappdata}') + '\Packages';
  if FindFirst(BaseDir + '\Microsoft.WindowsTerminal*', FindRec) then
  begin
    try
      repeat
        if FileExists(BaseDir + '\' + FindRec.Name + '\LocalState\settings.json') then
        begin
          Result := BaseDir + '\' + FindRec.Name + '\LocalState\settings.json';
          break;
        end;
      until not FindNext(FindRec);
    finally
      FindClose(FindRec);
    end;
  end;
end;

procedure AddUserPath(AppDir: string);
var
  OldPath: string;
begin
  RegQueryStringValue(HKCU, 'Environment', 'Path', OldPath);
  if Pos(Uppercase(AppDir), Uppercase(OldPath)) > 0 then exit;
  if OldPath = '' then
    RegWriteExpandStringValue(HKCU, 'Environment', 'Path', AppDir)
  else
    RegWriteExpandStringValue(HKCU, 'Environment', 'Path', OldPath + ';' + AppDir);
end;

procedure RemoveUserPath(AppDir: string);
var
  OldPath: string;
  P: Integer;
begin
  RegQueryStringValue(HKCU, 'Environment', 'Path', OldPath);
  P := Pos(Uppercase(AppDir), Uppercase(OldPath));
  if P = 0 then exit;
  Delete(OldPath, P, Length(AppDir));
  StringChangeEx(OldPath, ';;', ';', True);
  if Copy(OldPath, 1, 1) = ';' then Delete(OldPath, 1, 1);
  if Copy(OldPath, Length(OldPath), 1) = ';' then Delete(OldPath, Length(OldPath), 1);
  RegWriteExpandStringValue(HKCU, 'Environment', 'Path', OldPath);
end;

procedure RunWTProfile(SettingsPath: string; Args: string);
var
  ResultCode: Integer;
  PSCommand: string;
begin
  PSCommand := '-NoProfile -ExecutionPolicy Bypass -File "' + ExpandConstant('{app}') + '\wt-profile.ps1' + '" -SettingsPath "' + SettingsPath + '" ' + Args;
  Exec(ExpandConstant('{syswindows}\System32\WindowsPowerShell\v1.0\powershell.exe'), PSCommand, '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
  if ResultCode <> 0 then
    MsgBox('Windows Terminal settings were not modified (exit ' + IntToStr(ResultCode) + ').', mbError, MB_OK);
end;

procedure CurStepChanged(CurStep: TSetupStep);
var
  SettingsPath: string;
begin
  if CurStep <> ssPostInstall then exit;
  if IsTaskSelected('addpath') then
    AddUserPath(ExpandConstant('{app}'));
  if not (WizardIsComponentSelected('wtp') or WizardIsComponentSelected('wtdefault')) then exit;
  SettingsPath := WTSettingsPath();
  if SettingsPath = '' then
  begin
    MsgBox('Windows Terminal was not found; the profile was not added. Run fush.exe in the installation folder.', mbInformation, MB_OK);
    exit;
  end;
  if WizardIsComponentSelected('wtdefault') then
    RunWTProfile(SettingsPath, Format('-Commandline "%s" -SetDefault', [ExpandConstant('{app}') + '\fush.exe']))
  else if WizardIsComponentSelected('wtp') then
    RunWTProfile(SettingsPath, Format('-Commandline "%s"', [ExpandConstant('{app}') + '\fush.exe']));
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  SettingsPath: string;
begin
  if CurUninstallStep <> usUninstall then exit;
  RemoveUserPath(ExpandConstant('{app}'));
  SettingsPath := WTSettingsPath();
  if SettingsPath = '' then exit;
  if not FileExists(ExpandConstant('{app}') + '\wt-profile.ps1') then exit;
  RunWTProfile(SettingsPath, '-Remove');
end;
