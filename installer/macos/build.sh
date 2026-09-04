#!/usr/bin/env bash
#
# Builds Unluminate.app and the disk image it is delivered in.
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
#   installer/macos/build.sh                # build Unluminate.app and releases/unluminate-<version>.dmg
#   installer/macos/build.sh --install      # and copy the bundle into /Applications
#   installer/macos/build.sh --no-dmg       # just the bundle
#   installer/macos/build.sh --icon         # redraw the icon first (needs a Rust toolchain)
#   installer/macos/build.sh --notarize     # sign, send it to Apple, staple the ticket to the image
#
# The image goes to `releases/unluminate-<version>.dmg`, named from the version in Cargo.toml, so that past
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
app="$dist/Unluminate.app"
# Where a finished image is kept. `installer/dist` is the working area and is rewritten on every run;
# this is what is kept, and every file in it carries the version it was built from.
releases="$repo/releases"

# The identity and the notarising credentials, written down once rather than exported by hand every time.
# Ignored by git, because the key it points at is a credential and the rest are account details.
# `notarize.env.example` beside it says where each value comes from. Anything already in the environment
# wins, so a one-off run can still override it.
if [ -f "$here/notarize.env" ]; then
    # **Which is not what a plain `.` does.** The file holds ordinary assignments, so sourcing it overwrote
    # whatever the environment already had and a one-off `CODESIGN_IDENTITY=- ./build.sh` was ignored — the
    # run then failed asking for an identity the person had deliberately overridden. So what was set before
    # is remembered and put back after. Named one at a time rather than done cleverly, because these four
    # are what the file sets and a loop over the file's contents would be a way of running it twice.
    for kept in CODESIGN_IDENTITY NOTARY_PROFILE NOTARY_KEY NOTARY_KEY_ID; do
        eval "had_$kept=\${$kept+yes}"
        eval "was_$kept=\${$kept-}"
    done
    # shellcheck disable=SC1091
    . "$here/notarize.env"
    for kept in CODESIGN_IDENTITY NOTARY_PROFILE NOTARY_KEY NOTARY_KEY_ID; do
        eval "if [ -n \"\$had_$kept\" ]; then $kept=\"\$was_$kept\"; fi"
    done
fi

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

# ---------------------------------------------------------------------------------------------
# Notarising, in two submissions: the application, then the image holding it.
#
# The order matters and it is the reason this is not one step. A notarisation ticket has to be stapled
# to the thing it covers, and what a person ends up running is the application they dragged out of the
# image — so the application has to carry its own ticket, and it has to be stapled before the image is
# built round it. Then the image is notarised in its turn, so that the image itself opens without a
# warning as well. A stapled ticket is checked locally, which is what makes both work with no network.
# ---------------------------------------------------------------------------------------------
notary_credentials=()

# Work out how to talk to the notary service, storing the keychain profile the first time.
prepare_notarising() {
    if [ "$signed_properly" != 1 ]; then
        echo "Notarising needs a Developer ID signature. Set CODESIGN_IDENTITY to one of these:" >&2
        security find-identity -v -p codesigning >&2
        exit 1
    fi
    need xcrun

    # The profile is stored the first time, from an App Store Connect API key or from an app-specific
    # password, so that the values are looked up once and never again. After that the profile's name is
    # the only thing needed.
    if [ -n "${NOTARY_PROFILE:-}" ] && ! xcrun notarytool history --keychain-profile "$NOTARY_PROFILE" >/dev/null 2>&1; then
        if [ -n "${NOTARY_KEY:-}" ] && [ -n "${NOTARY_KEY_ID:-}" ] && [ -n "${NOTARY_ISSUER:-}" ]; then
            echo "  storing the profile $NOTARY_PROFILE from the API key $NOTARY_KEY_ID"
            if [ ! -f "$NOTARY_KEY" ]; then
                echo "NOTARY_KEY points at $NOTARY_KEY, which is not there." >&2
                exit 1
            fi
            xcrun notarytool store-credentials "$NOTARY_PROFILE" \
                --key "$NOTARY_KEY" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER" >/dev/null
        elif [ -n "${NOTARY_APPLE_ID:-}" ] && [ -n "${NOTARY_TEAM_ID:-}" ] && [ -n "${NOTARY_PASSWORD:-}" ]; then
            echo "  storing the profile $NOTARY_PROFILE from the app-specific password"
            xcrun notarytool store-credentials "$NOTARY_PROFILE" \
                --apple-id "$NOTARY_APPLE_ID" --team-id "$NOTARY_TEAM_ID" --password "$NOTARY_PASSWORD" >/dev/null
        fi
    fi

    if [ -n "${NOTARY_PROFILE:-}" ] && xcrun notarytool history --keychain-profile "$NOTARY_PROFILE" >/dev/null 2>&1; then
        notary_credentials=(--keychain-profile "$NOTARY_PROFILE")
        echo "  as the stored profile $NOTARY_PROFILE"
    elif [ -n "${NOTARY_APPLE_ID:-}" ] && [ -n "${NOTARY_TEAM_ID:-}" ] && [ -n "${NOTARY_PASSWORD:-}" ]; then
        notary_credentials=(--apple-id "$NOTARY_APPLE_ID" --team-id "$NOTARY_TEAM_ID" --password "$NOTARY_PASSWORD")
        echo "  as $NOTARY_APPLE_ID, team $NOTARY_TEAM_ID"
    else
        cat >&2 <<'MISSING'
Notarising needs credentials for notarytool, and the place to put them is
installer/macos/notarize.env, which is ignored by git:

  cp installer/macos/notarize.env.example installer/macos/notarize.env

Then fill in either an App Store Connect API key, from appstoreconnect.apple.com under Users and
Access, Integrations:

  NOTARY_KEY, NOTARY_KEY_ID, NOTARY_ISSUER

or an app-specific password, from appleid.apple.com under Sign-In and Security:

  NOTARY_APPLE_ID, NOTARY_TEAM_ID, NOTARY_PASSWORD

An Apple ID's own password is refused, and app-specific passwords only exist on an account with
two-factor authentication turned on. The profile named by NOTARY_PROFILE is stored from whichever of
the two is filled in, on the first run.
MISSING
        exit 1
    fi
}

