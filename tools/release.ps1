<#
.SYNOPSIS
  Releases Quill: bump the version, build, install it on this machine, tag, push, and publish a
  GitHub release with the installer on it.

.DESCRIPTION
  `task-1667` asks that finishing a task means releasing it, and asks for that in a form that is
  actually followed. Four commands in the right order is not that form; one is. So everything a
  person would otherwise have to remember is in here, in the order it has to happen, stopping at the
  first thing that goes wrong.

  What it does:

    1. Refuses to run on a dirty checkout. A release built from one is a release nobody can rebuild.
    2. Bumps `version` under `[workspace.package]` in Cargo.toml, which is the one place the version
       is written down. It reaches quill.exe's version block, the installer's file name, the Add or
       Remove Programs entry and Info.plist from there.
    3. Runs installer\windows\build.ps1 -Install: builds quill.exe and quill-cli.exe, refuses to
       package an executable with no version block, compiles the Inno Setup installer, closes a
       running Quill politely and installs it with every optional task on. The rebuild is what moves
       the build date the About box shows.
    4. Copies the installer into releases\.
    5. Commits Cargo.toml and Cargo.lock on their own as `Quill <version>`, tags `v<version>`, and
       pushes the branch and the tag.
    6. Creates the GitHub release with the installer attached.

  The task's own code is expected to be committed already: the version bump is a commit of its own so
  that the history stays greppable by ticket.

.PARAMETER Part
  Which part of the version goes up: patch (the default), minor or major. Patch for a fix, minor for
  a feature.

.PARAMETER Version
  Release exactly this version instead of bumping. Must be higher than the one in Cargo.toml.

.PARAMETER Notes
  The body of the GitHub release. Defaults to the subject of the commit the release is cut from.

.PARAMETER SkipInstall
  Build the installer but do not install it on this machine. The About box will then still show the
  old build, which is the thing this script exists to keep true, so use it only when releasing from a
  machine that is not the one Quill is used on.

.PARAMETER SkipPublish
  Do everything up to and including the tag, and stop before touching GitHub.

.PARAMETER WhatIf
  Say what would happen and change nothing.

.EXAMPLE
  pwsh tools\release.ps1
  pwsh tools\release.ps1 -Part minor -Notes "task-1667: the About box and a one-command release"
#>
[CmdletBinding()]
param(
    [ValidateSet('patch', 'minor', 'major')]
    [string] $Part = 'patch',
    [string] $Version,
    [string] $Notes,
    [switch] $SkipInstall,
    [switch] $SkipPublish,
    [switch] $WhatIf
)

$ErrorActionPreference = 'Stop'

$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$Repo = (Resolve-Path (Join-Path $Here '..')).Path
$Manifest = Join-Path $Repo 'Cargo.toml'
$ReleasesDir = Join-Path $Repo 'releases'

function Write-Step([string] $Message) {
    Write-Host ''
    Write-Host "==> $Message" -ForegroundColor Cyan
}

function Invoke-Checked([string] $What, [scriptblock] $Body) {
    & $Body
    if ($LASTEXITCODE -ne 0) { throw "$What failed with exit code $LASTEXITCODE." }
}

<#
.SYNOPSIS
  The version in Cargo.toml, which is the one place a version is written down.
#>
function Get-CurrentVersion {
    $line = Select-String -Path $Manifest -Pattern '^\s*version\s*=\s*"([0-9]+\.[0-9]+\.[0-9]+)"' |
        Select-Object -First 1
    if (-not $line) { throw "No version = 'x.y.z' in $Manifest." }
    return $line.Matches[0].Groups[1].Value
}

<#
.SYNOPSIS
  The next version, given which part is going up.
.DESCRIPTION
  The ordinary semantic-version rules: a minor bump zeroes the patch and a major bump zeroes both, so
  0.1.4 -Part minor is 0.2.0 rather than 0.2.4.
#>
function Get-NextVersion([string] $Current, [string] $Which) {
    $parts = $Current.Split('.') | ForEach-Object { [int] $_ }
    switch ($Which) {
        'major' { return "$($parts[0] + 1).0.0" }
        'minor' { return "$($parts[0]).$($parts[1] + 1).0" }
        default { return "$($parts[0]).$($parts[1]).$($parts[2] + 1)" }
    }
}

<#
.SYNOPSIS
  True when `a` is a higher version than `b`, compared part by part rather than as text.
