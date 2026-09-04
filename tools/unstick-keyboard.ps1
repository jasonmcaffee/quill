<#
.SYNOPSIS
  Puts the keyboard back after a modifier key has been left logically held down.

.DESCRIPTION
  A program that synthesises key presses -- a test harness, a screenshot walk, an automation script,
  anything calling `SendInput` or `keybd_event` -- presses a key by sending a key-down and releases
  it by sending a key-up. If it sends the down and then dies, is killed, throws or times out before
  the up, Windows goes on believing that key is held for as long as the session lasts. Nothing on the
  screen says so, and the physical keyboard cannot clear it, because the physical key was never down:
  its own key-up has already been and gone.

  A held Windows key is the one people notice, because every letter then becomes a shortcut -- D
  minimises everything, E opens Explorer, L locks the machine. A held Ctrl, Alt or Shift is quieter
  and just as wrong, and a mouse button left down is the same fault wearing a different hat.

  `task-1762` is where this came from. The release is `windows-input.ps1`'s, so that the tool a person
  runs by hand and the guarantee every driving script already has are one piece of code rather than
  two that could disagree.

.PARAMETER Check
  Say what is held and change nothing. Exits 1 when something is held, so it can be asked as a
  question by a script.

.EXAMPLE
  pwsh tools/unstick-keyboard.ps1

.EXAMPLE
  pwsh tools/unstick-keyboard.ps1 -Check
#>
[CmdletBinding()]
param([switch]$Check)

# Dot sourcing this releases everything by itself, which is the whole of the repair -- unless the
# question is only being asked, which is what `$UnluminateInputReadOnly` says.
$UnluminateInputReadOnly = [bool]$Check
. "$PSScriptRoot\windows-input.ps1"

if ($Check) {
    $held = Get-HeldInput
    if ($held.Count -eq 0) {
        Write-Host 'The keyboard is in order: nothing is being held.'
        exit 0
    }
    Write-Host ("Held down: {0}" -f ($held -join ', '))
    exit 1
}

# `windows-input.ps1` released everything as it loaded, so anything still held is being held now
# rather than having been left behind -- which is a different fault and worth saying so.
Start-Sleep -Milliseconds 60
$still = Get-HeldInput
if ($still.Count -eq 0) {
    Write-Host 'Released every modifier and mouse button. The keyboard is back to normal.'
    exit 0
}

Write-Host ("Still held after releasing: {0}" -f ($still -join ', '))
Write-Host 'Something is holding these down now rather than having left them down -- look for an'
Write-Host 'automation tool that is still running, or a key that is physically stuck.'
exit 1