# Send something to Apple, wait for the answer, and staple the ticket to it.
#
# --wait, because the answer is the point: without it the script would finish before Apple had looked
# at anything and there would be nothing to staple.
notarise() {
    local what="$1" staple_to="$2"
    xcrun notarytool submit "$what" "${notary_credentials[@]}" --wait
    xcrun stapler staple "$staple_to"
    xcrun stapler validate "$staple_to"
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
    python3 -c 'import json,sys; print(next(p["version"] for p in json.load(sys.stdin)["packages"] if p["name"]=="unluminate-app"))'
)"
echo "Unluminate $version"

if [ "$draw_icon" = 1 ]; then
    step "Drawing the icon"
    cargo run --release --manifest-path "$repo/installer/icon/Cargo.toml"
fi

# ---------------------------------------------------------------------------------------------
# The binaries. Universal when both targets are installed, and whichever one is when only one is.
#
# Two of them: the editor, and `unluminate-cli`, which drives a running one from a terminal. The command
# line goes inside the bundle beside the editor because that is where it looks for it — see
# `unluminate_cli::client::unluminate_program` — so putting `Unluminate.app/Contents/MacOS` on the PATH, or making
# one symlink into it, gives you both with nothing configured.
# ---------------------------------------------------------------------------------------------
step "Building unluminate and unluminate-cli"
targets=()
for target in aarch64-apple-darwin x86_64-apple-darwin; do
    if rustup target list --installed 2>/dev/null | grep -qx "$target"; then
        targets+=("$target")
    fi
done

built=()
built_cli=()
if [ "${#targets[@]}" -eq 0 ]; then
    echo "Neither Apple target is installed; building for this machine only."
    echo "For a universal binary: rustup target add aarch64-apple-darwin x86_64-apple-darwin"
    cargo build --release --manifest-path "$repo/Cargo.toml" -p unluminate-app --bin unluminate
    cargo build --release --manifest-path "$repo/Cargo.toml" -p unluminate-cli --bin unluminate-cli
    built+=("$repo/target/release/unluminate")
    built_cli+=("$repo/target/release/unluminate-cli")
else
    for target in "${targets[@]}"; do
        echo "  $target"
        cargo build --release --target "$target" --manifest-path "$repo/Cargo.toml" -p unluminate-app --bin unluminate
        cargo build --release --target "$target" --manifest-path "$repo/Cargo.toml" -p unluminate-cli --bin unluminate-cli
        built+=("$repo/target/$target/release/unluminate")
        built_cli+=("$repo/target/$target/release/unluminate-cli")
    done
fi

# ---------------------------------------------------------------------------------------------
# The bundle. Four files and a directory tree, which is why it is built here rather than by a tool.
# ---------------------------------------------------------------------------------------------
step "Building Unluminate.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

if [ "${#built[@]}" -gt 1 ]; then
    lipo -create -output "$app/Contents/MacOS/unluminate" "${built[@]}"
    lipo -create -output "$app/Contents/MacOS/unluminate-cli" "${built_cli[@]}"
    echo "  universal: $(lipo -archs "$app/Contents/MacOS/unluminate")"
else
    cp "${built[0]}" "$app/Contents/MacOS/unluminate"
    cp "${built_cli[0]}" "$app/Contents/MacOS/unluminate-cli"
    echo "  single architecture: $(lipo -archs "$app/Contents/MacOS/unluminate")"
fi
chmod +x "$app/Contents/MacOS/unluminate" "$app/Contents/MacOS/unluminate-cli"

# The icon. `iconutil` is Apple's own tool and so is the definition of the format; the committed
# unluminate.icns is the fallback, so that the icon can also be rebuilt on a machine that is not a Mac.
# The committed unluminate.icns is used when iconutil is absent **and when it refuses the iconset**. It refuses
# this one on macOS 26 with `Invalid Iconset` and says nothing about which file it objects to; every image
# is present, every one is the size its name claims, and a clean copy with no extended attributes is
# refused as well. Aborting the whole build over the icon meant a working editor could not be installed at
# all, which is a worse outcome than an icon built the other way.
if command -v iconutil >/dev/null 2>&1 && [ -d "$repo/installer/icon/Unluminate.iconset" ] \
    && iconutil --convert icns --output "$app/Contents/Resources/Unluminate.icns" \
        "$repo/installer/icon/Unluminate.iconset" 2>/dev/null; then
    echo "  icon built by iconutil"
