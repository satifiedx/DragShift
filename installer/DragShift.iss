; Inno Setup script: per-user install, no admin rights needed. Built by packaging\build.ps1.
#define AppVersion GetEnv("DS_VERSION")
#if AppVersion == ""
  #define AppVersion "2.2.0"
#endif

[Setup]
AppId={{6C1E2B0A-7D5F-4E8B-9F3A-D7A5F1C2B3E4}
AppName=DragShift
AppVersion={#AppVersion}
AppVerName=DragShift {#AppVersion}
AppPublisher=DragShift
AppPublisherURL=https://github.com/satifiedx/DragShift
AppSupportURL=https://github.com/satifiedx/DragShift/issues
DefaultDirName={localappdata}\Programs\DragShift
DefaultGroupName=DragShift
DisableProgramGroupPage=yes
DisableDirPage=auto
PrivilegesRequired=lowest
OutputDir=..\dist
OutputBaseFilename=DragShift-{#AppVersion}-Setup
SetupIconFile=..\dragshift\assets\dragshift.ico
UninstallDisplayIcon={app}\DragShift.exe
UninstallDisplayName=DragShift
Compression=lzma2/ultra64
SolidCompression=yes
WizardStyle=modern
CloseApplications=force
CloseApplicationsFilter=DragShift.exe,DragShiftUI.exe,ffmpeg.exe
RestartApplications=no
ArchitecturesAllowed=x64compatible
ArchitecturesInstallIn64BitMode=x64compatible
MinVersion=10.0

[Languages]
Name: "en"; MessagesFile: "compiler:Default.isl"
Name: "ru"; MessagesFile: "compiler:Languages\Russian.isl"
Name: "uk"; MessagesFile: "compiler:Languages\Ukrainian.isl"

[CustomMessages]
en.Autostart=Start DragShift when I sign in to Windows
ru.Autostart=Запускать DragShift при входе в Windows
uk.Autostart=Запускати DragShift під час входу в Windows
en.ContextMenu=Add "DragShift…" to the right-click menu for files
ru.ContextMenu=Добавить «DragShift…» в контекстное меню файлов
uk.ContextMenu=Додати «DragShift…» у контекстне меню файлів
en.Desktop=Create a desktop shortcut
ru.Desktop=Создать ярлык на рабочем столе
uk.Desktop=Створити ярлик на робочому столі
en.LaunchNow=Launch DragShift now
ru.LaunchNow=Запустить DragShift сейчас
uk.LaunchNow=Запустити DragShift зараз

[Tasks]
Name: "autostart"; Description: "{cm:Autostart}"; Flags: checkedonce
Name: "contextmenu"; Description: "{cm:ContextMenu}"; Flags: checkedonce
Name: "desktopicon"; Description: "{cm:Desktop}"; Flags: unchecked

[InstallDelete]
; files of DragShift 1.x (one big Python app) that 2.x no longer uses
Type: filesandordirs; Name: "{app}\_internal"

[Files]
Source: "..\dist\DragShift\*"; DestDir: "{app}"; Flags: recursesubdirs ignoreversion

[Icons]
Name: "{autoprograms}\DragShift"; Filename: "{app}\DragShift.exe"
Name: "{autoprograms}\DragShift Settings"; Filename: "{app}\DragShift.exe"; Parameters: "--settings"
Name: "{autodesktop}\DragShift"; Filename: "{app}\DragShift.exe"; Tasks: desktopicon
Name: "{usersendto}\DragShift"; Filename: "{app}\DragShift.exe"

[Registry]
Root: HKCU; Subkey: "Software\Microsoft\Windows\CurrentVersion\Run"; ValueType: string; ValueName: "DragShift"; ValueData: """{app}\DragShift.exe"" --quiet"; Tasks: autostart; Flags: uninsdeletevalue
Root: HKCU; Subkey: "Software\Classes\*\shell\DragShift"; ValueType: string; ValueName: "MUIVerb"; ValueData: "DragShift…"; Tasks: contextmenu; Flags: uninsdeletekey
Root: HKCU; Subkey: "Software\Classes\*\shell\DragShift"; ValueType: string; ValueName: "Icon"; ValueData: "{app}\DragShift.exe"; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\*\shell\DragShift"; ValueType: string; ValueName: "MultiSelectModel"; ValueData: "Player"; Tasks: contextmenu
Root: HKCU; Subkey: "Software\Classes\*\shell\DragShift\command"; ValueType: string; ValueName: ""; ValueData: """{app}\DragShift.exe"" ""%1"""; Tasks: contextmenu
Root: HKCU; Subkey: "Software\DragShift"; Flags: uninsdeletekey

[Run]
Filename: "{app}\DragShift.exe"; Description: "{cm:LaunchNow}"; Flags: nowait postinstall skipifsilent

[UninstallRun]
; ask the running copy to quit (it cancels jobs and removes its tray icon), then make sure
Filename: "{app}\DragShift.exe"; Parameters: "--quit"; Flags: runhidden waituntilterminated; RunOnceId: "QuitDragShift"
Filename: "{sys}\taskkill.exe"; Parameters: "/F /T /IM DragShift.exe"; Flags: runhidden; RunOnceId: "KillDragShift"

[UninstallDelete]
Type: filesandordirs; Name: "{localappdata}\DragShift"
