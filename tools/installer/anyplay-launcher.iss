; Inno Setup script for the AnyPlay Launcher (a per-user install: no administrator rights).
; Build: iscc /DAppVersion=0.1.0 /DExeSource=<path to the release exe>
;   /DNoticesSource=<path to THIRD-PARTY-LICENSES.txt from tools/notices.sh> /DOutputDir=<folder>
;   anyplay-launcher.iss

#ifndef AppVersion
  #error Pass /DAppVersion=<x.y.z>
#endif
#ifndef ExeSource
  #error Pass /DExeSource=<path to anyplay-launcher.exe>
#endif
#ifndef NoticesSource
  #error Pass /DNoticesSource=<path to THIRD-PARTY-LICENSES.txt>
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

#define AppName "AnyPlay Launcher"
#define AppExe "AnyPlay-Launcher.exe"
; The name up to v0.1.3. An install of then is upgraded in place (same AppId, same folder) and must
; not keep its exe or shortcuts (docs/DESIGN.md 9.7).
#define OldAppName "AnyPS5 Launcher"
#define OldAppExe "AnyPS5-Launcher.exe"

[Setup]
; Never change the AppId: it is how a newer setup finds and replaces an older install.
AppId={{6F0B7F5E-3C1D-4A52-9B8E-0D5E7A41C2A9}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher=AnyPlay Launcher contributors
AppPublisherURL=https://github.com/anyplaynet/launcher
AppSupportURL=https://github.com/anyplaynet/launcher/issues
AppUpdatesURL=https://github.com/anyplaynet/launcher/releases
VersionInfoVersion={#AppVersion}
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
; New installs only: an upgrade keeps its folder (UsePreviousAppDir, on by default), so an install
; from v0.1.x stays in "AnyPS5 Launcher". Never turn that off: the old folder and its uninstaller
; would be left behind.
DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=auto
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
SetupIconFile=..\..\crates\anyps5-launcher\assets\icon.ico
UninstallDisplayIcon={app}\{#AppExe}
UninstallDisplayName={#AppName}
OutputDir={#OutputDir}
OutputBaseFilename=AnyPlay-Launcher-{#AppVersion}-Setup
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern dynamic
CloseApplications=yes
RestartApplications=no

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"; Flags: unchecked

[Files]
Source: "{#ExeSource}"; DestDir: "{app}"; DestName: "{#AppExe}"; Flags: ignoreversion
Source: "..\..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
Source: "{#NoticesSource}"; DestDir: "{app}"; DestName: "THIRD-PARTY-LICENSES.txt"; Flags: ignoreversion

[InstallDelete]
; What an install of the old name leaves: its exe (also one that updated itself, since the swap
; keeps the name), the swap's leftovers beside it, and its shortcuts.
Type: files; Name: "{app}\{#OldAppExe}"
Type: files; Name: "{app}\{#OldAppExe}.new"
Type: files; Name: "{app}\{#OldAppExe}.old*"
Type: files; Name: "{autoprograms}\{#OldAppName}.lnk"
Type: files; Name: "{autodesktop}\{#OldAppName}.lnk"

[UninstallDelete]
; A self-update's leftovers; the next start removes them, an uninstall may come first.
Type: files; Name: "{app}\{#AppExe}.new"
Type: files; Name: "{app}\{#AppExe}.old*"
Type: files; Name: "{app}\{#OldAppExe}*"

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#AppExe}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#AppExe}"; Tasks: desktopicon

[Registry]
; The anyps5:// links and the folder entry "Open with AnyPlay Launcher" (docs/DESIGN.md 9.6); the
; key names keep the old name, links already out there use them. HKA is the
; current user for the default install and the machine for an administrator install. The launcher
; writes and removes the same keys from Settings; the uninstaller removes both here.
Root: HKA; Subkey: "Software\Classes\anyps5"; ValueType: string; ValueData: "URL:AnyPlay Link"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\anyps5"; ValueType: string; ValueName: "URL Protocol"; ValueData: ""
Root: HKA; Subkey: "Software\Classes\anyps5\DefaultIcon"; ValueType: string; ValueData: """{app}\{#AppExe}"",0"
Root: HKA; Subkey: "Software\Classes\anyps5\shell\open\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" ""%1"""
; Folders only, never files.
Root: HKA; Subkey: "Software\Classes\Directory\shell\AnyPS5"; ValueType: string; ValueData: "Open with AnyPlay Launcher"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Classes\Directory\shell\AnyPS5"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\{#AppExe}"",0"
Root: HKA; Subkey: "Software\Classes\Directory\shell\AnyPS5\command"; ValueType: string; ValueData: """{app}\{#AppExe}"" ""%1"""

[Run]
Filename: "{app}\{#AppExe}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[Code]
// A running launcher of the old name holds its exe, which [InstallDelete] cannot remove. Registered
// here, CloseApplications closes it like the files Setup replaces.
procedure RegisterExtraCloseApplicationsResources;
begin
  RegisterExtraCloseApplicationsResource(ExpandConstant('{app}\{#OldAppExe}'));
end;
