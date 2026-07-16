#ifndef MyAppVersion
  #define MyAppVersion "0.5.0"
#endif

[Setup]
AppId={{4CCF0AF7-8F6A-4EF2-B9BC-90AA2C6E2521}
AppName=Yeet
AppVersion={#MyAppVersion}
AppPublisher=hjosugi
AppPublisherURL=https://github.com/hjosugi/yeet
DefaultDirName={autopf}\Yeet
DefaultGroupName=Yeet
UninstallDisplayIcon={app}\yeet.exe
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
PrivilegesRequired=lowest
OutputDir=..\..
OutputBaseFilename=yeet-{#MyAppVersion}-windows-x64-setup
Compression=lzma2
SolidCompression=yes
WizardStyle=modern

[Files]
Source: "..\..\yeet-{#MyAppVersion}-windows-x64\*"; DestDir: "{app}"; Flags: ignoreversion recursesubdirs createallsubdirs

[Icons]
Name: "{group}\Yeet"; Filename: "{app}\yeet.exe"
Name: "{userdesktop}\Yeet"; Filename: "{app}\yeet.exe"; Tasks: desktopicon

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Additional shortcuts:"; Flags: unchecked

[Run]
Filename: "{app}\yeet.exe"; Description: "Launch Yeet"; Flags: nowait postinstall skipifsilent
