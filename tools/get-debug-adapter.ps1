# Fetch a debug adapter so `Run -> Debug` has something to drive, and print the setting to point at
# it.
#
# **Unluminate itself fetches nothing** — that is the rule that keeps a document from making a network
# request, and it keeps the editor from making one too, so pressing Debug with nothing installed is a
# sentence naming what was looked for rather than a download. This script is the other side of that
# sentence: a person who read it and wants an adapter runs this, once, deliberately. It is the same
# arrangement `release.ps1` has with `gh`.
#
#   pwsh tools/get-debug-adapter.ps1              # fetch CodeLLDB and say what to put in the settings
#   pwsh tools/get-debug-adapter.ps1 -WhatIf      # say what it would do and change nothing
#   pwsh tools/get-debug-adapter.ps1 -Remove      # take it away again
#
# CodeLLDB rather than lldb-dap because it needs no elevation: its `.vsix` is a zip holding
# `adapter/codelldb.exe` and the LLDB it drives, so unpacking it is the whole install. It also
# carries Rust-aware formatters, which is why the registry prefers it. `lldb-dap` is the other
# answer and ships inside every LLVM distribution — `winget install LLVM.LLVM`, which does need
# elevation.
#
# Nothing here touches Unluminate's settings file. It prints the line to paste, because a script that
# edited somebody's settings behind their back would be doing more than it was asked.

[CmdletBinding(SupportsShouldProcess = $true)]
param(
    # Where to put it. Under the local application data by default, which needs no elevation and is
    # a folder a person can delete.
    [string] $Into = (Join-Path $env:LOCALAPPDATA 'Unluminate\adapters'),
    # The CodeLLDB release to fetch. Pinned rather than 'latest' so two runs of this script a year
    # apart install the same thing.
    [string] $Version = 'v1.12.3',
    [switch] $Remove
)

$ErrorActionPreference = 'Stop'

$folder = Join-Path $Into 'codelldb'
$adapter = Join-Path $folder 'extension\adapter\codelldb.exe'

if ($Remove) {
    if (-not (Test-Path $folder)) {
        Write-Host "Nothing to remove: $folder is not there."
        return
    }
    if ($PSCmdlet.ShouldProcess($folder, 'Remove')) {
        Remove-Item -Recurse -Force $folder
        Write-Host "Removed $folder."
        Write-Host 'Take the `debug.lldb` line out of the settings as well, or Unluminate will look for an adapter that has gone.'
    }
    return
}

if (Test-Path $adapter) {
    Write-Host "Already there: $adapter"
    Write-Host ''
    Write-Host 'Put this in Unluminate''s settings file, under Edit -> Settings, or write it by hand:'
    Write-Host "  debug.lldb = $adapter"
    return
}

# Windows x64 only, which is what this machine is and what the one asset is built for. A person on
# another platform is told rather than left with a download that will not run.
if (-not $IsWindows -and $PSVersionTable.PSVersion.Major -ge 6) {
    throw 'This script fetches the Windows x64 build. On macOS or Linux, install lldb-dap from your LLVM distribution instead.'
}

$asset = "https://github.com/vadimcn/codelldb/releases/download/$Version/codelldb-win32-x64.vsix"
if (-not $PSCmdlet.ShouldProcess($asset, "Download into $folder")) {
    Write-Host "Would download $asset"
    Write-Host "Would unpack it into $folder"
    return
}

New-Item -ItemType Directory -Force -Path $folder | Out-Null
# A `.vsix` is a zip, and `Expand-Archive` refuses anything that is not named `.zip` — so it is
# fetched under a name it will accept rather than being renamed afterwards.
$archive = Join-Path $folder 'codelldb.zip'
Write-Host "Fetching $asset"
Invoke-WebRequest -Uri $asset -OutFile $archive
Write-Host "Unpacking into $folder"
Expand-Archive -Path $archive -DestinationPath $folder -Force
Remove-Item $archive

if (-not (Test-Path $adapter)) {
    throw "The archive unpacked but $adapter is not there; the release layout may have changed."
}

$size = [math]::Round((Get-ChildItem -Recurse $folder | Measure-Object -Property Length -Sum).Sum / 1MB)
Write-Host ''
Write-Host "CodeLLDB $Version is in $folder ($size MB)."
Write-Host 'Put this in Unluminate''s settings file:'
Write-Host "  debug.lldb = $adapter"
Write-Host ''
Write-Host 'The real-adapter test finds it through an environment variable of the same shape:'
Write-Host "  `$env:UNLUMINATE_LLDB_ADAPTER = '$adapter'; cargo test -p unluminate-app --test screenshots -- a_real_debugger"
Write-Host ''
Write-Host 'pwsh tools/get-debug-adapter.ps1 -Remove takes it away again.'
