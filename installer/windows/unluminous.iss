; The Windows installer for Unluminous.
;
; Built by `installer\windows\build.ps1`, which passes the version and the folder holding the built
; binary in as /D defines, so nothing here has to be edited when the version in Cargo.toml changes.
; `tasks/unluminous-installer-tdd.md` records why this is Inno Setup and not WiX, NSIS or a program of our
; own, and why the defaults below are the defaults.
;
; The short version of the defaults: a plain double click installs into %LOCALAPPDATA%\Programs\Unluminous
; with no elevation prompt at all, and the dialog offers all users for anybody who wants Program
; Files. Unluminous offers itself for text and Markdown files without taking the default association for
; any of them. Uninstalling removes everything it wrote and leaves %APPDATA%\Unluminous — the settings,
; the pane sizes, the recent projects and any installed plugins — alone.

#define AppName        "Unluminous"
#define AppPublisher   "Jason McAffee"
#define AppUrl         "https://github.com/jasonmcaffee/unluminous"
#define ExeName        "unluminous.exe"
; The command line, installed beside the editor. `unluminous-cli` looks for `unluminous` next to itself, so
; being in the same folder is what makes `unluminous-cli launch` work with nothing configured, and the
; PATH task below puts both of them on the path together.
#define CliName        "unluminous-cli.exe"
#define ProgId         "Unluminous.Document"

; Both of these are passed in by build.ps1. The fallbacks are here so that the script can be compiled
; by hand, from this folder, after a `cargo build --release`.
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef BinaryDir
  #define BinaryDir "..\..\target\release"
#endif
#ifndef OutputDir
  #define OutputDir "..\dist"
#endif

