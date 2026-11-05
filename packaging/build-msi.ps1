<#
.SYNOPSIS
  Build the Nijikage-Wallpaper release artifacts: MSI installer, portable ZIP and SHA256SUMS.txt.

.PARAMETER WixBin
  Folder containing candle.exe and light.exe of WiX Toolset 3.x
  (e.g. extract wix314-binaries.zip from https://github.com/wixtoolset/wix3/releases).

.PARAMETER Version
  Numeric MSI version, e.g. 0.1.0.

.PARAMETER Label
  Full version label used in file names, e.g. 0.1.0-beta.1.

.PARAMETER OutDir
  Output folder (default: <repo>\dist).

.NOTES
  Honors the CARGO_TARGET_DIR environment variable. Works with Windows PowerShell 5.1 and PowerShell 7.

.EXAMPLE
  powershell -ExecutionPolicy Bypass -File packaging\build-msi.ps1 -WixBin C:\tools\wix314 -Version 0.1.0 -Label 0.1.0-beta.1
#>
param(
    [Parameter(Mandatory = $true)] [string] $WixBin,
    [string] $Version = '0.1.0',
    [string] $Label = '0.1.0-beta.1',
    [string] $OutDir = ''
)
$ErrorActionPreference = 'Stop'
$repo = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
if (-not $OutDir) { $OutDir = Join-Path $repo 'dist' }
$candle = Join-Path $WixBin 'candle.exe'
$light = Join-Path $WixBin 'light.exe'
foreach ($t in @($candle, $light)) {
    if (-not (Test-Path $t)) { throw "Cannot find $t. Point -WixBin at the bin folder of WiX Toolset 3.x." }
}

New-Item -ItemType Directory -Force $OutDir | Out-Null
$work = Join-Path $OutDir 'obj'
New-Item -ItemType Directory -Force $work | Out-Null

Write-Host '==> Building release executables'
Push-Location $repo
cargo build --release
if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
Pop-Location
$targetDir = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $repo 'target' }
$rel = Join-Path $targetDir 'release'

Write-Host '==> Generating the license page (RTF)'
$gpl = [IO.File]::ReadAllText((Join-Path $repo 'LICENSE'))
$esc = $gpl.Replace('\', '\\').Replace('{', '\{').Replace('}', '\}') -replace "`r?`n", "\par`n"
$rtf = "{\rtf1\ansi\deff0{\fonttbl{\f0\fmodern Consolas;}}\f0\fs18 Nijikage-Wallpaper is licensed under GPL-3.0-or-later.\par\par $esc}"
[IO.File]::WriteAllText((Join-Path $repo 'packaging\wix\license.rtf'), $rtf, [Text.Encoding]::ASCII)

Write-Host '==> Compiling the WiX source'
$wixobj = Join-Path $work 'Nijikage-Wallpaper.wixobj'
& $candle -nologo -arch x64 -ext WixUtilExtension -ext WixUIExtension `
    "-dProductVersion=$Version" "-dSourceDir=$rel" "-dRepoDir=$repo" `
    -out $wixobj (Join-Path $repo 'packaging\wix\Nijikage-Wallpaper.wxs')
if ($LASTEXITCODE -ne 0) { throw 'candle failed' }

Write-Host '==> Linking the MSI'
$msi = Join-Path $OutDir "Nijikage-Wallpaper-$Label-x64.msi"
& $light -nologo -ext WixUtilExtension -ext WixUIExtension -sice:ICE91 -cultures:en-us -spdb -out $msi $wixobj
if ($LASTEXITCODE -ne 0) { throw 'light failed' }

Write-Host '==> Creating the portable ZIP'
$stage = Join-Path $work 'portable\Nijikage-Wallpaper'
if (Test-Path $stage) { Remove-Item $stage -Recurse -Force }
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item (Join-Path $rel 'nijikage-wallpaper.exe'), (Join-Path $rel 'nijikage-wallpaper-settings.exe') $stage
Copy-Item (Join-Path $repo 'LICENSE') (Join-Path $stage 'LICENSE.txt')
Copy-Item (Join-Path $repo 'THIRD_PARTY_LICENSES.md') $stage
Copy-Item (Join-Path $repo 'README.md'), (Join-Path $repo 'README.en.md'), (Join-Path $repo 'README.ja.md') $stage
$zip = Join-Path $OutDir "Nijikage-Wallpaper-$Label-x64.zip"
if (Test-Path $zip) { Remove-Item $zip -Force }
Compress-Archive -Path $stage -DestinationPath $zip

Write-Host '==> SHA256SUMS.txt'
$sums = Join-Path $OutDir 'SHA256SUMS.txt'
Get-ChildItem $OutDir -File | Where-Object { $_.Extension -in '.msi', '.zip' } | ForEach-Object {
    '{0}  {1}' -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
} | Set-Content $sums -Encoding ascii
Get-Content $sums
Write-Host "Done: $OutDir"
