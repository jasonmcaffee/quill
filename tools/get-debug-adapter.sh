#!/usr/bin/env bash
# Fetch a debug adapter on macOS or Linux, and print the setting to point at it.
#
# **Unluminous itself fetches nothing** — that is the rule that keeps a document from making a network
# request, and it keeps the editor from making one too, so pressing Debug with nothing installed is a
# sentence naming what was looked for rather than a download. This script is the other side of that
# sentence: a person who read it and wants an adapter runs this, once, deliberately.
#
# `tools/get-debug-adapter.ps1` is the Windows half and fetches CodeLLDB. This one covers the two
# adapters a machine here needs, and it exists because that script throws on anything but Windows —
# so a person on macOS was told to set `debug.node` and given no way at all to get the thing it names.
#
#   bash tools/get-debug-adapter.sh node        # Microsoft's js-debug, for JavaScript and TypeScript
#   bash tools/get-debug-adapter.sh lldb        # CodeLLDB, for Rust and native code
#   bash tools/get-debug-adapter.sh node --dry-run
#   bash tools/get-debug-adapter.sh node --remove
#
# js-debug is the one with no alternative. It ships as a `.js` file inside a GitHub release asset
# rather than as a program, so there is nothing for Unluminous to look for on PATH and nothing a package
# manager installs — which is why `debug.node` has no default. lldb-dap is the other answer for
# native code and does come from a package manager (`brew install llvm`, or your distribution's
# `lldb` package), so CodeLLDB here is a convenience rather than the only road.
#
# Nothing touches Unluminous's settings file. It prints the line to paste, because a script that edited
# somebody's settings behind their back would be doing more than it was asked.

set -euo pipefail

# Under the user's own data folder, which needs no privileges and is a folder a person can delete.
INTO=${UNLUMINOUS_ADAPTERS:-$HOME/.local/share/unluminous/adapters}
# Pinned rather than latest, so two runs of this script a year apart install the same thing.
JS_DEBUG_VERSION=v1.117.0
CODELLDB_VERSION=v1.12.3

WHICH=${1:-}
shift || true
DRY_RUN=no
REMOVE=no
for argument in "$@"; do
  case $argument in
    --dry-run) DRY_RUN=yes ;;
    --remove) REMOVE=yes ;;
    *)
      echo "get-debug-adapter.sh: $argument is not one of --dry-run or --remove" >&2
      exit 2
      ;;
  esac
done

usage() {
  echo "usage: bash tools/get-debug-adapter.sh <node|lldb> [--dry-run] [--remove]" >&2
  echo "  node  Microsoft's js-debug $JS_DEBUG_VERSION, for JavaScript and TypeScript" >&2
  echo "  lldb  CodeLLDB $CODELLDB_VERSION, for Rust and native code" >&2
  exit 2
}

# The platform pair CodeLLDB names its assets with. js-debug is one asset for every platform, because
# it is JavaScript.
codelldb_asset() {
  local os arch
  os=$(uname -s)
  arch=$(uname -m)
  case "$os-$arch" in
    Darwin-arm64) echo "codelldb-darwin-arm64.vsix" ;;
    Darwin-x86_64) echo "codelldb-darwin-x64.vsix" ;;
    Linux-aarch64) echo "codelldb-linux-arm64.vsix" ;;
    Linux-x86_64) echo "codelldb-linux-x64.vsix" ;;
    *)
      echo "get-debug-adapter.sh: no CodeLLDB build for $os $arch. Install lldb-dap from your LLVM distribution instead: brew install llvm, or your distribution's lldb package." >&2
      exit 1
      ;;
  esac
}

