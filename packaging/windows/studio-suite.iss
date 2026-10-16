; Studio Suite installer for Windows (Inno Setup 6). Built by scripts/make-windows.ps1, which passes
; AppVersion, Arch, Stage (the staged folder) and OutDir.
; Per-user install into %LOCALAPPDATA%\Programs\Studio Hub: no administrator rights needed, and the
; path AI tools are given (studio-hub.exe mcp) stays the same across updates.

#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif
#ifndef Arch
  #define Arch "x64"
#endif

[Setup]
AppId={{6C1E8A52-4D0B-4E43-9C55-57A1D0B5C0DE}
AppName=Studio Hub
AppVersion={#AppVersion}
AppVerName=Studio Hub {#AppVersion}
AppPublisher=Studio Hub
DefaultDirName={localappdata}\Programs\Studio Hub
DefaultGroupName=Studio Hub
DisableProgramGroupPage=yes
PrivilegesRequired=lowest
ArchitecturesAllowed={#Arch}compatible
ArchitecturesInstallIn64BitMode={#Arch}compatible
OutputDir={#OutDir}
OutputBaseFilename=Studio-Suite-{#AppVersion}-windows-{#Arch}-setup
SetupIconFile={#Stage}\StudioHub.ico
UninstallDisplayIcon={app}\studio-hub.exe
Compression=lzma2/max
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "{cm:CreateDesktopIcon}"; GroupDescription: "{cm:AdditionalIcons}"

[Files]
Source: "{#Stage}\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{autoprograms}\Studio Hub"; Filename: "{app}\studio-hub.exe"; IconFilename: "{app}\StudioHub.ico"
Name: "{autodesktop}\Studio Hub"; Filename: "{app}\studio-hub.exe"; IconFilename: "{app}\StudioHub.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\studio-hub.exe"; Description: "{cm:LaunchProgram,Studio Hub}"; Flags: nowait postinstall skipifsilent
