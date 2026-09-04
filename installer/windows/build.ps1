<#
.SYNOPSIS
  Builds the Windows installer for Unluminate, and optionally installs it.

.DESCRIPTION
  Goes from a checkout to a file that can be handed to somebody: find or install the Inno Setup
  compiler, build the release binary, read the version out of Cargo.toml, and compile
  installer\windows\unluminate.iss into installer\dist\UnluminateSetup-<version>-x64.exe.

  Anything a person would otherwise have to remember is in here rather than in a document.

.PARAMETER SkipBuild
  Use the binary already in target\release rather than running cargo.

.PARAMETER Icon
  Redraw installer\icon\unluminate.ico, unluminate.icns and Unluminate.iconset before building. The drawn files are
  committed, so this is only needed after changing the drawing.

.PARAMETER Install
  Run the installer that was just built, silently, with every optional task switched on. This is how
  Unluminate is installed on this machine and how it is upgraded.

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
$Script = Join-Path $Here 'unluminate.iss'
$BinaryDir = Join-Path $Repo 'target\release'

function Write-Step([string] $Message) {
    Write-Host ''
    Write-Host "==> $Message" -ForegroundColor Cyan
}

<#
.SYNOPSIS
  The path to the Inno Setup command line compiler, installing Inno Setup first if it is missing.
