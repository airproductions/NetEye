; NetEye installer (Inno Setup 6).
; Build: iscc /DAppVersion=1.0.0 /DSourceExe=..\target\release\neteye.exe installer\neteye.iss
; The release workflow (.github/workflows/release.yml) passes both from the tag and the build.

#ifndef AppVersion
  #define AppVersion "1.0.0"
#endif
#ifndef SourceExe
  #define SourceExe "..\target\release\neteye.exe"
#endif

[Setup]
AppId={{8F1C2A4E-5B3D-4E7A-9C61-0F3E7A2B9D30}
AppName=NetEye
AppVersion={#AppVersion}
AppVerName=NetEye {#AppVersion}
AppPublisher=NetEye contributors
AppPublisherURL=https://github.com/airproductions/NetEye
AppSupportURL=https://github.com/airproductions/NetEye/issues
AppUpdatesURL=https://github.com/airproductions/NetEye/releases
DefaultDirName={autopf}\NetEye
DefaultGroupName=NetEye
DisableProgramGroupPage=yes
; firewall rules and per-app traffic need administrator rights anyway
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
OutputDir=..\dist
OutputBaseFilename=NetEye-Setup-{#AppVersion}
SetupIconFile=..\assets\neteye.ico
UninstallDisplayIcon={app}\neteye.ico
UninstallDisplayName=NetEye
LicenseFile=..\LICENSE
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
; ask to close a running NetEye before installing over it or removing it
AppMutex=Local\NetEyeNetworkMonitor
CloseApplications=yes

[Tasks]
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "{#SourceExe}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\assets\neteye.ico"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; DestName: "LICENSE.txt"; Flags: ignoreversion
; bundled data and font licences travel with the program
Source: "..\assets\geo\DBIP-LICENSE.txt"; DestDir: "{app}\licenses"; DestName: "DB-IP (country data).txt"; Flags: ignoreversion
Source: "..\assets\flags\LICENSE"; DestDir: "{app}\licenses"; DestName: "flag-icons.txt"; Flags: ignoreversion
Source: "..\assets\fonts\OFL.txt"; DestDir: "{app}\licenses"; DestName: "Manrope font (OFL).txt"; Flags: ignoreversion

[Icons]
Name: "{autoprograms}\NetEye"; Filename: "{app}\neteye.exe"; IconFilename: "{app}\neteye.ico"; Comment: "Network monitor and per-app firewall"
Name: "{autodesktop}\NetEye"; Filename: "{app}\neteye.exe"; IconFilename: "{app}\neteye.ico"; Tasks: desktopicon

[Run]
Filename: "{app}\neteye.exe"; Description: "Start NetEye now"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; remove every firewall rule NetEye added and its startup entry, so nothing stays blocked
Filename: "{app}\neteye.exe"; Parameters: "--uninstall-cleanup"; RunOnceId: "NetEyeCleanup"; Flags: runhidden waituntilterminated

[Code]
procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Data: String;
begin
  if CurUninstallStep = usPostUninstall then
  begin
    Data := ExpandConstant('{userappdata}\NetEye');
    if DirExists(Data) and not UninstallSilent then
      if MsgBox('Also delete your NetEye history and settings?' + #13#10 + Data, mbConfirmation, MB_YESNO or MB_DEFBUTTON2) = IDYES then
        DelTree(Data, True, True, True);
  end;
end;
