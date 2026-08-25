<#
.SYNOPSIS
  Builds the Windows installer for Quill, and optionally installs it.

.DESCRIPTION
  Goes from a checkout to a file that can be handed to somebody: find or install the Inno Setup
  compiler, build the release binary, read the version out of Cargo.toml, and compile
  installer\windows\quill.iss into installer\dist\QuillSetup-<version>-x64.exe.

  Anything a person would otherwise have to remember is in here rather than in a document.

.PARAMETER SkipBuild
  Use the binary already in target\release rather than running cargo.

.PARAMETER Icon
  Redraw installer\icon\quill.ico, quill.icns and Quill.iconset before building. The drawn files are
  committed, so this is only needed after changing the drawing.

.PARAMETER Install
  Run the installer that was just built, silently, with every optional task switched on. This is how
  Quill is installed on this machine and how it is upgraded.

.PARAMETER AllUsers
  Install for every user, into Program Files, rather than for this user into %LOCALAPPDATA%. Needs
  administrator rights. Only meaningful with -Install.

.EXAMPLE
  pwsh installer\windows\build.ps1
  powershell -File installer\windows\build.ps1 -Install
#>
[CmdletBinding()]
param(
    [switch] $SkipBuild,
    [switch] $Icon,
    [switch] $Install,
    [switch] $AllUsers
)

$ErrorActionPreference = 'Stop'

$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$Repo = Resolve-Path (Join-Path $Here '..\..')
$Dist = Join-Path $Repo 'installer\dist'
$Script = Join-Path $Here 'quill.iss'
$BinaryDir = Join-Path $Repo 'target\release'

function Write-Step([string] $Message) {
    Write-Host ''
    Write-Host "==> $Message" -ForegroundColor Cyan
}

<#
.SYNOPSIS
  The path to the Inno Setup command line compiler, installing Inno Setup first if it is missing.
.DESCRIPTION
  Inno Setup is the one thing this build needs that is not already on a machine that can build Quill.
  Rather than telling the reader to go and install it, it is installed here with winget, which is on
  every Windows 10 and 11. It is looked for in both the per user and the per machine location,
  because winget will have chosen one of them.
