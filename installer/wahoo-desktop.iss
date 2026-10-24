; Built by CI: ISCC /DAppVersion=1.2.3 installer\wahoo-desktop.iss
#ifndef AppVersion
  #define AppVersion "0.0.0"
#endif

#ifndef Arch
  #define Arch "x64"
#endif
#ifndef BinDir
  #define BinDir "..\target\release"
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
OutputBaseFilename=WahooDesktop-Setup-{#AppVersion}-{#Arch}
Compression=lzma2
SolidCompression=yes
ArchitecturesAllowed={#Arch}compatible
ArchitecturesInstallIn64BitMode={#Arch}compatible
PrivilegesRequired=lowest
PrivilegesRequiredOverridesAllowed=dialog
WizardStyle=modern

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; Flags: unchecked

[Files]
Source: "{#BinDir}\wahoo-desktop.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "{#BinDir}\wahoo_cli.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\Wahoo Desktop"; Filename: "{app}\wahoo-desktop.exe"
Name: "{autodesktop}\Wahoo Desktop"; Filename: "{app}\wahoo-desktop.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\wahoo-desktop.exe"; Description: "Launch Wahoo Desktop"; Flags: nowait postinstall skipifsilent
