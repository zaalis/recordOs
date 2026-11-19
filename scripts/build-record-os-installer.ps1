param([switch]$SkipAppBuild, [switch]$SignUpdates)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
$desktop = Join-Path $repo 'apps/desktop'
$source = Join-Path $repo 'installer/source'
$framework = 'C:/Windows/Microsoft.NET/Framework64/v4.0.30319'
$conf = Get-Content (Join-Path $desktop 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json
$version = $conf.version
if ($version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$' -or $conf.productName -ne 'Record OS') { throw 'Version/name mismatch' }
$output = Join-Path $repo "installer/Record-OS-$version"
if (!$SkipAppBuild) {
    & (Join-Path $PSScriptRoot 'build-installer-art.ps1')
    Push-Location $repo
    try { & npm.cmd run build -w '@vision-pro/ipad'; if ($LASTEXITCODE) { throw 'iPad build failed' } } finally { Pop-Location }
    Push-Location $desktop
    try {
        $tauriArgs = @('tauri', 'build', '--target', 'x86_64-pc-windows-msvc')
        if ($SignUpdates) { $tauriArgs += @('--config', 'src-tauri/tauri.release.conf.json') }
        & npx.cmd @tauriArgs
        if ($LASTEXITCODE) { throw 'Tauri build failed' }
    } finally { Pop-Location }
}
$target = Join-Path $desktop 'src-tauri/target/x86_64-pc-windows-msvc/release'
$payload = Join-Path $target "bundle/nsis/Record OS_${version}_x64-setup.exe"
if (!(Test-Path -LiteralPath $payload)) { throw "Missing NSIS payload: $payload" }
if ($SignUpdates -and !(Test-Path -LiteralPath "$payload.sig")) { throw 'Missing updater signature' }
New-Item -ItemType Directory -Force -Path $output | Out-Null
$hashFile = Join-Path $output 'payload.sha256'
[IO.File]::WriteAllText($hashFile, (Get-FileHash -LiteralPath $payload -Algorithm SHA256).Hash)
$setup = Join-Path $output "Record-OS_${version}_x64-setup.exe"
$versionSource = Join-Path ([IO.Path]::GetTempPath()) ("RecordOS.Version-" + [guid]::NewGuid() + '.cs')
[IO.File]::WriteAllLines($versionSource, @(
    ('[assembly: System.Reflection.AssemblyVersion("{0}.0")]' -f $version),
    ('[assembly: System.Reflection.AssemblyFileVersion("{0}.0")]' -f $version)
))
$compilerArgs = @('/nologo', '/target:winexe', '/platform:x64', '/optimize+', '/utf8output',
    "/out:$setup", "/win32manifest:$(Join-Path $source 'app.manifest')",
    "/win32icon:$(Join-Path $desktop 'src-tauri/icons/icon.ico')",
    "/resource:$(Join-Path $source 'RecordOS.Setup.xaml'),layout.xaml",
    "/resource:$(Join-Path $desktop 'src-tauri/icons/128x128@2x.png'),logo.png",
    "/resource:$payload,payload.exe", "/resource:$hashFile,payload.sha256",
    "/reference:$framework/WPF/PresentationFramework.dll", "/reference:$framework/WPF/PresentationCore.dll",
    "/reference:$framework/WPF/WindowsBase.dll", "/reference:$framework/System.Xaml.dll",
    "/reference:$framework/System.Windows.Forms.dll", "/reference:$framework/System.Drawing.dll",
    (Join-Path $source 'RecordOS.Setup.cs'), $versionSource)
try {
    & (Join-Path $framework 'csc.exe') @compilerArgs
    if ($LASTEXITCODE) { throw 'Custom installer compilation failed' }
} finally {
    Remove-Item -LiteralPath $versionSource -ErrorAction SilentlyContinue
}
Copy-Item -LiteralPath (Join-Path $target 'vision-pro.exe') -Destination (Join-Path $output 'Record OS.exe') -Force
$report = Join-Path $output 'payload-verification.txt'
$process = Start-Process -FilePath $setup -ArgumentList @('--verify-payload', "`"$report`"") -WindowStyle Hidden -PassThru -Wait
if ($process.ExitCode -ne 0 -or !(Test-Path -LiteralPath $report) -or (Get-Content -LiteralPath $report)[1] -ne "Version: $version") { throw 'Embedded payload verification failed' }
$sums = @("Record-OS_${version}_x64-setup.exe", 'Record OS.exe') | ForEach-Object { "$( (Get-FileHash -LiteralPath (Join-Path $output $_) -Algorithm SHA256).Hash )  $_" }
$sums | Set-Content (Join-Path $output 'SHA256SUMS.txt') -Encoding utf8
Write-Output "Installer ready: $setup"
