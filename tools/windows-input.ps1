<#
.SYNOPSIS
  The one way a Quill script drives the real window with real keyboard and mouse input, written so
  that it cannot leave a key or a button held down.

.DESCRIPTION
  `task-1762` reported a machine on which pressing D minimised the window in front, because the left
  Windows key was held down logically with nothing holding it physically. That is what a synthetic
  key press leaves behind when its release never happens.

  A program presses a key by sending a key-down and releases it by sending a key-up. The scripts that
  drive Quill's real window — the screenshot walks, the wheel and pinch reproductions, the
  documentation captures — all press Ctrl, Alt or Shift, do something with it held, and then release
  it on the line after. Every one of those scripts also has a `throw` between the two: the window did
  not come to the front, the client area measured zero, the picture could not be written. A script
  that stops there has pressed a modifier and will never release it, and so has one a person
  interrupts, and so has one a timeout kills. Nothing on the screen says so afterwards, and the
  physical keyboard cannot clear it, because the physical key was never down.

  So no script should be written with a press on one line and a release on another again. Three
  rules, all of them in here rather than in each script:

    1. A key is held only for the length of a block, and the release is in a `finally`. However the
       block ends -- normally, by throwing, or by being interrupted -- the key goes up.
    2. Everything is released when the shell exits, through `PowerShell.Exiting`, which is the case
       where the throw was caught by nothing.
    3. Everything is released when this file is loaded, so a run can never inherit a key an earlier
       run left down, and so dot-sourcing it is itself a repair.

  Dot source it and use the functions:

      . "$PSScriptRoot\windows-input.ps1"
      Send-Wheel -Notches -3 -Keys $VkCtrl
      Send-Chord -Modifiers $VkCtrl -Key $VkS
      Invoke-Click -X 400 -Y 300

  `Get-HeldInput` says what is held and `Clear-HeldInput` releases it, which is what
  `unstick-keyboard.ps1` is.
#>

