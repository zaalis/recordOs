param([switch]$SkipAppBuild)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$desktop = Join-Path $repo 'apps/desktop'
$source = Join-Path $repo 'installer/source'
$output = Join-Path $repo 'installer/Record-OS-0.1.4'
$framework = 'C:/Windows/Microsoft.NET/Framework64/v4.0.30319'
$conf = Get-Content (Join-Path $desktop 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
if ($conf.version -ne '0.1.4' -or $conf.productName -ne 'Record OS') { throw 'Version/name mismatch' }
if (!$SkipAppBuild) {
    & (Join-Path $PSScriptRoot 'build-installer-art.ps1')
    Push-Location $repo
    try { & npm.cmd run build -w '@vision-pro/ipad'; if ($LASTEXITCODE) { throw 'iPad build failed' } } finally { Pop-Location }
    Push-Location $desktop
    try { & npx.cmd tauri build; if ($LASTEXITCODE) { throw 'Tauri build failed' } } finally { Pop-Location }
}
$payload = Join-Path $desktop 'src-tauri/target/release/bundle/nsis/Record OS_0.1.4_x64-setup.exe'
if (!(Test-Path -LiteralPath $payload)) { throw "Missing NSIS payload: $payload" }
New-Item -ItemType Directory -Force -Path $output | Out-Null
$hashFile = Join-Path $output 'payload.sha256'
[IO.File]::WriteAllText($hashFile, (Get-FileHash -LiteralPath $payload -Algorithm SHA256).Hash)
$setup = Join-Path $output 'Record-OS_0.1.4_x64-setup.exe'
$compilerArgs = @('/nologo', '/target:winexe', '/platform:x64', '/optimize+', '/utf8output',
    "/out:$setup", "/win32manifest:$(Join-Path $source 'app.manifest')",
    "/win32icon:$(Join-Path $desktop 'src-tauri/icons/icon.ico')",
    "/resource:$(Join-Path $source 'RecordOS.Setup.xaml'),layout.xaml",
    "/resource:$(Join-Path $desktop 'src-tauri/icons/128x128@2x.png'),logo.png",
    "/resource:$payload,payload.exe", "/resource:$hashFile,payload.sha256",
    "/reference:$framework/WPF/PresentationFramework.dll", "/reference:$framework/WPF/PresentationCore.dll",
    "/reference:$framework/WPF/WindowsBase.dll", "/reference:$framework/System.Xaml.dll",
    "/reference:$framework/System.Windows.Forms.dll", "/reference:$framework/System.Drawing.dll",
    (Join-Path $source 'RecordOS.Setup.cs'))
& (Join-Path $framework 'csc.exe') @compilerArgs
if ($LASTEXITCODE) { throw 'Custom installer compilation failed' }
Copy-Item -LiteralPath (Join-Path $desktop 'src-tauri/target/release/vision-pro.exe') -Destination (Join-Path $output 'Record OS.exe') -Force
$report = Join-Path $output 'payload-verification.txt'
$process = Start-Process -FilePath $setup -ArgumentList @('--verify-payload', "`"$report`"") -WindowStyle Hidden -PassThru -Wait
if ($process.ExitCode -ne 0 -or !(Test-Path -LiteralPath $report)) { throw 'Embedded payload verification failed' }
$sums = @('Record-OS_0.1.4_x64-setup.exe', 'Record OS.exe') | ForEach-Object { "$( (Get-FileHash -LiteralPath (Join-Path $output $_) -Algorithm SHA256).Hash )  $_" }
$sums | Set-Content (Join-Path $output 'SHA256SUMS.txt') -Encoding utf8
Write-Output "Installer ready: $setup"
