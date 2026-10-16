<#
.SYNOPSIS
  Build the Windows suite: Studio Hub + the seven apps in one installer (the Windows twin of make-dmg.sh).

.DESCRIPTION
  1. builds studio-hub.exe (static C runtime: no VC++ redistributable needed)
  2. downloads each app's official portable zip (versions in packaging/windows/apps.txt), cached
  3. stages   <stage>\studio-hub.exe, <stage>\apps\<id>\<id>.exe + <id>-cli.exe + licences
  4. writes   dist\Studio-Suite-<version>-windows-x64-setup.exe  (Inno Setup, per-user, no admin)
              dist\Studio-Suite-<version>-windows-x64.zip        (portable: unzip and run studio-hub.exe)

  Needs: Rust (MSVC toolchain), the Windows SDK (rc.exe, for the exe icon) and Inno Setup 6 (iscc.exe).

.EXAMPLE
  pwsh scripts/make-windows.ps1
  pwsh scripts/make-windows.ps1 -SkipBuild -NoInstaller
#>
param(
  [ValidateSet('x64', 'arm64')] [string] $Arch = 'x64',
  [switch] $SkipBuild,
  [switch] $NoInstaller
)
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'   # Invoke-WebRequest is many times slower with the progress bar
$Hub = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
Set-Location $Hub

$Version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$Target = @{ x64 = 'x86_64-pc-windows-msvc'; arm64 = 'aarch64-pc-windows-msvc' }[$Arch]
$Work = Join-Path $Hub 'target\suite-win'
$Stage = Join-Path $Work 'stage'
$Cache = Join-Path $Work 'downloads'
$Dist = Join-Path $Hub 'dist'
Write-Output "Studio Suite $Version for Windows $Arch"

if (-not $SkipBuild) {
  $flagVar = 'CARGO_TARGET_' + ($Target.ToUpper() -replace '-', '_') + '_RUSTFLAGS'
  [Environment]::SetEnvironmentVariable($flagVar, '-C target-feature=+crt-static')
  Write-Output "==> cargo build ($Target)"
  cargo build --release --locked --target $Target
  if ($LASTEXITCODE -ne 0) { throw "cargo build failed ($LASTEXITCODE)" }
}

if (Test-Path $Stage) { Remove-Item -Recurse -Force $Stage }
New-Item -ItemType Directory -Force -Path $Stage, $Cache, $Dist, (Join-Path $Stage 'apps') | Out-Null
Copy-Item (Join-Path $Hub "target\$Target\release\studio-hub.exe") $Stage
Copy-Item (Join-Path $Hub 'assets\icons\StudioHub.ico') $Stage

$apps = Get-Content packaging\windows\apps.txt | Where-Object { $_ -match '^\s*[a-z]' } | ForEach-Object {
  $id, $ver = -split $_; [pscustomobject]@{ Id = $id; Version = $ver }
}
foreach ($a in $apps) {
  $name = "$($a.Id)-$($a.Version)-windows-$Arch-portable"
  $zip = Join-Path $Cache "$name.zip"
  if (-not (Test-Path $zip)) {
    $url = "https://github.com/storytold/$($a.Id)/releases/download/v$($a.Version)/$name.zip"
    Write-Output "==> downloading $url"
    Invoke-WebRequest -Uri $url -OutFile "$zip.part"
    Move-Item "$zip.part" $zip
  }
  $unz = Join-Path $Work "unzip\$($a.Id)"
  if (Test-Path $unz) { Remove-Item -Recurse -Force $unz }
  Expand-Archive -Path $zip -DestinationPath $unz
  # The zip holds one folder (<name>\...); take its contents.
  $src = Get-ChildItem $unz -Directory | Select-Object -First 1
  if (-not $src) { $src = Get-Item $unz }
  $dest = Join-Path $Stage "apps\$($a.Id)"
  Copy-Item -Recurse $src.FullName $dest
  # portable.txt would keep the app's settings next to the exe (inside the install folder); without it
  # every app uses its normal per-user folder in %APPDATA%.
  Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $dest 'portable.txt')
  foreach ($exe in "$($a.Id).exe", "$($a.Id)-cli.exe") {
    if (-not (Test-Path (Join-Path $dest $exe))) { throw "$name.zip has no $exe" }
  }
  Write-Output "    $($a.Id) $($a.Version)"
}

@"
Studio Suite $Version for Windows

studio-hub.exe   the hub: opens the apps with AI control and connects Claude Code, Codex and Claude Desktop
apps\<app>\      the seven apps, unchanged official builds from github.com/storytold (MIT OR Apache-2.0;
                 each folder carries that app's licence files)

AI tools run "studio-hub.exe mcp". Use the Connect buttons in the hub window, or add it by hand:
  claude mcp add --scope user studio-hub -- "<this folder>\studio-hub.exe" mcp
"@ | Set-Content -Encoding utf8 (Join-Path $Stage 'README.txt')

# Check the hub sees all seven apps from the staged layout before packaging it.
$status = & (Join-Path $Stage 'studio-hub.exe') status | Out-String | ConvertFrom-Json
$missing = @($status.apps | Where-Object { -not $_.installed } | ForEach-Object { $_.app })
if ($missing.Count) { throw "studio-hub does not see: $($missing -join ', ')" }
Write-Output "==> studio-hub sees all $(@($status.apps).Count) apps"

$PortableZip = Join-Path $Dist "Studio-Suite-$Version-windows-$Arch.zip"
if (Test-Path $PortableZip) { Remove-Item $PortableZip }
Compress-Archive -Path (Join-Path $Stage '*') -DestinationPath $PortableZip -CompressionLevel Optimal
Write-Output "==> $PortableZip"

if (-not $NoInstaller) {
  $iscc = (Get-Command iscc.exe -ErrorAction SilentlyContinue).Source
  if (-not $iscc) { $iscc = @("${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe", "$env:ProgramFiles\Inno Setup 6\ISCC.exe", "$env:LOCALAPPDATA\Programs\Inno Setup 6\ISCC.exe") | Where-Object { Test-Path $_ } | Select-Object -First 1 }
  if (-not $iscc) { throw 'Inno Setup 6 (iscc.exe) not found: winget install JRSoftware.InnoSetup, or pass -NoInstaller' }
  & $iscc "/DAppVersion=$Version" "/DArch=$Arch" "/DStage=$Stage" "/DOutDir=$Dist" packaging\windows\studio-suite.iss
  if ($LASTEXITCODE -ne 0) { throw "iscc failed ($LASTEXITCODE)" }
  Write-Output "==> $(Join-Path $Dist "Studio-Suite-$Version-windows-$Arch-setup.exe")"
}
