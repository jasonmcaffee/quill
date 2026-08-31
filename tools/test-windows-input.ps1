<#
.SYNOPSIS
  Proves that `windows-input.ps1` cannot leave a key or a button held down, and that
  `unstick-keyboard.ps1` releases one that something else did.

.DESCRIPTION
  `task-1762` was a Windows key left down by something that pressed it and never released it, so the
  thing worth testing is not that a chord arrives -- it is that the release happens on every way out
  of the block, including the ways nobody writes a line for. Each case leaves a key down on purpose
  and then asks Windows, through `GetAsyncKeyState`, whether it is still down.

  It is safe to run while somebody is working. Every key it presses is a bare modifier, which no
  program acts on by itself; it types nothing, clicks nothing, turns the wheel through no notches and
  takes no window's focus.

      pwsh tools/test-windows-input.ps1
#>
[CmdletBinding()]
param()

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
$failures = 0

<#
.SYNOPSIS
  Runs one case and reports whether the keyboard came out of it clean.
.PARAMETER Name
  What the case is called in the report.
.PARAMETER Body
  The case. It may throw; that is usually the point.
#>
function Test-Case {
    param([Parameter(Mandatory)][string]$Name, [Parameter(Mandatory)][scriptblock]$Body)
    try { & $Body } catch { }
    Start-Sleep -Milliseconds 80
    $held = Get-HeldInput
    if ($held.Count -eq 0) {
        Write-Host ("  ok    {0}" -f $Name)
        return $true
    }
    Write-Host ("  FAIL  {0} -- left held: {1}" -f $Name, ($held -join ', '))
    return $false
}

<#
.SYNOPSIS
  Says whether a case passed, and counts it if it did not.
.PARAMETER Passed
  What Test-Case answered.
#>
function Add-Result {
    param([bool]$Passed)
    if (-not $Passed) { $script:failures++ }
}

. "$root\windows-input.ps1"

Write-Host 'windows-input.ps1'

Add-Result (Test-Case 'a chord releases the key it pressed' {
    Send-Chord -Modifiers @() -Key $VkCtrl -SettleMs 0
})

Add-Result (Test-Case 'a chord releases its modifiers' {
    Send-Chord -Modifiers @($VkLCtrl, $VkRCtrl) -Key $VkCtrl -SettleMs 0
})

Add-Result (Test-Case 'a block that throws still releases its keys' {
    Invoke-WithKeysHeld @($VkLCtrl, $VkRCtrl) { throw 'the window never came to the front' }
})

Add-Result (Test-Case 'a wheel turn that throws still releases Ctrl' {
    Invoke-WithKeysHeld $VkCtrl { Send-Wheel -Notches 0; throw 'the picture could not be written' }
})

Add-Result (Test-Case 'a key held while the block is stopped is released' {
    Invoke-WithKeysHeld $VkLCtrl { Write-Error 'stopped part way through' -ErrorAction Stop }
})

Write-Host 'unstick-keyboard.ps1'

# A key left down the way the fault leaves one: a bare key-down through `keybd_event` with no
# matching up anywhere, which is exactly what was measured on the machine in `task-1762`. Left Ctrl
# rather than the Windows key, because on a machine running the macOS Command-key hook an injected
# Windows key is swallowed by that hook, so the fault would not reproduce with it.
[Quill.Input]::keybd_event(0xA2, 0, 0x0000, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 80
$before = Get-HeldInput
if ($before -contains 'Left Ctrl') {
    Write-Host '  ok    a bare key-down really does leave a modifier held'
} else {
    Write-Host '  FAIL  the fault could not be reproduced, so the repair proves nothing'
    $failures++
}

& pwsh -NoProfile -File "$root\unstick-keyboard.ps1" | ForEach-Object { Write-Host ("        {0}" -f $_) }
Start-Sleep -Milliseconds 80
$after = Get-HeldInput
if ($after.Count -eq 0) {
    Write-Host '  ok    unstick-keyboard released it'
} else {
    Write-Host ("  FAIL  still held: {0}" -f ($after -join ', '))
    $failures++
}

# `-Check` has to answer without changing anything, so it is asked while a key is genuinely down.
[Quill.Input]::keybd_event(0xA2, 0, 0x0000, [UIntPtr]::Zero)
Start-Sleep -Milliseconds 80
& pwsh -NoProfile -File "$root\unstick-keyboard.ps1" -Check | Out-Null
$checkExit = $LASTEXITCODE
$stillHeld = Get-HeldInput
if ($checkExit -eq 1 -and ($stillHeld -contains 'Left Ctrl')) {
    Write-Host '  ok    -Check reports a held key and leaves it alone'
} else {
    Write-Host ("  FAIL  -Check exited {0} and the key is {1}" -f $checkExit,
        $(if ($stillHeld.Count) { 'still held' } else { 'no longer held' }))
    $failures++
}
Clear-HeldInput
Start-Sleep -Milliseconds 80

$leftOver = Get-HeldInput
if ($leftOver.Count -ne 0) {
    Write-Host ("  FAIL  the test itself left something held: {0}" -f ($leftOver -join ', '))
    $failures++
}

Write-Host ''
if ($failures -eq 0) {
    Write-Host 'All cases passed, and the keyboard is as it was found.'
    exit 0
}
Write-Host ("{0} case(s) failed." -f $failures)
exit 1
