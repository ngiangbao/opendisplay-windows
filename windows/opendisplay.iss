; Inno Setup script for OpenDisplay (Windows): ONE executable, receiver + sender.
; Built by build-installer.ps1.
#ifndef AppVersion
  #define AppVersion "0.2.0"
#endif
#ifndef ExePath
  #define ExePath "C:\odt\release\opendisplay.exe"
#endif

[Setup]
AppId={{6C1B3E52-8D54-4C0B-9B0F-0D1500A1B201}
AppName=OpenDisplay
AppVersion={#AppVersion}
AppPublisher=OpenDisplay (unofficial Windows port)
AppPublisherURL=https://github.com/ngiangbao/opendisplay-windows
DefaultDirName={autopf}\OpenDisplay
DefaultGroupName=OpenDisplay
OutputBaseFilename=OpenDisplay-Setup-{#AppVersion}
OutputDir=output
Compression=lzma2
SolidCompression=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0
LicenseFile=..\LICENSE
UninstallDisplayName=OpenDisplay

[Tasks]
Name: "firewall"; Description: "Allow OpenDisplay through Windows Firewall on private and domain networks (TCP 9000, UDP 9001, mDNS)"; GroupDescription: "Network:"
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[InstallDelete]
; Remove the files of the old two-part 0.1.x installer when upgrading in place.
Type: files; Name: "{app}\od-receiver.exe"

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\OpenDisplay"; Filename: "{app}\opendisplay.exe"
Name: "{group}\OpenDisplay (fullscreen display)"; Filename: "{app}\opendisplay.exe"; Parameters: "receive"
Name: "{autodesktop}\OpenDisplay"; Filename: "{app}\opendisplay.exe"; Tasks: desktopicon

[Run]
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""OpenDisplay Receiver"""; Flags: runhidden; Tasks: firewall
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""OpenDisplay"""; Flags: runhidden; Tasks: firewall
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""OpenDisplay"" dir=in action=allow program=""{app}\opendisplay.exe"" enable=yes profile=private,domain"; Flags: runhidden; Tasks: firewall
Filename: "{app}\opendisplay.exe"; Description: "Start OpenDisplay"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""OpenDisplay"""; Flags: runhidden; RunOnceId: "DelFirewall"
