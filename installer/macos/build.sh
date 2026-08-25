#!/usr/bin/env bash
#
# Builds Quill.app and the disk image it is delivered in.
#
# A Mac needs two things Windows does not. A bundle, because an application that is a bare executable
# has no icon in the Dock, no name in the menu bar and no bundle identity to hang a transparent,
# undecorated window and a screen menu bar off. And a disk image, because that is how a Mac
# application that is not on the App Store arrives: a window with the application on one side and an
# alias to /Applications on the other, and the person drags one onto the other.
#
# It uses nothing that is not part of a stock Xcode command line install: cargo, lipo, iconutil,
# codesign, hdiutil, plutil.
#
# Usage:
#   installer/macos/build.sh                # build Quill.app and Quill-<version>.dmg
#   installer/macos/build.sh --install      # and copy the bundle into /Applications
#   installer/macos/build.sh --no-dmg       # just the bundle
#   installer/macos/build.sh --icon         # redraw the icon first (needs a Rust toolchain)
#
# Signing: set CODESIGN_IDENTITY to a Developer ID Application identity and the bundle is signed with
# it. With nothing set it is signed ad-hoc, which is what makes Gatekeeper treat it as an unsigned
# application to be allowed through once rather than a damaged one to be refused outright. Notarising
# needs an Apple Developer account; installer/README.md has the two notarytool commands.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
dist="$repo/installer/dist"
app="$dist/Quill.app"

install_it=0
make_dmg=1
draw_icon=0
# A while loop rather than `for argument in "$@"`, because the bash macOS ships is 3.2 and that one
# treats an empty "$@" as an unset variable under `set -u`, so running this with no arguments at all
# would stop before it started.
while [ "$#" -gt 0 ]; do
    case "$1" in
        --install) install_it=1 ;;
        --no-dmg) make_dmg=0 ;;
        --icon) draw_icon=1 ;;
        -h|--help) sed -n '3,25p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done

step() { printf '\n==> %s\n' "$1"; }
need() {
    command -v "$1" >/dev/null 2>&1 || {
        echo "$1 is not on the PATH. Install the Xcode command line tools: xcode-select --install" >&2
        exit 1
    }
}

if [ "$(uname -s)" != "Darwin" ]; then
    echo "This builds a macOS bundle and has to run on a Mac." >&2
    exit 1
fi

need cargo
need lipo
need codesign
need plutil
need python3

# ---------------------------------------------------------------------------------------------
# The version, from the one place it is written down.
# ---------------------------------------------------------------------------------------------
version="$(
    cargo metadata --no-deps --format-version 1 --manifest-path "$repo/Cargo.toml" |
    python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="quill-app"))'
)"
echo "Quill $version"

if [ "$draw_icon" = 1 ]; then
    step "Drawing the icon"
    cargo run --release --manifest-path "$repo/installer/icon/Cargo.toml"
fi

# ---------------------------------------------------------------------------------------------
# The binary. Universal when both targets are installed, and whichever one is when only one is.
# ---------------------------------------------------------------------------------------------
step "Building quill"
targets=()
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    if rustup target list --installed 2>/dev/null | grep -qx "$target"; then
        targets+=("$target")
    fi
done

built=()
if [ "${#targets[@]}" -eq 0 ]; then
    echo "Neither Apple target is installed; building for this machine only."
    echo "For a universal binary: rustup target add aarch64-apple-darwin x86_64-apple-darwin"
    cargo build --release --manifest-path "$repo/Cargo.toml" -p quill-app --bin quill
    built+=("$repo/target/release/quill")
else
    for target in "${targets[@]}"; do
        echo "  $target"
        cargo build --release --target "$target" --manifest-path "$repo/Cargo.toml" -p quill-app --bin quill
        built+=("$repo/target/$target/release/quill")
    done
fi

# ---------------------------------------------------------------------------------------------
# The bundle. Four files and a directory tree, which is why it is built here rather than by a tool.
# ---------------------------------------------------------------------------------------------
step "Building Quill.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

if [ "${#built[@]}" -gt 1 ]; then
    lipo -create -output "$app/Contents/MacOS/quill" "${built[@]}"
    echo "  universal: $(lipo -archs "$app/Contents/MacOS/quill")"
else
    cp "${built[0]}" "$app/Contents/MacOS/quill"
    echo "  single architecture: $(lipo -archs "$app/Contents/MacOS/quill")"
fi
chmod +x "$app/Contents/MacOS/quill"

# The icon. `iconutil` is Apple's own tool and so is the definition of the format; the committed
# quill.icns is the fallback, so that the icon can also be rebuilt on a machine that is not a Mac.
if command -v iconutil >/dev/null 2>&1 && [ -d "$repo/installer/icon/Quill.iconset" ]; then
    iconutil --convert icns --output "$app/Contents/Resources/Quill.icns" "$repo/installer/icon/Quill.iconset"
    echo "  icon built by iconutil"
else
    cp "$repo/installer/icon/quill.icns" "$app/Contents/Resources/Quill.icns"
    echo "  icon taken from the committed quill.icns"
fi

sed "s/__VERSION__/$version/g" "$here/Info.plist" > "$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null
printf 'APPL????' > "$app/Contents/PkgInfo"

# ---------------------------------------------------------------------------------------------
# Signing.
# ---------------------------------------------------------------------------------------------
step "Signing"
identity="${CODESIGN_IDENTITY:--}"
if [ "$identity" = "-" ]; then
    echo "  ad-hoc (set CODESIGN_IDENTITY for a Developer ID signature)"
    codesign --force --sign - "$app"
else
    echo "  $identity"
    codesign --force --options runtime --timestamp --sign "$identity" "$app"
fi
codesign --verify --deep --strict --verbose=1 "$app"

# ---------------------------------------------------------------------------------------------
# The disk image.
# ---------------------------------------------------------------------------------------------
if [ "$make_dmg" = 1 ]; then
    need hdiutil
    step "Building the disk image"
    dmg="$dist/Quill-$version.dmg"
    staging="$dist/dmg-staging"
    rm -rf "$staging" "$dmg"
    mkdir -p "$staging"
    cp -R "$app" "$staging/Quill.app"
    ln -s /Applications "$staging/Applications"
    hdiutil create -volname "Quill $version" -srcfolder "$staging" -ov -format UDZO "$dmg" >/dev/null
    rm -rf "$staging"
    echo "  $dmg ($(du -h "$dmg" | cut -f1))"
fi

# ---------------------------------------------------------------------------------------------
# Installing, which on a Mac is a copy.
# ---------------------------------------------------------------------------------------------
if [ "$install_it" = 1 ]; then
    step "Installing into /Applications"
    rm -rf "/Applications/Quill.app"
    cp -R "$app" "/Applications/Quill.app"
    # Launch Services will not notice a new bundle on its own if one was there a moment ago.
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
        -f "/Applications/Quill.app" || true
    echo "  /Applications/Quill.app"
fi

printf '\nDone. %s\n' "$app"