fetch_node() {
  local folder=$INTO/js-debug
  local adapter=$folder/src/dapDebugServer.js
  local asset="https://github.com/microsoft/vscode-js-debug/releases/download/$JS_DEBUG_VERSION/js-debug-dap-$JS_DEBUG_VERSION.tar.gz"

  if [ "$REMOVE" = yes ]; then
    if [ ! -d "$folder" ]; then
      echo "Nothing to remove: $folder is not there."
      return
    fi
    rm -rf "$folder"
    echo "Removed $folder."
    echo 'Take the debug.node line out of the settings as well, or Unluminous will look for an adapter that has gone.'
    return
  fi

  if [ -f "$adapter" ]; then
    echo "Already there: $adapter"
    echo
    echo "Put this in Unluminous's settings file, under Edit -> Settings, or write it by hand:"
    echo "  debug.node = $adapter"
    return
  fi

  if [ "$DRY_RUN" = yes ]; then
    echo "Would download $asset"
    echo "Would unpack it into $INTO"
    echo "Would print: debug.node = $adapter"
    return
  fi

  mkdir -p "$INTO"
  echo "Fetching $asset"
  # The archive already holds a `js-debug/` folder, so it unpacks into $INTO rather than into a
  # folder made for it.
  curl -fsSL "$asset" | tar xz -C "$INTO"

  if [ ! -f "$adapter" ]; then
    echo "get-debug-adapter.sh: the archive unpacked but $adapter is not there; the release layout may have changed." >&2
    exit 1
  fi
  echo
  echo "js-debug $JS_DEBUG_VERSION is in $folder ($(du -sh "$folder" | cut -f1))."
  echo "Put this in Unluminous's settings file:"
  echo "  debug.node = $adapter"
  echo
  echo 'The real-adapter test finds it through an environment variable of the same shape:'
  echo "  UNLUMINOUS_NODE_ADAPTER='$adapter' cargo test -p unluminous-app --test screenshots -- a_real_node_debugger"
  echo
  echo 'bash tools/get-debug-adapter.sh node --remove takes it away again.'
}

fetch_lldb() {
  local folder=$INTO/codelldb
  local adapter=$folder/extension/adapter/codelldb
  local name
  name=$(codelldb_asset)
  local asset="https://github.com/vadimcn/codelldb/releases/download/$CODELLDB_VERSION/$name"

  if [ "$REMOVE" = yes ]; then
    if [ ! -d "$folder" ]; then
      echo "Nothing to remove: $folder is not there."
      return
    fi
    rm -rf "$folder"
    echo "Removed $folder."
    echo 'Take the debug.lldb line out of the settings as well, or Unluminous will look for an adapter that has gone.'
    return
  fi

  if [ -f "$adapter" ]; then
    echo "Already there: $adapter"
    echo
    echo "Put this in Unluminous's settings file, under Edit -> Settings, or write it by hand:"
    echo "  debug.lldb = $adapter"
    return
  fi

  if [ "$DRY_RUN" = yes ]; then
    echo "Would download $asset"
    echo "Would unpack it into $folder"
    echo "Would print: debug.lldb = $adapter"
    return
  fi

  mkdir -p "$folder"
  echo "Fetching $asset"
  # A .vsix is a zip. Fetched to a file first because unzip cannot read a stream.
  local archive=$folder/codelldb.zip
  curl -fsSL -o "$archive" "$asset"
  echo "Unpacking into $folder"
  unzip -q -o "$archive" -d "$folder"
  rm -f "$archive"
  chmod +x "$adapter" 2>/dev/null || true

  if [ ! -f "$adapter" ]; then
    echo "get-debug-adapter.sh: the archive unpacked but $adapter is not there; the release layout may have changed." >&2
    exit 1
  fi
  echo
  echo "CodeLLDB $CODELLDB_VERSION is in $folder ($(du -sh "$folder" | cut -f1))."
  echo "Put this in Unluminous's settings file:"
  echo "  debug.lldb = $adapter"
  echo
  echo 'The real-adapter test finds it through an environment variable of the same shape:'
  echo "  UNLUMINOUS_LLDB_ADAPTER='$adapter' cargo test -p unluminous-app --test screenshots -- a_real_debugger"
  echo
  echo 'bash tools/get-debug-adapter.sh lldb --remove takes it away again.'
}

case "$WHICH" in
  node) fetch_node ;;
  lldb) fetch_lldb ;;
  *) usage ;;
esac
