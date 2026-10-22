; Built by CI: ISCC /DAppVersion=1.2.3 installer\wahoo-desktop.iss
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

[Setup]
AppId={{6B0C5B58-5E5F-4B52-9C55-3A5E8D0F7A11}
AppName=Wahoo Desktop
AppVersion={#AppVersion}
AppPublisher=bertt
DefaultDirName={autopf}\Wahoo Desktop
DefaultGroupName=Wahoo Desktop
UninstallDisplayIcon={app}\wahoo-desktop.exe
SetupIconFile=..\assets\icon.ico
OutputDir=..\dist
OutputBaseFilename=WahooDesktop-Setup-{#AppVersion}
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "..\target\release\wahoo-desktop.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\target\release\wahoo_cli.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Wahoo Desktop"; Filename: "{app}\wahoo-desktop.exe"
Name: "{autodesktop}\Wahoo Desktop"; Filename: "{app}\wahoo-desktop.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\wahoo-desktop.exe"; Description: "Launch Wahoo Desktop"; Flags: nowait postinstall skipifsilent