#>
function Test-Higher([string] $A, [string] $B) {
    $left = $A.Split('.') | ForEach-Object { [int] $_ }
    $right = $B.Split('.') | ForEach-Object { [int] $_ }
    for ($index = 0; $index -lt 3; $index++) {
        if ($left[$index] -ne $right[$index]) { return $left[$index] -gt $right[$index] }
    }
    return $false
}

<#
.SYNOPSIS
  Write the new version into the `[workspace.package]` table of Cargo.toml.
.DESCRIPTION
  Only the first `version = "x.y.z"` in the file is touched. The workspace table is at the top and
  every crate inherits from it with `version.workspace = true`, so there is exactly one line to
  change and changing more than one would be a mistake rather than a thoroughness.
#>
function Set-Version([string] $New) {
    $text = Get-Content -Raw -Path $Manifest
    $pattern = '(?m)^(\s*version\s*=\s*")([0-9]+\.[0-9]+\.[0-9]+)(")'
    $replaced = [regex]::new($pattern).Replace($text, "`${1}$New`${3}", 1)
    if ($replaced -eq $text) { throw "Could not write the version into $Manifest." }
    # -NoNewline because the file already ends with one, and Set-Content would add a second.
    Set-Content -Path $Manifest -Value $replaced -NoNewline -Encoding utf8
    # Cargo.lock names every workspace member's version, so it has to move with the manifest. A
    # metadata read is the cheapest thing that rewrites it, and it fails loudly if the edit was wrong.
    Invoke-Checked 'cargo metadata' { & cargo metadata --no-deps --format-version 1 --manifest-path $Manifest | Out-Null }
}

<#
.SYNOPSIS
  The GitHub CLI, installing it with winget the first time.
.DESCRIPTION
  The same choice installer\windows\build.ps1 makes about Inno Setup: the one thing this needs that a
  machine able to build Quill does not already have is installed here rather than described in a
  document. It is looked for on the PATH first, then where the MSI puts it, because a shell opened
  before the install will not have the new PATH.
#>
function Get-GitHubCli {
    $onPath = Get-Command 'gh' -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    $candidates = @(
        (Join-Path $env:ProgramFiles 'GitHub CLI\gh.exe'),
        (Join-Path ${env:ProgramFiles(x86)} 'GitHub CLI\gh.exe'),
        (Join-Path $env:LOCALAPPDATA 'Programs\GitHub CLI\gh.exe')
    )
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) { return $candidate }
    }

    Write-Step 'Installing the GitHub CLI, which is not on this machine'
    $winget = Get-Command 'winget' -ErrorAction SilentlyContinue
    if (-not $winget) {
        throw 'gh is not installed and winget is not available to install it. Install it from https://cli.github.com and run this again.'
    }
    & winget install --id GitHub.cli --exact --silent `
        --accept-source-agreements --accept-package-agreements --disable-interactivity
    foreach ($candidate in $candidates) {
        if (Test-Path $candidate) { return $candidate }
    }
    throw 'The GitHub CLI was installed but gh.exe could not be found afterwards.'
}

<#
.SYNOPSIS
  A GitHub token, from the credential helper git is already using.
.DESCRIPTION
  Pushing to this repository already works, which means a credential for github.com is already
  stored, and `git credential fill` is the supported way to ask for it. Using it means there is no
  second credential to set up and nothing new written to disk. GH_TOKEN wins if it is already set,
  which is how a machine with its own token keeps using it.

  Nothing here is printed or returned into the transcript beyond a yes or no.
#>
function Get-GitHubToken {
    if ($env:GH_TOKEN) { return $env:GH_TOKEN }
    if ($env:GITHUB_TOKEN) { return $env:GITHUB_TOKEN }
    # The request goes in through a file rather than a pipe. Windows PowerShell 5.1 does not deliver a
    # piped string to a native program's standard input in a form `git credential` accepts — it
    # answers `refusing to work with credential missing protocol field` — and a redirection from a
    # file does. The file holds the protocol and the host and no secret; the answer, which does hold
    # one, is only ever in memory.
    $ask = Join-Path ([System.IO.Path]::GetTempPath()) ("quill-credential-" + [guid]::NewGuid().ToString('N') + ".txt")
    try {
        Set-Content -Path $ask -Value "protocol=https`nhost=github.com`n" -NoNewline -Encoding ascii
        $answer = & cmd /c "git credential fill < `"$ask`"" 2>$null
    } finally {
        Remove-Item -Path $ask -Force -ErrorAction SilentlyContinue
    }
    $line = $answer | Where-Object { $_ -like 'password=*' } | Select-Object -First 1
    if (-not $line) {
        throw 'No GitHub credential is stored for github.com. Run `gh auth login` (or set GH_TOKEN) and run this again.'
    }
    return $line.Substring('password='.Length)
}

