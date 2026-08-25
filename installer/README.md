# Building an installer for Quill

Everything that turns the built binary into something a person can install. One folder, two
platforms, one drawing of the icon. `tasks/quill-installer-tdd.md` says why each of these is what it
is; this says how to run them.

```
installer/
  icon/       the Quill mark: a program that draws it, and the drawn files
  windows/    the Inno Setup script, and the script that builds and installs it
  macos/      the bundle's manifest, and the script that builds Quill.app and the disk image
  dist/       what the two build scripts write. Not committed.
```

The version comes from `Cargo.toml` and nowhere else. It reaches `quill.exe`'s version block, the
installer's file name, the Add or Remove Programs entry and `Quill.app`'s `Info.plist` from there, so
releasing a new version is changing one number.

---

## Windows

```powershell
powershell -File installer\windows\build.ps1              # build the installer
powershell -File installer\windows\build.ps1 -Install     # build it and install it on this machine
powershell -File installer\windows\build.ps1 -Install -AllUsers   # into Program Files, needs admin
powershell -File installer\windows\build.ps1 -SkipBuild   # use target\release\quill.exe as it is
powershell -File installer\windows\build.ps1 -Icon        # redraw the icon first
```

It writes `installer\dist\QuillSetup-<version>-x64.exe`, about 6.5 MB.

The script installs the Inno Setup compiler with `winget` the first time, if it is not already there.
Nothing else is needed beyond what building Quill already needs: `rc.exe`, which puts the icon inside
`quill.exe`, comes with the Windows SDK that the MSVC toolchain already depends on.

**What the installer does.** A plain double click installs into `%LOCALAPPDATA%\Programs\Quill` with
no elevation prompt; the first page offers all users, which puts it in `Program Files` instead. Five
optional things, all on one page and all remembered by the uninstaller:

| | |
|---|---|
| Desktop icon | a **Quill** shortcut on the desktop |
| PATH | `quill` opens a folder from any terminal |
| Right click a file | *Open with Quill* |
| Right click a folder | *Open with Quill*, on the folder and on the empty space inside it |
| Open with | Quill is offered for `.md .markdown .txt .rs .js .ts .json .toml .yml .yaml` — offered, never taken as the default |

**Uninstalling** removes the files, the shortcuts, the `PATH` entry and every registry key, and leaves
every other `PATH` entry exactly as it was. It deliberately does **not** touch `%APPDATA%\Quill` —
the settings, the pane sizes, the recent projects and any installed plugins — because uninstalling to
install a newer version must not throw those away.

**Installing over a running Quill.** Quill does not answer the Restart Manager's request to shut down,
so setup cannot close it by itself: a person is shown the list and closes Quill, and `build.ps1
-Install` closes it for you, with the window's own close rather than a kill. See section 4 of the TDD.

---

## macOS

```bash
installer/macos/build.sh              # Quill.app in installer/dist, quill-<version>.dmg in releases
installer/macos/build.sh --install    # and copy the bundle into /Applications
installer/macos/build.sh --no-dmg     # just the bundle
installer/macos/build.sh --icon       # redraw the icon first
installer/macos/build.sh --notarize   # sign, send the image to Apple, staple the ticket to it
```

It uses nothing that is not in a stock Xcode command line install: `cargo`, `lipo`, `iconutil`,
`codesign`, `hdiutil`, `plutil`, and `notarytool` and `stapler` when notarising. For a universal binary,
have both targets installed first:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

With only one installed it builds that one and says so.

The image goes to `releases/quill-<version>.dmg`. `installer/dist/` is the working area and is
rewritten on every run.

### Signing, in three levels

The script says which one it did, and checks with `spctl` afterwards rather than leaving it to be
guessed at.

| | What it takes | What a person who downloads it sees |
|---|---|---|
| **Ad-hoc** | nothing | "Apple cannot check it for malicious software", and a right click and *Open* gets past it, once. A copy built on the machine it runs on has no quarantine flag and opens with no warning at all. |
| **Developer ID** | `CODESIGN_IDENTITY` naming an identity in the keychain | The same warning. A signature alone is not enough on macOS 10.15 and later; the notarisation is what removes it. |
| **Notarised** | that, plus `--notarize` and credentials | Nothing. It opens. |

The bundle is signed with the **hardened runtime** at every level, including ad-hoc. Notarising requires
it, and an application that breaks under it breaks whether the signature is real or not, so the ad-hoc
build is where that shows up rather than the first signed one. Quill runs under it: no entitlements are
needed, because it loads only system frameworks and the processes it starts — `git`, a shell — are not
restricted by the runtime.

### Notarising

