; Inno Setup script for the OpenDisplay Receiver (Windows). Built by build-installer.ps1.
#ifndef AppVersion
  #define AppVersion "0.1.0"
#endif
#ifndef ExePath
  #define ExePath "..\..\..\odt\release\od-receiver.exe"
#endif

[Setup]
AppId={{6C1B3E52-8D54-4C0B-9B0F-0D1500A1B201}
AppName=OpenDisplay Receiver
AppVersion={#AppVersion}
AppPublisher=OpenDisplay (unofficial Windows port)
AppPublisherURL=https://github.com/ngiangbao/opendisplay-windows
DefaultDirName={autopf}\OpenDisplay Receiver
DefaultGroupName=OpenDisplay Receiver
OutputBaseFilename=OpenDisplay-Receiver-Setup-{#AppVersion}
OutputDir=output
Compression=lzma2
SolidCompression=yes
PrivilegesRequired=admin
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
LicenseFile=..\LICENSE
UninstallDisplayName=OpenDisplay Receiver

[Tasks]
Name: "firewall"; Description: "Allow OpenDisplay through Windows Firewall (TCP 9000, UDP 9001, mDNS)"; GroupDescription: "Network:"
Name: "desktopicon"; Description: "Create a desktop shortcut"; GroupDescription: "Shortcuts:"; Flags: unchecked

[Files]
Source: "{#ExePath}"; DestDir: "{app}"; Flags: ignoreversion
Source: "..\LICENSE"; DestDir: "{app}"; Flags: ignoreversion

[Icons]
Name: "{group}\OpenDisplay Receiver (fullscreen)"; Filename: "{app}\od-receiver.exe"
Name: "{group}\OpenDisplay Receiver (window)"; Filename: "{app}\od-receiver.exe"; Parameters: "--windowed"
Name: "{autodesktop}\OpenDisplay Receiver"; Filename: "{app}\od-receiver.exe"; Parameters: "--windowed"; Tasks: desktopicon

[Run]
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall add rule name=""OpenDisplay Receiver"" dir=in action=allow program=""{app}\od-receiver.exe"" enable=yes profile=private,domain"; Flags: runhidden; Tasks: firewall
Filename: "{app}\od-receiver.exe"; Parameters: "--windowed"; Description: "Start OpenDisplay Receiver"; Flags: nowait postinstall skipifsilent

[UninstallRun]
Filename: "{sys}\netsh.exe"; Parameters: "advfirewall firewall delete rule name=""OpenDisplay Receiver"""; Flags: runhidden; RunOnceId: "DelFirewall"
