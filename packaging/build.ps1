# Builds the Windows release from a clean checkout:
#   dist\DragShift\            the app folder (DragShift.exe, ui\, ffmpeg\)
#   dist\DragShift-Setup.exe   installer (per-user, no admin)
#   dist\DragShift-Portable.zip
# Needs: Rust (stable), Python 3.10+ with requirements.txt + pyinstaller, Inno Setup 6, 7-Zip or tar.
param([string]$Python = "python", [switch]$SkipTests)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# ffmpeg: a pinned gyan.dev "essentials" build, checked against hashes before anything runs it
$FfVersion = "8.0.1"
$FfUrl = "https://github.com/GyanD/codexffmpeg/releases/download/$FfVersion/ffmpeg-$FfVersion-essentials_build.zip"
$FfZipSha = "e2aaeaa0fdbc397d4794828086424d4aaa2102cef1fb6874f6ffd29c0b88b673"
$FfExeSha = "5af82a0d4fe2b9eae211b967332ea97edfc51c6b328ca35b827e73eac560dc0d"

function Step($t) { Write-Host "`n== $t" -ForegroundColor Cyan }
function Check($ok, $msg) { if (-not $ok) { throw $msg } }
# native tools write progress to stderr; Windows PowerShell 5.1 would turn that into errors
function Native([scriptblock]$cmd) {
    $old = $ErrorActionPreference; $ErrorActionPreference = "Continue"
    try { & $cmd 2>&1 | ForEach-Object { "$_" } | Out-Host } finally { $ErrorActionPreference = $old }
}
function Sha($f) { (Get-FileHash -Algorithm SHA256 $f).Hash.ToLower() }

$version = (Select-String -Path native\Cargo.toml -Pattern '^version = "(.+)"').Matches[0].Groups[1].Value
Write-Host "DragShift $version"
$out = "dist\DragShift"
if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Force $out | Out-Null

Step "native daemon"
Push-Location native
Native { cargo build --release }
Check ($LASTEXITCODE -eq 0) "cargo build failed"
if (-not $SkipTests) { Native { cargo test --release }; Check ($LASTEXITCODE -eq 0) "cargo test failed" }
Pop-Location
Copy-Item native\target\release\DragShift.exe $out

Step "editor windows (PyInstaller)"
Native { & $Python -m PyInstaller packaging\DragShiftUI.spec --noconfirm --distpath dist --workpath build --log-level WARN }
Check ($LASTEXITCODE -eq 0) "PyInstaller failed"
Move-Item dist\ui "$out\ui"

Step "ffmpeg $FfVersion"
$cache = "build\ffmpeg-$FfVersion"
$zip = "$cache\ffmpeg.zip"
New-Item -ItemType Directory -Force $cache | Out-Null
if (-not (Test-Path $zip) -or (Sha $zip) -ne $FfZipSha) {
    Invoke-WebRequest -Uri $FfUrl -OutFile $zip -UseBasicParsing
}
Check ((Sha $zip) -eq $FfZipSha) "ffmpeg archive hash mismatch: refusing to use it"
$x = "$cache\x"
if (Test-Path $x) { Remove-Item -Recurse -Force $x }
Expand-Archive -Path $zip -DestinationPath $x
$src = Get-ChildItem $x -Directory | Select-Object -First 1
Check ((Sha "$($src.FullName)\bin\ffmpeg.exe") -eq $FfExeSha) "ffmpeg.exe hash mismatch: refusing to use it"
New-Item -ItemType Directory -Force "$out\ffmpeg" | Out-Null
Copy-Item "$($src.FullName)\bin\ffmpeg.exe" "$out\ffmpeg\"
Copy-Item "$($src.FullName)\LICENSE" "$out\ffmpeg\LICENSE.txt"
@"
ffmpeg $FfVersion "essentials" build by Gyan Doshi (https://www.gyan.dev/ffmpeg/builds/), GPL v3.
Source code: https://github.com/FFmpeg/FFmpeg  -  build configuration: run ffmpeg -version.
DragShift runs it as a separate program; it is not linked into DragShift.
SHA-256 of ffmpeg.exe: $FfExeSha
"@ | Set-Content -Encoding utf8 "$out\ffmpeg\README.txt"
Copy-Item LICENSE "$out\LICENSE.txt"

if (-not $SkipTests) {
    Step "self-tests of the assembled app"
    $env:QT_QPA_PLATFORM = "offscreen"
    $p = Start-Process "$out\ui\DragShiftUI.exe" -ArgumentList "--selftest" -Wait -PassThru -NoNewWindow
    Check ($p.ExitCode -eq 0) "DragShiftUI self-test failed"
    Remove-Item Env:QT_QPA_PLATFORM
    $p = Start-Process "$out\DragShift.exe" -ArgumentList "--selftest" -Wait -PassThru
    Check ($p.ExitCode -eq 0) "DragShift media self-test failed (see %LOCALAPPDATA%\DragShift\log.txt)"
}

Step "installer"
$iscc = @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe") |
    Where-Object { Test-Path $_ } | Select-Object -First 1
Check $iscc "Inno Setup 6 not found"
$env:DS_VERSION = $version
Native { & $iscc /Q installer\DragShift.iss }
Check ($LASTEXITCODE -eq 0) "Inno Setup failed"

Step "portable zip"
$pz = "dist\DragShift-$version-Portable.zip"
if (Test-Path $pz) { Remove-Item $pz }
Compress-Archive -Path $out -DestinationPath $pz -CompressionLevel Optimal

Step "checksums"
$sums = Get-ChildItem dist -File | Where-Object { $_.Name -match '\.(exe|zip)$' } |
    ForEach-Object { "$(Sha $_.FullName)  $($_.Name)" }
$sums | Set-Content -Encoding ascii dist\SHA256SUMS.txt
$sums
Get-ChildItem dist -File | ForEach-Object { "{0,-36} {1,8:N1} MB" -f $_.Name, ($_.Length / 1MB) }
