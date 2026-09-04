<#
.SYNOPSIS
  The six end-to-end agent tests, on this machine, once.

.DESCRIPTION
  `task-1804` §2.2 asks for a scheduled run of `crates/unluminous-app/tests/agent_board.rs` — the
  deepest tests in the repository, every one of them `#[ignore]`d because they take minutes and cost
  tokens.

  `.github/workflows/nightly.yml` is the scheduled run for anybody who has put a key on the
  repository. **This is the scheduled run for this machine**, which is where the key and the agent
  already are, and it is here because a nightly that needs a secret nobody has set is a nightly that
  covers nothing.

  It runs them one at a time, writes what happened into `_agent_output/nightly/`, and says on the
  console whether they passed. Nothing here writes a key anywhere: `ANTHROPIC_API_KEY` and
  `ANTHROPIC_BASE_URL` are read from the environment at the moment of launch, which is the same path
  the window uses.

.PARAMETER Register
  Register it with the Windows scheduler to run at 03:00 every day, and stop. It is not registered
  by running it; that is a thing a person should ask for once.

.PARAMETER Unregister
  Take it out of the scheduler again.

.EXAMPLE
  pwsh tools/nightly.ps1
  pwsh tools/nightly.ps1 -Register
#>
[CmdletBinding()]
param(
    [switch] $Register,
    [switch] $Unregister
)

$ErrorActionPreference = 'Stop'

$Here = Split-Path -Parent $MyInvocation.MyCommand.Path
$Repo = Resolve-Path (Join-Path $Here '..')
$TaskName = 'Unluminous nightly agent tests'

if ($Unregister) {
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
    Write-Host "Took '$TaskName' out of the scheduler." -ForegroundColor Green
    return
}

if ($Register) {
    $me = Join-Path $Here 'nightly.ps1'
    $action = New-ScheduledTaskAction -Execute 'pwsh.exe' -Argument "-NoProfile -File `"$me`""
    $trigger = New-ScheduledTaskTrigger -Daily -At 3am
    # Run whether or not somebody is logged in would need a stored credential, which this script will
    # not ask for. It runs as the person who registered it, when they are logged in, which is what a
    # developer machine is.
    Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger -Force | Out-Null
    Write-Host "Registered '$TaskName' for 03:00 daily." -ForegroundColor Green
    Write-Host "Take it out again with: pwsh tools/nightly.ps1 -Unregister"
    return
}

# The two things they need, checked before anything is built so a missing one is not found forty
# seconds in. The tests themselves skip rather than fail when `claude` is missing; this says so up
# front, because a run that covers nothing should not look like a run that passed.
$claude = Get-Command claude -ErrorAction SilentlyContinue
if (-not $claude) {
    Write-Warning 'There is no `claude` on the path, so every one of these tests will skip. Nothing was covered.'
}
if (-not $env:ANTHROPIC_API_KEY) {
    Write-Warning 'ANTHROPIC_API_KEY is not set in this shell, so the agent will have no key.'
}

$out = Join-Path $Repo '_agent_output\nightly'
New-Item -ItemType Directory -Force -Path $out | Out-Null
$stamp = Get-Date -Format 'yyyy-MM-dd-HHmmss'
$log = Join-Path $out "agent-board-$stamp.log"

Write-Host ''
Write-Host "==> The six end-to-end agent tests, one at a time" -ForegroundColor Cyan
Write-Host "    Writing to $log"

# One at a time, because each starts an agent that reads and writes files in its own temporary folder
# and this machine has one key with one rate limit behind it. That is `agent_board.rs`'s own rule.
& cargo test --manifest-path (Join-Path $Repo 'Cargo.toml') -p unluminous-app --test agent_board -- --ignored --test-threads=1 2>&1 |
    Tee-Object -FilePath $log

$passed = $LASTEXITCODE -eq 0
Write-Host ''
if ($passed) {
    Write-Host "The end-to-end agent tests passed. Log: $log" -ForegroundColor Green
} else {
    Write-Host "The end-to-end agent tests FAILED. Log: $log" -ForegroundColor Red
    exit 1
}