[Setup]
; The key every upgrade hangs off. It must never change: the same GUID is what makes the next version
; replace this one rather than sit beside it in Add or Remove Programs.
;
; It changed exactly once, in task-1800, when the product was renamed. Keeping the GUID the old name
; shipped under would have made Inno treat this as an upgrade of that product and install into the
; folder its install recorded, under the old name -- the one folder name the rename existed to be rid
; of. That product was uninstalled by its own uninstaller instead, which is what takes its Add or
; Remove Programs row, its PATH entry, its App Paths key, its three context-menu verbs and its ten
; OpenWithProgids values with it.
AppId={{64635361-18F5-4632-B119-16D487214CA1}
AppName={#AppName}
AppVersion={#AppVersion}
AppVerName={#AppName} {#AppVersion}
AppPublisher={#AppPublisher}
AppPublisherURL={#AppUrl}
AppSupportURL={#AppUrl}
AppUpdatesURL={#AppUrl}
VersionInfoVersion={#AppVersion}
VersionInfoProductName={#AppName}

DefaultDirName={autopf}\{#AppName}
DefaultGroupName={#AppName}
DisableProgramGroupPage=yes
DisableDirPage=no
AllowNoIcons=yes
LicenseFile=license.txt

; Per user by default, and no elevation prompt for it. `dialog` puts the choice on the first page, so
; anybody who wants it in Program Files for everybody can have that instead, and `commandline` lets
; build.ps1 -AllUsers ask for the same thing without a person clicking.
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=commandline dialog

; Unluminous is built for x64 and needs Windows 10 or later.
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0

; The uninstaller is signed too, when build.ps1 was given a certificate.
;
; Inno builds `unins000.exe` while it compiles, so it is the one file in the install that this script
; cannot sign afterwards -- it does not exist until the setup runs. `SignTool` names a command Inno
; runs on it, `build.ps1` passes that command in as `/Sunluminous=...`, and `SignedUninstaller` is
; what asks for it to be used. Without the define nothing here changes, which is what an unsigned
; build has always done. `task-1804` §6.
#ifdef SignCommand
SignTool=unluminous
SignedUninstaller=yes
#endif

; Upgrading while Unluminous is open asks to close it through the Restart Manager, rather than failing on
; a locked file half way through.
CloseApplications=yes
RestartApplications=no
SetupMutex={#AppName}Setup

OutputDir={#OutputDir}
OutputBaseFilename=UnluminousSetup-{#AppVersion}-x64
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
SetupIconFile=..\icon\unluminous.ico
UninstallDisplayIcon={app}\{#ExeName}
UninstallDisplayName={#AppName}

; Both of these tell Explorer to reread what setup changed rather than wait for the next sign in.
ChangesEnvironment=yes
ChangesAssociations=yes

[Languages]
Name: "english"; MessagesFile: "compiler:Default.isl"

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"
Name: "addtopath"; Description: "Add Unluminous to the PATH, so that ""unluminous"" opens a folder and ""unluminous-cli"" drives it from a terminal"; GroupDescription: "Other:"
Name: "contextfile"; Description: "Add ""Open with Unluminous"" to the right click menu of a file"; GroupDescription: "Other:"
Name: "contextfolder"; Description: "Add ""Open with Unluminous"" to the right click menu of a folder"; GroupDescription: "Other:"
Name: "associate"; Description: "Offer Unluminous in ""Open with"" for text, Markdown and source files"; GroupDescription: "Other:"

[Files]
Source: "{#BinaryDir}\{#ExeName}"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinaryDir}\{#CliName}"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\{#AppName}"; Filename: "{app}\{#ExeName}"
Name: "{autodesktop}\{#AppName}"; Filename: "{app}\{#ExeName}"; Tasks: desktopicon

[Registry]
; ---------------------------------------------------------------------------------------------
; Where Windows looks the program up by name, which is what makes Win+R "unluminous" work whether or not
; the PATH task was taken.
; ---------------------------------------------------------------------------------------------
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\{#ExeName}"; ValueType: string; ValueName: ""; ValueData: "{app}\{#ExeName}"; Flags: uninsdeletekey
Root: HKA; Subkey: "Software\Microsoft\Windows\CurrentVersion\App Paths\{#ExeName}"; ValueType: string; ValueName: "Path"; ValueData: "{app}"

; ---------------------------------------------------------------------------------------------
; "Open with Unluminous" on a file, on a folder, and on the empty space inside a folder. %1 is what was
; clicked; %V is the folder being looked at when the click was on its background.
; ---------------------------------------------------------------------------------------------
Root: HKA; Subkey: "Software\Classes\*\shell\{#AppName}"; ValueType: string; ValueName: ""; ValueData: "Open with {#AppName}"; Flags: uninsdeletekey; Tasks: contextfile
Root: HKA; Subkey: "Software\Classes\*\shell\{#AppName}"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\{#ExeName}"",0"; Tasks: contextfile
Root: HKA; Subkey: "Software\Classes\*\shell\{#AppName}\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#ExeName}"" ""%1"""; Tasks: contextfile

Root: HKA; Subkey: "Software\Classes\Directory\shell\{#AppName}"; ValueType: string; ValueName: ""; ValueData: "Open with {#AppName}"; Flags: uninsdeletekey; Tasks: contextfolder
Root: HKA; Subkey: "Software\Classes\Directory\shell\{#AppName}"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\{#ExeName}"",0"; Tasks: contextfolder
Root: HKA; Subkey: "Software\Classes\Directory\shell\{#AppName}\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#ExeName}"" ""%1"""; Tasks: contextfolder
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\{#AppName}"; ValueType: string; ValueName: ""; ValueData: "Open with {#AppName}"; Flags: uninsdeletekey; Tasks: contextfolder
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\{#AppName}"; ValueType: string; ValueName: "Icon"; ValueData: """{app}\{#ExeName}"",0"; Tasks: contextfolder
Root: HKA; Subkey: "Software\Classes\Directory\Background\shell\{#AppName}\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#ExeName}"" ""%V"""; Tasks: contextfolder

; ---------------------------------------------------------------------------------------------
; The file kinds. Unluminous registers itself as an application that *can* open these and appears in Open
; with; it does not take the default association for any of them. A text editor that silently becomes
; the owner of .json is a text editor people uninstall.
; ---------------------------------------------------------------------------------------------
Root: HKA; Subkey: "Software\Classes\{#ProgId}"; ValueType: string; ValueName: ""; ValueData: "{#AppName} Document"; Flags: uninsdeletekey; Tasks: associate
Root: HKA; Subkey: "Software\Classes\{#ProgId}\DefaultIcon"; ValueType: string; ValueName: ""; ValueData: """{app}\{#ExeName}"",0"; Tasks: associate
Root: HKA; Subkey: "Software\Classes\{#ProgId}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#ExeName}"" ""%1"""; Tasks: associate

Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}"; ValueType: string; ValueName: "FriendlyAppName"; ValueData: "{#AppName}"; Flags: uninsdeletekey; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\shell\open\command"; ValueType: string; ValueName: ""; ValueData: """{app}\{#ExeName}"" ""%1"""; Tasks: associate

Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".md"; ValueData: ""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".markdown"; ValueData: ""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".txt"; ValueData: ""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".rs"; ValueData: ""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".js"; ValueData: ""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".ts"; ValueData: ""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".json"; ValueData: ""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".toml"; ValueData: ""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".yml"; ValueData: ""; Tasks: associate
Root: HKA; Subkey: "Software\Classes\Applications\{#ExeName}\SupportedTypes"; ValueType: string; ValueName: ".yaml"; ValueData: ""; Tasks: associate

Root: HKA; Subkey: "Software\Classes\.md\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.markdown\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.txt\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.rs\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.js\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.ts\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.json\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.toml\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.yml\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate
Root: HKA; Subkey: "Software\Classes\.yaml\OpenWithProgids"; ValueType: string; ValueName: "{#ProgId}"; ValueData: ""; Flags: uninsdeletevalue; Tasks: associate

[Run]
Filename: "{app}\{#ExeName}"; Description: "{cm:LaunchProgram,{#AppName}}"; Flags: nowait postinstall skipifsilent

[Code]
{ The PATH entry.

  It is done here rather than in [Registry] for two reasons. The environment lives under a different
  key for a per user install than for a per machine one, so one [Registry] line cannot serve both.
  And taking an entry back out of a semicolon separated list on uninstall is not something a registry
  line can express at all: it has to be read, split and written back without our part.

  Reading and writing it back is only safe because RegQueryStringValue hands back what is stored
  rather than what it expands to. A PATH commonly holds %USERPROFILE% or a version variable, and a
  read that expanded them would freeze today's values into somebody's environment for good. }

const
  UserEnvironmentKey = 'Environment';
  MachineEnvironmentKey = 'SYSTEM\CurrentControlSet\Control\Session Manager\Environment';

{ Which hive and key the PATH being changed lives in, which depends on whether this is a per machine
  install or a per user one. }
function EnvironmentRoot(): Integer;
begin
  if IsAdminInstallMode() then
    Result := HKEY_LOCAL_MACHINE
  else
    Result := HKEY_CURRENT_USER;
end;

function EnvironmentKey(): String;
begin
  if IsAdminInstallMode() then
    Result := MachineEnvironmentKey
  else
    Result := UserEnvironmentKey;
end;

{ Whether Path already has Entry in it, compared without regard to case or to a trailing slash. The
  semicolons on both sides are what stop "C:\Unluminous" matching inside "C:\Unluminous Extras". }
function PathHasEntry(const Path, Entry: String): Boolean;
begin
  Result := Pos(';' + Uppercase(Entry) + ';', ';' + Uppercase(Path) + ';') > 0;
end;

// Put the install folder on the PATH, once.
procedure AddApplicationToPath();
var
  Existing: String;
  Entry: String;
begin
  Entry := ExpandConstant('{app}');
  if not RegQueryStringValue(EnvironmentRoot(), EnvironmentKey(), 'Path', Existing) then
    Existing := '';
  if PathHasEntry(Existing, Entry) then
    exit;
  if (Existing <> '') and (Existing[Length(Existing)] <> ';') then
    Existing := Existing + ';';
  RegWriteExpandStringValue(EnvironmentRoot(), EnvironmentKey(), 'Path', Existing + Entry);
end;

{ Take the install folder back off the PATH.
  Every other entry is put back exactly as it was, in the order it was in, including an empty one.
  An empty entry is somebody's PATH having a doubled semicolon in it, which is theirs and not ours to
  tidy up: an uninstaller that quietly rewrites the rest of the PATH while removing its own entry is
  an uninstaller that broke something a fortnight later for a reason nobody can find. }
procedure RemoveApplicationFromPath();
var
  Existing: String;
  Rebuilt: String;
  Part: String;
  Entry: String;
  Cut: Integer;
  First: Boolean;
begin
  Entry := Uppercase(ExpandConstant('{app}'));
  if not RegQueryStringValue(EnvironmentRoot(), EnvironmentKey(), 'Path', Existing) then
    exit;

  Rebuilt := '';
  First := True;
  { A semicolon on the end so that the last entry is read by the same step as the others. The empty
    string it leaves behind is what ends the loop, and is not itself an entry. }
  Existing := Existing + ';';
  repeat
    Cut := Pos(';', Existing);
    Part := Copy(Existing, 1, Cut - 1);
    Existing := Copy(Existing, Cut + 1, Length(Existing));
    if Uppercase(Part) <> Entry then
    begin
      if not First then
        Rebuilt := Rebuilt + ';';
      Rebuilt := Rebuilt + Part;
      First := False;
    end;
  until Existing = '';

  RegWriteExpandStringValue(EnvironmentRoot(), EnvironmentKey(), 'Path', Rebuilt);
end;

procedure CurStepChanged(CurStep: TSetupStep);
begin
  if (CurStep = ssPostInstall) and WizardIsTaskSelected('addtopath') then
    AddApplicationToPath();
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
begin
  // Before the files go, because the install folder still has to resolve. The settings, the pane
  // sizes, the recent projects and any installed plugins live in %APPDATA%\Unluminous and are
  // deliberately left alone: uninstalling to install a newer version must not throw them away.
  if CurUninstallStep = usUninstall then
    RemoveApplicationFromPath();
end;