# ---------------------------------------------------------------------------------------------

Set-Location $Repo

$current = Get-CurrentVersion
$next = if ($Version) { $Version } else { Get-NextVersion $current $Part }
if ($next -notmatch '^[0-9]+\.[0-9]+\.[0-9]+$') { throw "$next is not a version of the form x.y.z." }
if (-not (Test-Higher $next $current)) {
    throw "$next is not higher than the version already released, $current."
}

$branch = (& git rev-parse --abbrev-ref HEAD).Trim()
Write-Host "Quill $current -> $next  on $branch"

if ($WhatIf) {
    Write-Host ''
    Write-Host 'What would happen:' -ForegroundColor Yellow
    Write-Host "  1. Cargo.toml version -> $next"
    Write-Host "  2. installer\windows\build.ps1$(if (-not $SkipInstall) { ' -Install' })"
    Write-Host "  3. releases\QuillSetup-$next-x64.exe"
    Write-Host "  4. commit `"Quill $next`", tag v$next, push $branch"
    if (-not $SkipPublish) { Write-Host "  5. gh release create v$next with the installer attached" }
    return
}

Write-Step 'Checking the working tree'
$dirty = & git status --porcelain
if ($dirty) {
    Write-Host ($dirty -join "`n")
    throw 'The working tree is not clean. Commit the task''s own work first: a release built from a dirty checkout is one nobody can rebuild.'
}

# Everything GitHub needs is checked here, before anything is changed, so that a missing credential
# cannot leave a pushed tag with no release behind it.
$gh = $null
$token = $null
if (-not $SkipPublish) {
    $gh = Get-GitHubCli
    $token = Get-GitHubToken
    $env:GH_TOKEN = $token
    Invoke-Checked 'gh auth status' { & $gh auth status 2>&1 | Out-Null }
    Write-Host "GitHub CLI: $gh (authenticated)"
}

Write-Step "Setting the version to $next"
Set-Version $next

Write-Step 'Building the installer, and installing it'
$build = Join-Path $Repo 'installer\windows\build.ps1'
$arguments = @('-File', $build)
if (-not $SkipInstall) { $arguments += '-Install' }
& powershell @arguments
if ($LASTEXITCODE -ne 0) { throw 'installer\windows\build.ps1 failed.' }

$setup = Join-Path $Repo "installer\dist\QuillSetup-$next-x64.exe"
if (-not (Test-Path $setup)) { throw "The installer was not written to $setup." }
New-Item -ItemType Directory -Force -Path $ReleasesDir | Out-Null
$kept = Join-Path $ReleasesDir "QuillSetup-$next-x64.exe"
Copy-Item -Path $setup -Destination $kept -Force
Write-Host "Kept $kept"

Write-Step "Committing and tagging v$next"
Invoke-Checked 'git add' { & git add -- Cargo.toml Cargo.lock }
Invoke-Checked 'git commit' { & git commit -m "Quill $next" | Out-Null }
Invoke-Checked 'git tag' { & git tag -a "v$next" -m "Quill $next" }
Invoke-Checked 'git push' { & git push origin $branch }
Invoke-Checked 'git push --tags' { & git push origin "v$next" }

if ($SkipPublish) {
    Write-Host ''
    Write-Host "Tagged and pushed v$next. Not published, because -SkipPublish was given." -ForegroundColor Green
    return
}

Write-Step "Publishing the GitHub release"
if (-not $Notes) { $Notes = (& git log -1 --pretty=%s "v$next^").Trim() }
$body = @"
$Notes

Windows: download **QuillSetup-$next-x64.exe** below and run it. It installs into
%LOCALAPPDATA%\Programs\Quill with no elevation prompt, and puts ``quill`` and ``quill-cli`` on the PATH.

``Quill -> About Quill`` in the window says which build this is.
"@
& $gh release create "v$next" $kept --repo jasonmcaffee/quill --title "Quill $next" --notes $body
if ($LASTEXITCODE -ne 0) {
    throw "The tag v$next was pushed but the release was not created. Run: gh release create v$next `"$kept`" --title `"Quill $next`""
}

$url = (& $gh release view "v$next" --repo jasonmcaffee/quill --json url --jq .url).Trim()
Write-Host ''
Write-Host "Quill $next is released: $url" -ForegroundColor Green
if (-not $SkipInstall) {
    Write-Host "Installed at $(Join-Path $env:LOCALAPPDATA 'Programs\Quill\quill.exe')" -ForegroundColor Green
}
