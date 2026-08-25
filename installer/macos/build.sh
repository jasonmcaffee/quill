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
# codesign, hdiutil, plutil, and notarytool and stapler when notarising.
#
# Usage:
#   installer/macos/build.sh                # build Quill.app and releases/quill-<version>.dmg
#   installer/macos/build.sh --install      # and copy the bundle into /Applications
#   installer/macos/build.sh --no-dmg       # just the bundle
#   installer/macos/build.sh --icon         # redraw the icon first (needs a Rust toolchain)
#   installer/macos/build.sh --notarize     # sign, send it to Apple, staple the ticket to the image
#
# The image goes to `releases/quill-<version>.dmg`, named from the version in Cargo.toml, so that past
# versions sit beside each other and a file's name says which one it is.
#
# Signing and notarising. There are three levels and the script says which one it did.
#
#   1. Ad-hoc, when CODESIGN_IDENTITY is not set. Gatekeeper treats it as an unsigned application:
#      allowed through once with a right click and Open rather than refused as damaged. That is enough
#      for a copy that never leaves the machine it was built on.
#   2. Developer ID, when CODESIGN_IDENTITY names a `Developer ID Application` identity in the
#      keychain. Signed with the hardened runtime and a timestamp, which is what notarising requires.
#   3. Notarised, with --notarize as well: the image goes to Apple, and the ticket that comes back is
#      stapled to it, so somebody who downloads it opens it with no warning and with no network.
#
# Level 3 needs credentials for notarytool, either of:
#
#   NOTARY_PROFILE   a profile stored once with
#                    xcrun notarytool store-credentials <name> --apple-id <id> --team-id <team> --password <app-specific>
#   NOTARY_APPLE_ID, NOTARY_TEAM_ID, NOTARY_PASSWORD   the same three given each time
#
# installer/README.md says where each of those comes from. Nothing here prints or stores a password.

set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo="$(cd "$here/../.." && pwd)"
dist="$repo/installer/dist"
app="$dist/Quill.app"
# Where a finished image is kept. `installer/dist` is the working area and is rewritten on every run;
# this is what is kept, and every file in it carries the version it was built from.
releases="$repo/releases"

install_it=0
make_dmg=1
draw_icon=0
notarize=0
# A while loop rather than `for argument in "$@"`, because the bash macOS ships is 3.2 and that one
# treats an empty "$@" as an unset variable under `set -u`, so running this with no arguments at all
# would stop before it started.
while [ "$#" -gt 0 ]; do
    case "$1" in
        --install) install_it=1 ;;
        --no-dmg) make_dmg=0 ;;
        --icon) draw_icon=1 ;;
        --notarize|--notarise) notarize=1 ;;
        -h|--help) sed -n '3,45p' "${BASH_SOURCE[0]}"; exit 0 ;;
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
signed_properly=0
if [ "$identity" = "-" ]; then
    # The hardened runtime is asked for even here. Notarising requires it, and a program that breaks
    # under it breaks whether or not the signature is a real one, so the ad-hoc build is the place to
    # find that out rather than the first signed build.
    echo "  ad-hoc, with the hardened runtime (set CODESIGN_IDENTITY for a Developer ID signature)"
    codesign --force --options runtime --sign - "$app"
else
    if ! security find-identity -v -p codesigning | grep -qF "$identity"; then
        echo "CODESIGN_IDENTITY is set to \"$identity\", and no identity of that name is in the keychain." >&2
        echo "What is there:" >&2
        security find-identity -v -p codesigning >&2
        exit 1
    fi
    echo "  $identity"
    codesign --force --options runtime --timestamp --sign "$identity" "$app"
    signed_properly=1
fi
codesign --verify --deep --strict --verbose=1 "$app"
# What the system makes of it, which is the question a person downloading it will ask. Ad-hoc is
# rejected and that is expected, so this reports rather than stopping.
if spctl --assess --type execute "$app" >/dev/null 2>&1; then
    echo "  Gatekeeper accepts it"
else
    echo "  Gatekeeper rejects it, which is what an unsigned or un-notarised application gets"
fi