```bash
export CODESIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)"
xcrun notarytool store-credentials quill \
    --apple-id you@example.com --team-id TEAMID --password <app-specific-password>
export NOTARY_PROFILE=quill
installer/macos/build.sh --notarize
```

`NOTARY_APPLE_ID`, `NOTARY_TEAM_ID` and `NOTARY_PASSWORD` work instead of a stored profile, for a
machine where storing one is not wanted. Nothing is printed or written by the script either way.

`--notarize` signs the application and the image with the identity, sends the image to Apple, waits for
the answer, staples the ticket to the image and then asks `spctl` and `stapler validate` whether it
took. The staple is the point: the ticket is checked locally, so the image opens for somebody with no
network.

### What is needed before any of that works

None of it can be done from a checkout alone, and each one is a thing only the account holder can do:

1. **A paid Apple Developer Program membership.** A free Apple ID cannot have a Developer ID
   Application certificate, which is the only kind of signature Gatekeeper accepts outside the App
   Store.
2. **The certificate, in the login keychain.** The short way is Xcode: *Settings*, *Accounts*, sign in,
   select the team, *Manage Certificates*, the `+`, *Developer ID Application*. It needs the Account
   Holder or Admin role. The long way is a certificate signing request from Keychain Access uploaded at
   developer.apple.com and the issued certificate downloaded and double clicked.
3. **The Team ID**, ten characters, from developer.apple.com under Membership. It is the part in
   brackets in the identity's name.
4. **An app-specific password** for the Apple ID, from appleid.apple.com under *Sign-In and Security*.
   The Apple ID's own password is refused. An App Store Connect API key works too, with `--key`.

`security find-identity -v -p codesigning` lists what the machine actually has. Until step 2 is done it
says `0 valid identities found`, and the script stops with that same list rather than producing
something that looks signed and is not.

**What has been run.** Both sides now. The Windows side was built, installed, run, uninstalled and
reinstalled on a real machine. The macOS side was run on an Apple silicon Mac on 2026-08-25 and needed
no correction: `build.sh` went from a clean checkout to `Quill.app` and `Quill-0.1.0.dmg` on the first
run, the icon was built by `iconutil`, and the two places the design document expected to want a fix,
the `rustup target list` check and the ad-hoc `codesign --deep` call, both behaved as written.

What was checked afterwards:

| | |
|---|---|
| The bundle | `Contents/MacOS/quill`, `Contents/Resources/Quill.icns`, `Contents/Info.plist`, `Contents/PkgInfo` and `_CodeSignature`, 15 MB |
| The manifest | `plutil -lint` passes, and the version, `CFBundleName` and `com.jasonmcaffee.quill` are all substituted in |
| What the system calls it | `mdls` reports the display name `Quill` and the version `0.1.0`, which is what the Dock and the menu bar read |
| The icon | a real 1024 point `icns`, by `sips` |
| The signature | `codesign --verify --deep --strict` passes on the bundle, on the copy in `/Applications` and on the copy inside the mounted image |
| The disk image | 6.7 MB, mounts, holds `Quill.app` beside an alias to `/Applications` |
| Installing | `--install` puts it in `/Applications`, `open -a` launches it, and the folder passed after `--args` reaches the window: it turns up at the top of `recent.txt` |
| Installing over a running copy | works, because a Mac replaces the bundle and leaves the running process on its old inode. There is nothing here like the Restart Manager problem the Windows side has |

**The one thing still not done** is a real identity, and it is not something a checkout can carry: this
machine has `0 valid identities found`. Everything up to that point is in place and was exercised — the
hardened runtime is on and the application runs under it, the image is built and signed, and `--notarize`
stops with the list of identities and the four steps above rather than producing something that looks
signed. The moment the certificate is in the keychain, one run does the rest.

Two smaller things left as they are, deliberately. The binary is **arm64 only** on a machine with only
that target installed — `rustup target add x86_64-apple-darwin` and another run makes it universal, and
the script says which it built. And the disk image opens as a plain Finder window rather than one with
the icons positioned and a background picture: laying that out means driving Finder with AppleScript,
which is a permission prompt and a fragile step for something that is decoration.

---

## The icon

```bash
cargo run --release --manifest-path installer/icon/Cargo.toml
```

Draws the mark once at 1024 points and writes `quill.ico`, `quill.icns` and `Quill.iconset/` beside
itself. It is a workspace of its own, so `cargo build` on Quill does not build it, and its output is
committed — `quill.ico` is a build input for `quill.exe`, so a fresh checkout has to have it already.

Change the drawing in `installer/icon/src/main.rs`, run it, and **look at the pictures** before
committing them, the same rule the screenshot tests are held to. The colours are the ones in
`theme::color`; a colour that is not in that list does not belong in the icon either.