#>
function Get-InnoSetupCompiler {
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA 'Programs\Inno Setup 6\ISCC.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'Inno Setup 6\ISCC.exe'),
        (Join-Path $env:ProgramFiles 'Inno Setup 6\ISCC.exe')
    )
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) { return $candidate }
    }

    $onPath = Get-Command 'iscc' -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }

    Write-Step 'Installing Inno Setup, which is not on this machine'
    $winget = Get-Command 'winget' -ErrorAction SilentlyContinue
    if (-not $winget) {
        throw 'Inno Setup is not installed and winget is not available to install it. Install Inno Setup 6 from https://jrsoftware.org/isdl.php and run this again.'
    }
    & winget install --id JRSoftware.InnoSetup --exact --silent `
        --accept-source-agreements --accept-package-agreements --disable-interactivity
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) { return $candidate }
    }
    throw 'Inno Setup was installed but ISCC.exe could not be found afterwards.'
}

<#
.SYNOPSIS
  The version in Cargo.toml, which is the one place a version is written down.
.DESCRIPTION
  It reaches the exe's version block, the installer's file name, the Add or Remove Programs entry and
  the macOS bundle from here. `cargo metadata` is asked rather than the file being parsed, so that a
  workspace inheriting its version still answers correctly.
#>
function Get-QuillVersion {
    $json = & cargo metadata --no-deps --format-version 1 --manifest-path (Join-Path $Repo 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed.' }
    $metadata = $json | ConvertFrom-Json
    $package = $metadata.packages | Where-Object { $_.name -eq 'quill-app' }
    if (-not $package) { throw 'quill-app is not in the workspace metadata.' }
    return $package.version
}

# ---------------------------------------------------------------------------------------------

if ($Icon) {
    Write-Step 'Drawing the icon'
    & cargo run --release --manifest-path (Join-Path $Repo 'installer\icon\Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw 'The icon generator failed.' }
}

$compiler = Get-InnoSetupCompiler
Write-Host "Inno Setup: $compiler"

$version = Get-QuillVersion
Write-Host "Quill $version"

if (-not $SkipBuild) {
    Write-Step 'Building quill.exe'
    & cargo build --release --manifest-path (Join-Path $Repo 'Cargo.toml') -p quill-app --bin quill
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed.' }
}

$exe = Join-Path $BinaryDir 'quill.exe'
if (-not (Test-Path $exe)) {
    throw "There is no binary at $exe. Run without -SkipBuild."
}

# The installer must not ship an unlabelled executable: if the resource block is missing then the
# Windows SDK was not there when it was built, and the shortcut, the taskbar and Add or Remove
# Programs would all show a generic icon.
$info = (Get-Item $exe).VersionInfo
if ($info.ProductName -ne 'Quill') {
    throw "quill.exe has no version block, so it has no icon either. Install the Windows SDK (it comes with the C++ build tools) and build again."
}

Write-Step 'Compiling the installer'
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
& $compiler `
    "/DAppVersion=$version" `
    "/DBinaryDir=$BinaryDir" `
    "/DOutputDir=$Dist" `
    $Script
if ($LASTEXITCODE -ne 0) { throw 'ISCC failed.' }

$setup = Join-Path $Dist "QuillSetup-$version-x64.exe"
if (-not (Test-Path $setup)) { throw "ISCC reported success but $setup is not there." }
$size = [math]::Round((Get-Item $setup).Length / 1MB, 1)
Write-Host ''
Write-Host "Built $setup ($size MB)" -ForegroundColor Green

<#
.SYNOPSIS
  Closes a copy of Quill already running from the folder that is about to be written to.
.DESCRIPTION
  Quill does not answer the Restart Manager's request to shut down, so a silent install over a
  running copy stops with "Setup was unable to automatically close all applications". The installer
  itself is deliberately left polite about this — CloseApplications=yes asks a person rather than
  killing an editor that may have unsaved changes — so the closing is done here, where it is our own
  automated install doing the asking, and with the window's own close rather than a kill.
#>
function Close-RunningQuill([string] $Folder) {
    if (-not (Test-Path $Folder)) { return }
    $running = Get-Process -Name 'quill' -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith($Folder, [StringComparison]::OrdinalIgnoreCase) }
    if (-not $running) { return }

    Write-Host 'Closing the copy of Quill that is already running'
    foreach ($process in $running) { $process.CloseMainWindow() | Out-Null }

    $deadline = (Get-Date).AddSeconds(20)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 500
        $still = Get-Process -Id ($running | Select-Object -ExpandProperty Id) -ErrorAction SilentlyContinue
        if (-not $still) { return }
    }
    throw 'Quill is running and would not close. Close it and run this again.'
}

if ($Install) {
    Write-Step 'Installing'
    if ($AllUsers) {
        $target = Join-Path $env:ProgramFiles 'Quill'
    } else {
        $target = Join-Path $env:LOCALAPPDATA 'Programs\Quill'
    }
    Close-RunningQuill $target
    # Every optional task, because this is the switch that installs Quill on the machine it was built
    # on and the point is to have all of it.
    $arguments = @(
        '/VERYSILENT',
        '/SUPPRESSMSGBOXES',
        '/NORESTART',
        '/SP-',
        '/TASKS=desktopicon,addtopath,contextfile,contextfolder,associate'
    )
    if ($AllUsers) { $arguments += '/ALLUSERS' } else { $arguments += '/CURRENTUSER' }

    $run = Start-Process -FilePath $setup -ArgumentList $arguments -Wait -PassThru
    if ($run.ExitCode -ne 0) { throw "The installer exited with $($run.ExitCode)." }

    $installed = Join-Path $target 'quill.exe'
    if (Test-Path $installed) {
        Write-Host "Installed $installed" -ForegroundColor Green
    } else {
        Write-Warning "The installer reported success but $installed is not there."
    }
}
