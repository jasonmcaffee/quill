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
installer/macos/build.sh              # Quill.app and Quill-<version>.dmg, in installer/dist
installer/macos/build.sh --install    # and copy the bundle into /Applications
installer/macos/build.sh --no-dmg     # just the bundle
installer/macos/build.sh --icon       # redraw the icon first
```

It uses nothing that is not in a stock Xcode command line install: `cargo`, `lipo`, `iconutil`,
`codesign`, `hdiutil`, `plutil`. For a universal binary, have both targets installed first:

```bash
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

With only one installed it builds that one and says so.

**Signing.** With nothing set the bundle is signed ad-hoc, which is what makes Gatekeeper treat it as
an unsigned application to be allowed through once — right click, Open — rather than a damaged one to
be refused. With a Developer ID:

```bash
export CODESIGN_IDENTITY="Developer ID Application: Your Name (TEAMID)"
installer/macos/build.sh
xcrun notarytool submit installer/dist/Quill-<version>.dmg \
  --apple-id you@example.com --team-id TEAMID --password <app-specific-password> --wait
xcrun stapler staple installer/dist/Quill-<version>.dmg
```

**What has and has not been run.** The Windows side was built, installed, run, uninstalled and
reinstalled on a real machine. The macOS side has **not** been run: this was written on Windows with
no Mac attached. `Info.plist` is checked for well-formedness, `build.sh` passes `bash -n`, and it is
written against the documented behaviour of each tool and checks for each one before using it — but
the first run on a Mac is still a first run, and the likely places for it to want a correction are the
`rustup target list` check and the ad-hoc `codesign --deep` call.

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