.DESCRIPTION
  Inno Setup is the one thing this build needs that is not already on a machine that can build Unluminate.
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
function Get-UnluminateVersion {
    $json = & cargo metadata --no-deps --format-version 1 --manifest-path (Join-Path $Repo 'Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw 'cargo metadata failed.' }
    $metadata = $json | ConvertFrom-Json
    $package = $metadata.packages | Where-Object { $_.name -eq 'unluminate-app' }
    if (-not $package) { throw 'unluminate-app is not in the workspace metadata.' }
    return $package.version
}

<#
.SYNOPSIS
  The path to signtool.exe, or nothing when the Windows SDK is not installed.
.DESCRIPTION
  It lives under the SDK in a folder named after the SDK version, so the newest one is taken. Not
  installed here the way Inno Setup is: the SDK is a large download and a machine that cannot sign
  should say so rather than fetching two gigabytes without being asked.
#>
function Get-SignTool {
    if ($env:UNLUMINATE_SIGNTOOL -and (Test-Path $env:UNLUMINATE_SIGNTOOL)) {
        return $env:UNLUMINATE_SIGNTOOL
    }
    $roots = @(
        (Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'),
        (Join-Path $env:ProgramFiles 'Windows Kits\10\bin')
    )
    foreach ($root in $roots) {
        if (-not (Test-Path $root)) { continue }
        $found = Get-ChildItem -Path $root -Recurse -Filter 'signtool.exe' -ErrorAction SilentlyContinue |
            Where-Object { $_.FullName -match '\\x64\\' } |
            Sort-Object FullName -Descending |
            Select-Object -First 1
        if ($found) { return $found.FullName }
    }
    return $null
}

<#
.SYNOPSIS
  The signtool command line Inno runs on the uninstaller, or nothing when there is no certificate.
.DESCRIPTION
  `unins000.exe` is built while the setup is compiled and does not exist until it runs, so it is the
  one file `Add-Signature` cannot reach. Inno signs it itself, given a command with `$f` where the
  file goes -- which is what this builds, in exactly the shape `Add-Signature` uses so the two cannot
  come to sign things differently.
#>
function Get-UninstallerSignCommand {
    $thumbprint = $env:UNLUMINATE_SIGN_THUMBPRINT
    $certificate = $env:UNLUMINATE_SIGN_CERT
    if (-not $thumbprint -and -not $certificate) { return $null }
    $signtool = Get-SignTool
    if (-not $signtool) { return $null }
    $timestamp = if ($env:UNLUMINATE_SIGN_TIMESTAMP) { $env:UNLUMINATE_SIGN_TIMESTAMP } else { 'http://timestamp.digicert.com' }
    $parts = @("`"$signtool`"", 'sign', '/fd', 'SHA256', '/td', 'SHA256', '/tr', $timestamp)
    if ($thumbprint) {
        $parts += @('/sha1', $thumbprint)
    } else {
        $parts += @('/f', "`"$certificate`"")
        if ($env:UNLUMINATE_SIGN_PASSWORD) { $parts += @('/p', "`"$env:UNLUMINATE_SIGN_PASSWORD`"") }
    }
    $parts += '$f'
    return ($parts -join ' ')
}

<#
.SYNOPSIS
  Sign the given files with Authenticode, if this machine has been given a certificate.
.DESCRIPTION
  `task-1804` §6: the Windows installer is not signed, so every download meets a SmartScreen
  warning saying Windows protected your PC, and the only way past it is More info then Run anyway.
  macOS has been handled properly since the beginning -- `installer/macos/build.sh` does ad-hoc,
  Developer ID and full notarisation with `notarytool` and stapling -- so this is a gap on one
  platform rather than a missing idea.

  **It needs a certificate, which is a purchase with a lead time, and there is not one yet.** So this
  is written now and does nothing until there is: name a certificate and it signs, name none and it
  says once, plainly, that the build is unsigned and what that means for whoever downloads it. That
  is the shape the ticket asked for -- "signtool in installer/windows/build.ps1 once a certificate
  exists, start the purchase early" -- with the code waiting rather than the code being the thing
  that has to be written when the certificate arrives.

  Two ways to name one, because the two ways certificates are held are genuinely different:

  - `UNLUMINATE_SIGN_THUMBPRINT` -- a certificate already in this machine's store, which is what a
    hardware token or an EV certificate is. This is the one to prefer: the key never leaves the
    token.
  - `UNLUMINATE_SIGN_CERT` and `UNLUMINATE_SIGN_PASSWORD` -- a `.pfx` file and its password. Read
    from the environment at the moment of use and never written anywhere, which is
    `services::agent_tasks::keychain`'s rule about a secret: what is written down is the name of the
    place the key is.

  `UNLUMINATE_SIGN_TIMESTAMP` names the timestamping service, and it defaults to DigiCert's. A
  signature without a timestamp stops being valid the day the certificate expires, which for a
  download somebody keeps is the whole point of signing it.
#>
function Add-Signature([string[]] $Paths) {
    $thumbprint = $env:UNLUMINATE_SIGN_THUMBPRINT
    $certificate = $env:UNLUMINATE_SIGN_CERT
    if (-not $thumbprint -and -not $certificate) {
        Write-Host ''
        Write-Warning 'This build is NOT signed. Whoever downloads it will meet a SmartScreen warning.'
        Write-Host '         Set UNLUMINATE_SIGN_THUMBPRINT (a certificate in this machine''s store) or' -ForegroundColor DarkGray
        Write-Host '         UNLUMINATE_SIGN_CERT and UNLUMINATE_SIGN_PASSWORD (a .pfx and its password).' -ForegroundColor DarkGray
        return
    }
    $signtool = Get-SignTool
    if (-not $signtool) {
        throw 'A signing certificate is named but signtool.exe is not on this machine. It comes with the Windows SDK.'
    }
    $timestamp = if ($env:UNLUMINATE_SIGN_TIMESTAMP) { $env:UNLUMINATE_SIGN_TIMESTAMP } else { 'http://timestamp.digicert.com' }
    foreach ($path in $Paths) {
        if (-not (Test-Path $path)) { throw "There is nothing to sign at $path." }
        $arguments = @('sign', '/fd', 'SHA256', '/td', 'SHA256', '/tr', $timestamp)
        if ($thumbprint) {
            $arguments += @('/sha1', $thumbprint)
        } else {
            $arguments += @('/f', $certificate)
            if ($env:UNLUMINATE_SIGN_PASSWORD) { $arguments += @('/p', $env:UNLUMINATE_SIGN_PASSWORD) }
        }
        $arguments += $path
        & $signtool @arguments
        if ($LASTEXITCODE -ne 0) { throw "signtool failed on $path." }
        # Asked back rather than believed. A `sign` that returns zero and leaves an unsigned file is
        # not a thing that happens, but a certificate that has expired signs happily and verifies as
        # a failure -- and a release is not the moment to find that out.
        & $signtool 'verify' '/pa' '/q' $path
        if ($LASTEXITCODE -ne 0) { throw "signtool signed $path and then would not verify it." }
        Write-Host "Signed $(Split-Path -Leaf $path)" -ForegroundColor Green
    }
}

# ---------------------------------------------------------------------------------------------

if ($Icon) {
    Write-Step 'Drawing the icon'
    & cargo run --release --manifest-path (Join-Path $Repo 'installer\icon\Cargo.toml')
    if ($LASTEXITCODE -ne 0) { throw 'The icon generator failed.' }
}

$compiler = Get-InnoSetupCompiler
Write-Host "Inno Setup: $compiler"

$version = Get-UnluminateVersion
Write-Host "Unluminate $version"

if (-not $SkipBuild) {
    Write-Step 'Building unluminate.exe and unluminate-cli.exe'
    # `CC` naming cl.exe by its full path, with no `INCLUDE` beside it, is worse than `CC` not being
    # set at all: `cc-rs` takes it as the compiler to use and then skips the work it would otherwise
    # do to find the Visual Studio environment, so `cl.exe` runs with no include path and the first
    # C dependency stops at `fatal error C1034: stdarg.h: no include path set`. Some shells here
    # export it -- an agent terminal does -- and a release must not depend on which shell it was
    # started from, so they are put aside for the build and put back afterwards.
    $keptCc = $env:CC; $keptCxx = $env:CXX
    if ($env:CC -and -not $env:INCLUDE) { $env:CC = $null; $env:CXX = $null }
    try {
        & cargo build --release --manifest-path (Join-Path $Repo 'Cargo.toml') -p unluminate-app --bin unluminate
        if ($LASTEXITCODE -ne 0) { throw 'cargo build failed.' }
        # The command line ships beside the editor, and `unluminate-cli` looks for `unluminate` next to itself,
        # so the two have to be installed into one folder.
        & cargo build --release --manifest-path (Join-Path $Repo 'Cargo.toml') -p unluminate-cli --bin unluminate-cli
        if ($LASTEXITCODE -ne 0) { throw 'cargo build failed for unluminate-cli.' }
    } finally {
        $env:CC = $keptCc
        $env:CXX = $keptCxx
    }
}

$exe = Join-Path $BinaryDir 'unluminate.exe'
if (-not (Test-Path $exe)) {
    throw "There is no binary at $exe. Run without -SkipBuild."
}
$cli = Join-Path $BinaryDir 'unluminate-cli.exe'
if (-not (Test-Path $cli)) {
    throw "There is no binary at $cli. Run without -SkipBuild."
}

# The installer must not ship an unlabelled executable: if the resource block is missing then the
# Windows SDK was not there when it was built, and the shortcut, the taskbar and Add or Remove
# Programs would all show a generic icon.
$info = (Get-Item $exe).VersionInfo
if ($info.ProductName -ne 'Unluminate') {
    throw "unluminate.exe has no version block, so it has no icon either. Install the Windows SDK (it comes with the C++ build tools) and build again."
}

# **The binaries first, then the installer.** An installer is signed as one file, and what it holds
# is signed separately or not at all -- so a person who runs `unluminate.exe` from the install folder
# would meet the warning the signed installer just took away.
Write-Step 'Signing'
Add-Signature @($exe, $cli)

Write-Step 'Compiling the installer'
New-Item -ItemType Directory -Force -Path $Dist | Out-Null
$compilerArguments = @(
    "/DAppVersion=$version",
    "/DBinaryDir=$BinaryDir",
    "/DOutputDir=$Dist"
)
$uninstallerSigning = Get-UninstallerSignCommand
if ($uninstallerSigning) {
    # `/S<name>=<command>` is how Inno is given a signing program, and `/DSignCommand` is what turns
    # on the `SignTool` directive in the script that uses it.
    $compilerArguments += "/Sunluminate=$uninstallerSigning"
    $compilerArguments += '/DSignCommand'
}
& $compiler @compilerArguments $Script
if ($LASTEXITCODE -ne 0) { throw 'ISCC failed.' }

$setup = Join-Path $Dist "UnluminateSetup-$version-x64.exe"
if (-not (Test-Path $setup)) { throw "ISCC reported success but $setup is not there." }
Add-Signature @($setup)
$size = [math]::Round((Get-Item $setup).Length / 1MB, 1)
Write-Host ''
Write-Host "Built $setup ($size MB)" -ForegroundColor Green

<#
.SYNOPSIS
  Closes a copy of Unluminate already running from the folder that is about to be written to.
.DESCRIPTION
  Unluminate does not answer the Restart Manager's request to shut down, so a silent install over a
  running copy stops with "Setup was unable to automatically close all applications". The installer
  itself is deliberately left polite about this — CloseApplications=yes asks a person rather than
  killing an editor that may have unsaved changes — so the closing is done here, where it is our own
  automated install doing the asking, and with the window's own close rather than a kill.
#>
function Close-RunningUnluminate([string] $Folder) {
    if (-not (Test-Path $Folder)) { return }
    $running = Get-Process -Name 'unluminate' -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith($Folder, [StringComparison]::OrdinalIgnoreCase) }
    if (-not $running) { return }

    Write-Host 'Closing the copy of Unluminate that is already running'
    foreach ($process in $running) { $process.CloseMainWindow() | Out-Null }

    $deadline = (Get-Date).AddSeconds(20)
    while ((Get-Date) -lt $deadline) {
        Start-Sleep -Milliseconds 500
        $still = Get-Process -Id ($running | Select-Object -ExpandProperty Id) -ErrorAction SilentlyContinue
        if (-not $still) { return }
    }
    throw 'Unluminate is running and would not close. Close it and run this again.'
}

if ($Install) {
    Write-Step 'Installing'
    if ($AllUsers) {
        $target = Join-Path $env:ProgramFiles 'Unluminate'
    } else {
        $target = Join-Path $env:LOCALAPPDATA 'Programs\Unluminate'
    }
    Close-RunningUnluminate $target
    # Every optional task, because this is the switch that installs Unluminate on the machine it was built
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

    foreach ($name in @('unluminate.exe', 'unluminate-cli.exe')) {
        $installed = Join-Path $target $name
        if (Test-Path $installed) {
            Write-Host "Installed $installed" -ForegroundColor Green
        } else {
            Write-Warning "The installer reported success but $installed is not there."
        }
    }
}