Add-Type -Namespace Quill -Name Input -MemberDefinition @'
[DllImport("user32.dll")] public static extern short GetAsyncKeyState(int vKey);
[DllImport("user32.dll")] public static extern void keybd_event(byte vk, byte scan, uint flags, System.UIntPtr extra);
[DllImport("user32.dll")] public static extern void mouse_event(uint flags, int dx, int dy, uint data, System.UIntPtr extra);
[DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
'@

# The virtual key codes a driving script needs, named so that a script reads as the chord it sends.
$script:VkShift = 0x10; $script:VkCtrl = 0x11; $script:VkAlt = 0x12
$script:VkLShift = 0xA0; $script:VkRShift = 0xA1
$script:VkLCtrl = 0xA2; $script:VkRCtrl = 0xA3
$script:VkLAlt = 0xA4; $script:VkRAlt = 0xA5
$script:VkLWin = 0x5B; $script:VkRWin = 0x5C
$script:VkEsc = 0x1B; $script:VkTab = 0x09; $script:VkEnter = 0x0D; $script:VkSpace = 0x20

$script:KeyDown = 0x0000
$script:KeyUp = 0x0002
$script:Extended = 0x0001

# Every modifier, with the extended-key flag the ones with an E0 scan code need -- a key-up sent
# without it is a key-up for a different key as far as the keyboard driver is concerned. The mouse
# buttons are here for the same reason a modifier is: a button pressed for a drag that threw half way
# through is a button the desktop still believes is down.
$script:Modifiers = [ordered]@{
    'Left Shift'    = @{ vk = 0xA0; extended = $false }
    'Right Shift'   = @{ vk = 0xA1; extended = $false }
    'Left Ctrl'     = @{ vk = 0xA2; extended = $false }
    'Right Ctrl'    = @{ vk = 0xA3; extended = $true  }
    'Left Alt'      = @{ vk = 0xA4; extended = $false }
    'Right Alt'     = @{ vk = 0xA5; extended = $true  }
    'Left Windows'  = @{ vk = 0x5B; extended = $true  }
    'Right Windows' = @{ vk = 0x5C; extended = $true  }
    'Shift'         = @{ vk = 0x10; extended = $false }
    'Ctrl'          = @{ vk = 0x11; extended = $false }
    'Alt'           = @{ vk = 0x12; extended = $false }
}

$script:MouseButtons = [ordered]@{
    'Left mouse button'   = @{ vk = 0x01; up = 0x0004 }
    'Right mouse button'  = @{ vk = 0x02; up = 0x0010 }
    'Middle mouse button' = @{ vk = 0x04; up = 0x0040 }
}

<#
.SYNOPSIS
  Names every modifier key and mouse button Windows currently believes is held down.
#>
function Get-HeldInput {
    $held = @()
    foreach ($name in $script:Modifiers.Keys) {
        if (([Quill.Input]::GetAsyncKeyState($script:Modifiers[$name].vk) -band 0x8000) -ne 0) { $held += $name }
    }
    foreach ($name in $script:MouseButtons.Keys) {
        if (([Quill.Input]::GetAsyncKeyState($script:MouseButtons[$name].vk) -band 0x8000) -ne 0) { $held += $name }
    }
    ,$held
}

<#
.SYNOPSIS
  Sends the key-up for every modifier and mouse button, whether or not it is held.

.DESCRIPTION
  Releasing something that was never pressed is harmless, so this does not ask first: asking and then
  releasing would miss anything pressed between the two. The Windows key is the one to arrange
  around -- the shell opens the Start menu when it goes up with nothing pressed in between, so Ctrl
  is tapped first to make it a chord, and Win+Ctrl means nothing.
#>
function Clear-HeldInput {
    $windowsHeld = ([Quill.Input]::GetAsyncKeyState(0x5B) -band 0x8000) -ne 0 -or
                   ([Quill.Input]::GetAsyncKeyState(0x5C) -band 0x8000) -ne 0
    if ($windowsHeld) {
        [Quill.Input]::keybd_event(0x11, 0, $script:KeyDown, [UIntPtr]::Zero)
        [Quill.Input]::keybd_event(0x11, 0, $script:KeyUp,   [UIntPtr]::Zero)
    }
    foreach ($name in $script:Modifiers.Keys) {
        $key = $script:Modifiers[$name]
        $flags = $script:KeyUp
        if ($key.extended) { $flags = $flags -bor $script:Extended }
        [Quill.Input]::keybd_event([byte]$key.vk, 0, $flags, [UIntPtr]::Zero)
    }
    foreach ($name in $script:MouseButtons.Keys) {
        [Quill.Input]::mouse_event([uint32]$script:MouseButtons[$name].up, 0, 0, 0, [UIntPtr]::Zero)
    }
}

<#
.SYNOPSIS
  Presses a key and releases it.
.PARAMETER Vk
  The virtual key code to press.
.PARAMETER HoldMs
  How long the key is held, in milliseconds.
#>
function Send-Key {
    param([Parameter(Mandatory)][int]$Vk, [int]$HoldMs = 60)
    try {
        [Quill.Input]::keybd_event([byte]$Vk, 0, $script:KeyDown, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds $HoldMs
    } finally {
        [Quill.Input]::keybd_event([byte]$Vk, 0, $script:KeyUp, [UIntPtr]::Zero)
    }
}

<#
.SYNOPSIS
  Runs a block with some keys held down, and releases them however the block ends.
.PARAMETER Keys
  The virtual key codes to hold.
.PARAMETER Body
  What to do while they are held.
#>
function Invoke-WithKeysHeld {
    param([Parameter(Position = 0)][int[]]$Keys = @(),
          [Parameter(Mandatory, Position = 1)][scriptblock]$Body)
    try {
        foreach ($vk in $Keys) {
            [Quill.Input]::keybd_event([byte]$vk, 0, $script:KeyDown, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 40
        }
        & $Body
    } finally {
        # In reverse, so a chord comes apart the way a hand takes it apart, and unconditionally: a
        # `throw` inside the block lands here on its way out.
        $letGo = @($Keys)
        [array]::Reverse($letGo)
        foreach ($vk in $letGo) {
            [Quill.Input]::keybd_event([byte]$vk, 0, $script:KeyUp, [UIntPtr]::Zero)
        }
    }
}

<#
.SYNOPSIS
  Sends a chord: a key pressed with some modifiers held, with the modifiers always released.
.PARAMETER Modifiers
  The virtual key codes held down for the press.
.PARAMETER Key
  The virtual key code pressed.
.PARAMETER SettleMs
  How long to wait afterwards for the window to catch up.
#>
function Send-Chord {
    param([int[]]$Modifiers = @(), [Parameter(Mandatory)][int]$Key, [int]$SettleMs = 400)
    Invoke-WithKeysHeld $Modifiers { Send-Key -Vk $Key }.GetNewClosure()
    Start-Sleep -Milliseconds $SettleMs
}

<#
.SYNOPSIS
  Turns the mouse wheel, optionally with keys held. A negative count scrolls down.
.PARAMETER Notches
  How many notches to turn, negative for down.
.PARAMETER Keys
  Virtual key codes to hold while turning, such as Ctrl for a zoom.
.PARAMETER StepMs
  How long to wait between notches.
#>
function Send-Wheel {
    param([int]$Notches = 1, [int[]]$Keys = @(), [int]$StepMs = 90)
    # `mouse_event` takes the delta as an unsigned word, so a scroll down is the two's complement of
    # 120 rather than a negative number; PowerShell will not cast -120 to a UInt32.
    $step = if ($Notches -ge 0) { [uint32]120 } else { [uint32]4294967176 }
    $turns = [Math]::Abs($Notches)
    Invoke-WithKeysHeld $Keys {
        # Zero notches holds the keys and turns nothing, which is what a test asks for; `1..0`
        # counts backwards in PowerShell and would turn the wheel the wrong way once.
        for ($i = 1; $i -le $turns; $i++) {
            [Quill.Input]::mouse_event(0x0800, 0, 0, $step, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds $StepMs
        }
    }.GetNewClosure()
}

<#
.SYNOPSIS
  Clicks at a screen position, releasing the button however the click ends.
.PARAMETER X
  The screen x to click at.
.PARAMETER Y
  The screen y to click at.
.PARAMETER Times
  How many clicks.
.PARAMETER Right
  Click the right button instead of the left.
#>
function Invoke-Click {
    param([Parameter(Mandatory)][int]$X, [Parameter(Mandatory)][int]$Y,
          [int]$Times = 1, [switch]$Right)
    $down = if ($Right) { 0x0008 } else { 0x0002 }
    $up   = if ($Right) { 0x0010 } else { 0x0004 }
    [void][Quill.Input]::SetCursorPos($X, $Y)
    Start-Sleep -Milliseconds 250
    for ($i = 0; $i -lt $Times; $i++) {
        try {
            [Quill.Input]::mouse_event([uint32]$down, 0, 0, 0, [UIntPtr]::Zero)
            Start-Sleep -Milliseconds 40
        } finally {
            [Quill.Input]::mouse_event([uint32]$up, 0, 0, 0, [UIntPtr]::Zero)
        }
        Start-Sleep -Milliseconds 60
    }
}

<#
.SYNOPSIS
  Drags from one screen position to another, releasing the button however the drag ends.
.PARAMETER FromX
  Where the drag starts.
.PARAMETER FromY
  Where the drag starts.
.PARAMETER ToX
  Where the drag ends.
.PARAMETER ToY
  Where the drag ends.
.PARAMETER Steps
  How many positions the pointer is moved through.
#>
function Invoke-Drag {
    param([Parameter(Mandatory)][int]$FromX, [Parameter(Mandatory)][int]$FromY,
          [Parameter(Mandatory)][int]$ToX, [Parameter(Mandatory)][int]$ToY, [int]$Steps = 20)
    [void][Quill.Input]::SetCursorPos($FromX, $FromY)
    Start-Sleep -Milliseconds 300
    try {
        [Quill.Input]::mouse_event(0x0002, 0, 0, 0, [UIntPtr]::Zero)
        Start-Sleep -Milliseconds 150
        for ($i = 1; $i -le $Steps; $i++) {
            $x = $FromX + [int](($ToX - $FromX) * $i / $Steps)
            $y = $FromY + [int](($ToY - $FromY) * $i / $Steps)
            [void][Quill.Input]::SetCursorPos($x, $y)
            Start-Sleep -Milliseconds 25
        }
    } finally {
        [Quill.Input]::mouse_event(0x0004, 0, 0, 0, [UIntPtr]::Zero)
    }
    Start-Sleep -Milliseconds 300
}

# The two guarantees that do not depend on a script remembering anything. Loading this file repairs
# whatever an earlier run left down, and the shell exiting releases whatever this one is holding --
# which is the case a `throw` nothing caught takes.
#
# `$QuillInputReadOnly` is for the one caller that wants to look without touching: `unstick-keyboard
# -Check` promises to change nothing, and a repair on load would make that a lie.
if (-not $QuillInputReadOnly) {
    Clear-HeldInput
    Register-EngineEvent -SourceIdentifier PowerShell.Exiting -SupportEvent -Action { Clear-HeldInput } | Out-Null
}