else
    cp "$repo/installer/icon/unluminate.icns" "$app/Contents/Resources/Unluminate.icns"
    echo "  icon taken from the committed unluminate.icns, because iconutil is absent or refused the iconset"
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
# Inside out. `unluminate-cli` is a second Mach-O binary inside `Contents/MacOS`, and codesign treats a
# nested binary as something that must carry its own signature: sealing it as though it were a
# resource is what makes `codesign --verify --deep --strict` fail on a bundle that looked signed.
# So it is signed first, and then the bundle round it.
sign_the_cli() {
    if [ "$1" = "-" ]; then
        codesign --force --options runtime --sign - "$app/Contents/MacOS/unluminate-cli"
    else
        codesign --force --options runtime --timestamp --sign "$1" "$app/Contents/MacOS/unluminate-cli"
    fi
}
if [ "$identity" = "-" ]; then
    # The hardened runtime is asked for even here. Notarising requires it, and a program that breaks
    # under it breaks whether or not the signature is a real one, so the ad-hoc build is the place to
    # find that out rather than the first signed build.
    echo "  ad-hoc, with the hardened runtime (set CODESIGN_IDENTITY for a Developer ID signature)"
    sign_the_cli -
    codesign --force --options runtime --sign - "$app"
else
    if ! security find-identity -v -p codesigning | grep -qF "$identity"; then
        echo "CODESIGN_IDENTITY is set to \"$identity\", and no identity of that name is in the keychain." >&2
        echo "What is there:" >&2
        security find-identity -v -p codesigning >&2
        exit 1
    fi
    echo "  $identity"
    sign_the_cli "$identity"
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
# The application's own notarisation, before the image is built round it.
# ---------------------------------------------------------------------------------------------
if [ "$notarize" = 1 ]; then
    step "Notarising the application"
    prepare_notarising
    # A zip to send, made with ditto because it keeps the symlinks and the metadata a bundle needs and
    # `zip` does not. It is only the parcel: what gets stapled is the bundle itself.
    parcel="$dist/Unluminate.app.zip"
    rm -f "$parcel"
    ditto -c -k --sequesterRsrc --keepParent "$app" "$parcel"
    notarise "$parcel" "$app"
    rm -f "$parcel"
    if spctl --assess --type execute --verbose=2 "$app" >/dev/null 2>&1; then
        echo "  Gatekeeper accepts the application, and its ticket is stapled to it"
    else
        echo "  Gatekeeper still rejects the application" >&2
        spctl --assess --type execute --verbose=2 "$app" >&2 || true
        exit 1
    fi
fi

# ---------------------------------------------------------------------------------------------
# The disk image, holding the application that now carries its ticket.
# ---------------------------------------------------------------------------------------------
dmg=""
if [ "$make_dmg" = 1 ]; then
    need hdiutil
    step "Building the disk image"
    mkdir -p "$releases"
    dmg="$releases/unluminate-$version.dmg"
    staging="$dist/dmg-staging"
    rm -rf "$staging" "$dmg"
    mkdir -p "$staging"
    cp -R "$app" "$staging/Unluminate.app"
    ln -s /Applications "$staging/Applications"
    hdiutil create -volname "Unluminate $version" -srcfolder "$staging" -ov -format UDZO "$dmg" >/dev/null
    rm -rf "$staging"
    # The image itself is signed as well as the application inside it, with the same identity. An
    # unsigned image round a signed application is a thing a person can be handed and told to trust,
    # and notarising one is refused.
    if [ "$signed_properly" = 1 ]; then
        codesign --force --timestamp --sign "$identity" "$dmg"
        echo "  signed the image too"
    fi
    echo "  $dmg ($(du -h "$dmg" | cut -f1))"

    if [ "$notarize" = 1 ]; then
        step "Notarising the image"
        notarise "$dmg" "$dmg"
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
fi

if [ "$notarize" = 1 ] && [ -z "$dmg" ] && [ "$make_dmg" = 1 ]; then
    echo "Nothing to notarise: --notarize and --no-dmg together leave only the application." >&2
fi

# ---------------------------------------------------------------------------------------------
# Installing, which on a Mac is a copy.
# ---------------------------------------------------------------------------------------------
if [ "$install_it" = 1 ]; then
    step "Installing into /Applications"
    rm -rf "/Applications/Unluminate.app"
    cp -R "$app" "/Applications/Unluminate.app"
    # Launch Services will not notice a new bundle on its own if one was there a moment ago.
    /System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
        -f "/Applications/Unluminate.app" || true
    echo "  /Applications/Unluminate.app"
fi

printf '\nDone. %s\n' "$app"
if [ -n "$dmg" ]; then
    printf '      %s\n' "$dmg"
fi
