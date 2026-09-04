#!/usr/bin/env bash
# Drive a real Unluminous window through every area of the command line, photographing it as it goes.
#
# This is the fourth layer of Unluminous's tests: the screenshot tests render offscreen, and only a real
# run shows that the real window really did what a command asked. Every step writes a PNG into
# `_agent_output/task-1661-unluminous-cli/shots`, and the reply to every command is appended to
# `transcript.txt` beside them, so the pictures and the answers can be read together.
#
#   pwsh/bash tools/unluminous-cli-tour.sh
#
# It leaves the window open at the end, on purpose: the last picture is of a window somebody can
# still look at. `unluminous-cli quit` closes it.

set -u

CLI=${UNLUMINOUS_CLI:-./target/release/unluminous-cli.exe}
OUT=_agent_output/task-1661-unluminous-cli
SHOTS=$OUT/shots
mkdir -p "$SHOTS"
: >"$OUT/transcript.txt"

step=0
failures=0

# Run one command, record what it said, and photograph the window afterwards.
say() {
  step=$((step + 1))
  local name=$1
  shift
  local numbered
  numbered=$(printf '%02d' "$step")
  {
    echo "=============================================================================="
    echo "[$numbered] $name"
    echo "\$ unluminous-cli $*"
  } >>"$OUT/transcript.txt"
  local reply
  reply=$("$CLI" "$@" --json 2>&1)
  local code=$?
  echo "$reply" >>"$OUT/transcript.txt"
  echo "exit $code" >>"$OUT/transcript.txt"
  if [ $code -ne 0 ]; then
    failures=$((failures + 1))
    echo "FAILED [$numbered] $name (exit $code)"
    echo "$reply" | head -5
  else
    echo "ok     [$numbered] $name"
  fi
  "$CLI" window screenshot "$SHOTS/$numbered-$name.png" --json >/dev/null 2>&1
}

# A command run for its answer, with no picture taken: nothing about it changes what is on screen.
read_only() {
  local name=$1
  shift
  {
    echo "------------------------------------------------------------------------------"
    echo "($name)"
    echo "\$ unluminous-cli $*"
  } >>"$OUT/transcript.txt"
  "$CLI" "$@" --json >>"$OUT/transcript.txt" 2>&1
  echo "exit $?" >>"$OUT/transcript.txt"
}

echo "Driving the Unluminous at:"
"$CLI" instances

# ---------------------------------------------------------------- a known starting point
say window-size          window size --width 1200 --height 800
say window-focus         window focus
read_only status         status

# ---------------------------------------------------------------- the explorer
say explorer-show        explorer show
say explorer-width       explorer width 320
say explorer-expand      explorer expand unluminous-cli/src
read_only explorer-tree  explorer tree --limit 30
read_only explorer-files explorer files --limit 10
say explorer-filter      explorer filter tdd
say explorer-unfilter    explorer filter
say explorer-hide        explorer hide
say explorer-back        explorer show

# ---------------------------------------------------------------- tabs and the editor
say tab-open-readme      tab open README.md
say tab-open-tdd         tab open tasks/unluminous-cli-tdd.md --permanent
say tab-open-source      tab open unluminous-cli/src/catalogue.rs --permanent
read_only tab-list       tab list
say tab-previous         tab previous
# Named rather than counted, so the tour lands on the same file however the window was left. The
# three view modes only apply to a file with a preview, and `not-applicable` for a `.rs` file is the
# right answer rather than a fault.
say tab-show-readme      tab show README.md
say editor-caret         editor caret --line 40 --column 1
say editor-select        editor select --from-line 40 --to-line 46
say editor-view-side     editor view side
say editor-view-preview  editor view preview
say editor-view-raw      editor view raw
read_only editor-status  editor status
read_only editor-text    editor text --from-line 1 --to-line 12

# ---------------------------------------------------------------- the modals
say modal-go-to-file     modal open go-to-file --query mdrs
read_only goto-results   modal results --limit 5
say modal-choose         modal choose 0
say modal-cancel         modal cancel
say modal-find           modal open find-in-files --query "fn main"
read_only find-results   modal results --wait 8000 --limit 8
say modal-find-results   modal state
say modal-moved          modal move --x 80 --y 90
say modal-resized        modal size --width 760 --height 520
say modal-reset          modal reset
say modal-accept         modal accept 0
say modal-settings       modal open settings --page terminal
say modal-settings-shut  modal cancel
say modal-new-file       modal open new-file --path _agent_output/task-1661-unluminous-cli
say modal-new-file-shut  modal cancel

# ---------------------------------------------------------------- the settings
read_only settings-list  settings list
say settings-font-size   settings set appearance.font.size 20
say settings-opacity     settings set appearance.background.opacity 0.55
say settings-numbers-off settings set editor.line_numbers false
say settings-numbers-on  settings set editor.line_numbers true
say settings-font-back   settings set appearance.font.size 13
say settings-opacity-back settings set appearance.background.opacity 0.83

# ---------------------------------------------------------------- the terminal
say terminal-show        terminal show
say terminal-height      terminal height 320
say terminal-send        terminal send git status --short
read_only terminal-read  terminal read --wait-for "$" --timeout 12000 --lines 20
say terminal-after-git   window message the terminal has run git status
say terminal-new-tab     terminal new
say terminal-send-2      terminal send cargo --version
read_only terminal-read2 terminal read --wait-for cargo --timeout 30000 --lines 12
say terminal-two-tabs    terminal list
say terminal-close       terminal close
say terminal-hide        terminal hide

# ---------------------------------------------------------------- git, actions, plugins
read_only git-status     git status
read_only git-actions    git actions
say git-annotate         git action annotate
say git-annotate-off     git action annotate
read_only action-list    action list
say action-line-numbers  action run toggle-line-numbers
say action-back          action run toggle-line-numbers
say action-about         action run about
read_only plugins        plugins list

# ---------------------------------------------------------------- what it refuses
read_only refuse-chooser action run open-file
read_only refuse-missing tab open no-such-file.md
read_only refuse-unknown editor view sideways

echo
echo "$step steps, $failures failures. Pictures in $SHOTS, replies in $OUT/transcript.txt"
exit $((failures > 0))