# ---------------------------------------------------------------------------------------------
# The disk image.
# ---------------------------------------------------------------------------------------------
dmg=""
if [ "$make_dmg" = 1 ]; then
    need hdiutil
    step "Building the disk image"
    mkdir -p "$releases"
    dmg="$releases/quill-$version.dmg"
    staging="$dist/dmg-staging"
    rm -rf "$staging" "$dmg"
    mkdir -p "$staging"
    cp -R "$app" "$staging/Quill.app"
    ln -s /Applications "$staging/Applications"
    hdiutil create -volname "Quill $version" -srcfolder "$staging" -ov -format UDZO "$dmg" >/dev/null
    rm -rf "$staging"
    # The image itself is signed as well as the application inside it, with the same identity. An
    # unsigned image round a signed application is a thing a person can be handed and told to trust,
    # and notarising one is refused.
    if [ "$signed_properly" = 1 ]; then
        codesign --force --timestamp --sign "$identity" "$dmg"
        echo "  signed the image too"
    fi
    echo "  $dmg ($(du -h "$dmg" | cut -f1))"
fi

# ---------------------------------------------------------------------------------------------
# Notarising: Apple looks at the image and sends back a ticket, which is stapled to it.
#
# The ticket is what lets a person who downloads the image open it with no warning and with no
# network connection, because the stapled ticket is checked locally.
# ---------------------------------------------------------------------------------------------
if [ "$notarize" = 1 ]; then
    step "Notarising"
    if [ "$signed_properly" != 1 ]; then
        echo "Notarising needs a Developer ID signature. Set CODESIGN_IDENTITY to one of these:" >&2
        security find-identity -v -p codesigning >&2
        exit 1
    fi
    if [ -z "$dmg" ]; then
        echo "Nothing to notarise: --notarize and --no-dmg together." >&2
        exit 2
    fi
    need xcrun

    # Either a stored profile or the three values. Neither is printed.
    credentials=()
    if [ -n "${NOTARY_PROFILE:-}" ]; then
        credentials=(--keychain-profile "$NOTARY_PROFILE")
        echo "  as the stored profile $NOTARY_PROFILE"
    elif [ -n "${NOTARY_APPLE_ID:-}" ] && [ -n "${NOTARY_TEAM_ID:-}" ] && [ -n "${NOTARY_PASSWORD:-}" ]; then
        credentials=(--apple-id "$NOTARY_APPLE_ID" --team-id "$NOTARY_TEAM_ID" --password "$NOTARY_PASSWORD")
        echo "  as $NOTARY_APPLE_ID, team $NOTARY_TEAM_ID"
    else
        cat >&2 <<'MISSING'
Notarising needs credentials for notarytool. Either store them once:

  xcrun notarytool store-credentials quill \
      --apple-id you@example.com --team-id TEAMID --password <app-specific-password>
  export NOTARY_PROFILE=quill

or pass them each time:

  export NOTARY_APPLE_ID=you@example.com NOTARY_TEAM_ID=TEAMID NOTARY_PASSWORD=<app-specific-password>

The app-specific password comes from appleid.apple.com, not the Apple ID's own password.
MISSING
        exit 1
    fi

    # --wait, because the answer is the point: without it the script would finish before Apple had
    # looked at anything and there would be nothing to staple.
    xcrun notarytool submit "$dmg" "${credentials[@]}" --wait

    step "Stapling the ticket"
    xcrun stapler staple "$dmg"
    xcrun stapler validate "$dmg"
    # The question a person downloading it asks, asked here instead. `--type open` with the primary
    # signature context is how a disk image is assessed rather than an application.
    if spctl --assess --type open --context context:primary-signature "$dmg" >/dev/null 2>&1; then
        echo "  Gatekeeper accepts the image: it will open with no warning"
    else
        echo "  Gatekeeper still rejects the image, so something above did not take" >&2
        spctl --assess --type open --context context:primary-signature --verbose=2 "$dmg" >&2 || true
        exit 1
    fi
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
if [ -n "$dmg" ]; then
    printf '      %s\n' "$dmg"
fi
