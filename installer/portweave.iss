#ifndef AppVersion
  #error AppVersion is required
#endif
#ifndef BinarySourceDir
  #error BinarySourceDir is required
#endif
#ifndef OutputDir
  #error OutputDir is required
#endif

[Setup]
; Keep this ID stable so subsequent releases upgrade the same installation.
AppId={{953643AA-87C9-4D90-A93D-CB5CA6265D49}
AppName=PortWeave
AppVersion={#AppVersion}
AppPublisher=PortWeave
AppPublisherURL=https://github.com/yulianjie/ssh-port-mapping
DefaultDirName={localappdata}\Programs\PortWeave
PrivilegesRequired=lowest
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
DisableProgramGroupPage=yes
OutputDir={#OutputDir}
OutputBaseFilename=PortWeave-{#AppVersion}-windows-x86_64-Setup
SetupIconFile=..\assets\portweave.ico
UninstallDisplayIcon={app}\portweave.exe
LicenseFile=..\LICENSE
Compression=lzma2
SolidCompression=yes
WizardStyle=modern
CloseApplications=yes
RestartApplications=no

[Tasks]
Name: "desktopicon"; Description: "Create a &desktop shortcut"; Flags: unchecked

[Files]
Source: "{#BinarySourceDir}\portweave.exe"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\README.md"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\PortWeave"; Filename: "{app}\portweave.exe"
Name: "{userdesktop}\PortWeave"; Filename: "{app}\portweave.exe"; Tasks: desktopicon

[Run]
Filename: "{app}\portweave.exe"; Description: "Launch PortWeave"; Flags: nowait postinstall skipifsilent unchecked

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Command: String;
begin
  if CurUninstallStep = usUninstall then
  begin
    // Remove only this installation's startup entry, not a portable copy's.
    if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run',
      'PortWeave', Command) then
      if CompareText(Trim(Command), '"' + ExpandConstant('{app}\portweave.exe') + '"') = 0 then
        RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'PortWeave');
  end;
end;
