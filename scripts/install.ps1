# Installs Warble for this Windows user: copies the app to a permanent folder and adds a Start
# menu shortcut, which is how Start search, PowerToys Run and Raycast find apps.
#
# From the repo, after `cargo build --release -p warble`:
#   powershell -ExecutionPolicy Bypass -File scripts\install.ps1
# Or from a folder holding warble.exe, warble.ico and this script (what friends get).
# Run it again after each update.

$ErrorActionPreference = 'Stop'

$exe = Join-Path $PSScriptRoot 'warble.exe'
$ico = Join-Path $PSScriptRoot 'warble.ico'
if (-not (Test-Path $exe)) {
    $repo = Split-Path -Parent $PSScriptRoot
    $exe = Join-Path $repo 'target\release\warble.exe'
    $ico = Join-Path $repo 'crates\app\assets\warble.ico'
}
if (-not (Test-Path $exe)) {
    Write-Host "Couldn't find warble.exe. Build it first: cargo build --release -p warble"
    exit 1
}
if (Get-Process warble -ErrorAction SilentlyContinue) {
    Write-Host 'Warble is running. Quit it from its tray icon (right-click, Quit Warble), then run this again.'
    exit 1
}

$dir = Join-Path $env:LOCALAPPDATA 'Programs\Warble'
New-Item -ItemType Directory -Force $dir | Out-Null
Copy-Item $exe, $ico $dir -Force

$lnk = Join-Path ([Environment]::GetFolderPath('Programs')) 'Warble.lnk'
$shortcut = (New-Object -ComObject WScript.Shell).CreateShortcut($lnk)
$shortcut.TargetPath = Join-Path $dir 'warble.exe'
$shortcut.WorkingDirectory = $dir
$shortcut.IconLocation = (Join-Path $dir 'warble.ico') + ',0'
$shortcut.Description = 'Voice changer and soundboard for Discord and game chat'
$shortcut.Save()

Write-Host "Installed Warble to $dir"
Write-Host 'Search for Warble in Start, PowerToys Run or Raycast to open it.'
