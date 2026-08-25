# Shipping Quill: an installer for Windows and for macOS

A technical design for what `task-1650` asks for — a folder of its own that turns the built binary
into something a person can install — and a record of what was chosen, what was rejected, and why.

Until now Quill is run with `cargo run --release`. That is fine for the person writing it and no use
to anybody else: there is no icon on the exe, nothing in the Start Menu, nothing in Add or Remove
Programs, and on a Mac there is no bundle, so the window has no name in the Dock and the menu bar has
nowhere to hang. This closes that gap on both platforms, from one folder, without changing how the
application itself behaves.

---

## 0. What this keeps to

`CLAUDE.md` says what each crate is for and what must never be in it. **None of that changes.** The
installer packages what the build already produces; it does not reach into the application. The one
exception is deliberate and is argued for in section 3: `quill.exe` gains an icon and a version
block, because a Windows program without them is an unlabelled generic window in the taskbar and a
blank entry in Add or Remove Programs, and no installer can put that right from the outside.

Three rules the rest of this follows.

**One folder, two platforms, one source for the picture.** The Windows installer and the macOS bundle
must not each carry their own drawing of a quill, or they will drift apart. There is one generator
and it writes every size and both container formats.

**The build is a script, not a list of steps in a document.** `installer/windows/build.ps1` and
`installer/macos/build.sh` each go from a clean checkout to a file that can be handed to somebody.
Anything a person would otherwise have to remember — install the compiler, make the icon, stamp the
version — is in the script.

**Nothing is invented that the machine already knows.** The same argument `quill-git` makes for
shelling out to `git`: the platform has an installer format, a shortcut format, an uninstall
registry and a bundle layout, and Quill should produce exactly those rather than something adjacent
to them.

---

## 1. The shape of it

```
installer/
  README.md            how to build an installer, on either platform
  icon/                the Quill mark: the generator, and its output
    Cargo.toml         its own workspace, so `cargo build` at the root does not build it
    src/main.rs        draws the mark and writes every size and both containers
    quill.ico          committed, so a build needs no generator
    quill.icns         committed, for the same reason
    Quill.iconset/     the PNG set, in the names `iconutil` on a Mac reads
  windows/
    quill.iss          the Inno Setup script
    license.txt        shown on the licence page
    build.ps1          cargo build -> ISCC -> dist/QuillSetup-<version>-x64.exe
  macos/
    Info.plist         the bundle's manifest, with the version substituted in
    build.sh           cargo build -> Quill.app -> dist/Quill-<version>.dmg
  dist/                what the two scripts write. Not committed.
```

`installer/` sits beside `crates/`, `design/` and `tools/` rather than inside any of them, because it
is not part of any crate and it is not a design document. `tools/` was considered — it already holds
`build-git-demo.ps1` — and rejected: `tools/` is scripts that help while working on Quill, and this
is how Quill is delivered.

---

## 2. The mark

Quill has a palette read out of a screenshot and a rule that icons are drawn rather than lettered,
and the application icon should be the same thing at a larger size. So it is drawn, by a program, at
every size the two platforms ask for.

**What is drawn.** A rounded square in the window's own two darks — `EDITOR` `#1A1F26` at the bottom
lifting to `TITLE_BAR` `#2A313D` at the top, which is the vertical order the window itself has — with
a hairline of `CONTROL_BORDER` round the edge so the icon keeps its shape against a light wallpaper.
On it, a quill: a feather in `TEXT` `#E8EBF1` running from the top right down to a nib at the lower
left, its underside in `TEXT_DIM` so it reads as a surface rather than a cut-out, barbs cut into its
front edge and a shaft down its middle in the plate's own dark, and out of the nib a stroke of
`ACCENT` `#489FF8` — the colour everything switched on is drawn in — as the ink it has just laid
down. Every colour is a named one out of `theme::color` and none was chosen at the icon; the plate's
corner is the window's corner.

**How it is drawn.** `tiny-skia`, which is a path rasteriser with anti-aliasing and gradients and no
system dependencies at all — it is the renderer `resvg` is built on. The alternatives were an SVG
committed to the repository and rasterised by whatever is to hand, which makes the build depend on a
tool that is on one machine and not another, and hand-written pixels, which cannot be redrawn at 1024
points. A program that draws paths can be read, corrected and re-run.

The mark is drawn **once at 1024 points and downsampled** with a box filter for every smaller size,
rather than drawn again at each size. Drawing a 3 point stroke at 16 points gives a smear; a
downsample of a large drawing gives the soft, legible mark that every other application's 16 point
icon is.

**The two containers are written by hand**, because both are simple and neither has a crate worth the
dependency:

- **`.ico`** — a six byte header, a sixteen byte directory entry per image, then the images. Sizes 16,
  24, 32, 48, 64, 128 and 256. The three smallest are written as a DIB (a `BITMAPINFOHEADER` with
  double height, bottom-up BGRA rows and an AND mask that is all zero because the alpha channel does
  the masking) and the rest as PNG. Windows has read PNG entries since Vista, but a DIB is what every
  reader has always understood, and at 16 points a DIB is smaller anyway.
- **`.icns`** — the four bytes `icns`, a big-endian total length, then one chunk per image: a four
  byte type, a big-endian length, and a PNG. `ic04` and `ic05` for 16 and 32, `ic07` through `ic10`
  for 128 up to 1024, and `ic11` through `ic14` for the retina pairs. macOS has taken PNG in these
  chunks since 10.8; the older `is32`/`s8mk` pair is run-length encoded and there is nothing left
  that needs it.

`build.sh` still prefers `iconutil` when it is on the machine, feeding it `icon/Quill.iconset`, whose
file names are Apple's own `icon_<size>x<size>[@2x].png`, and falls back to the committed
`quill.icns`. Apple's tool is the definition of the format; the hand-written file is there so that the
icon can be rebuilt on a machine that is not a Mac, and so that a checkout with no generator run still
has one.

**The output is committed.** `quill.ico` is a build input for `quill.exe` itself (section 3) and the
generator is a second workspace with its own dependencies. A checkout must build without running it.

---

## 3. `quill.exe` carries its own icon and version

Windows takes a program's icon, its name in the taskbar, its description in Task Manager and its
version in Add or Remove Programs from a resource block inside the executable. An installer cannot
supply one: the shortcut it writes can point at an icon file, but the running window, the taskbar
button and the Alt-Tab entry all read the exe. So `quill.exe` gets one.

`crates/quill-app/build.rs`, active only on Windows, uses `winresource` to compile a resource script
with `rc.exe` from the Windows SDK — which is already on any machine that can link a `windows-msvc`
target, because `link.exe` and `rc.exe` ship together. It sets the icon to
`installer/icon/quill.ico` and fills in `ProductName`, `FileDescription`, `CompanyName`,
`LegalCopyright` and both version fields from `CARGO_PKG_VERSION`, so the version is stamped in one
place — `Cargo.toml` — and reaches the exe, the installer's file name, its Add or Remove Programs
entry and the macOS bundle from there.

The build script is written so that a missing `rc.exe` is a warning and not an error. A build with no
Windows SDK still produces a working `quill.exe`, just an unlabelled one; only the installer needs
the labelled one, and the machine that builds the installer has the SDK.

Two things were rejected. **Leaving the icon to the shortcut** gives an application whose window and
taskbar button are the generic exe icon, which is exactly the thing that makes a program look
unfinished. **Patching the resource into the exe after the link** would mean writing a PE editor, to
avoid a build dependency that is one line and already present.

---

## 4. Windows: Inno Setup

Four ways to produce a Windows installer were weighed.

| | What it gives | Why not |
|---|---|---|
| **Write our own, in Rust** | No external tool; the repository is already Rust. | Everything an installer does would have to be re-implemented: the elevation manifest, `.lnk` files through COM, the uninstall key, upgrade detection, and an uninstaller that can delete the directory it is running from. That is a lot of new, un-exercised code to solve a solved problem, and every one of those pieces is a way to leave a machine in a half-installed state. |
| **WiX, producing an MSI** | The format group policy and enterprise deployment want. | Needs the .NET SDK and the `wix` tool, neither of which is on this machine. Per-user MSIs are awkward, authoring is verbose, and nothing here needs a transform or an administrative install. |
| **NSIS** | Small, scriptable, no runtime. | Its script language is a stack machine with a plugin system, and everything below is one directive in Inno Setup. |
| **Inno Setup** ✅ | One `.iss`; a single self-contained exe; shortcuts, uninstall registration, upgrade in place, per-user or per-machine, file associations and a `PATH` entry are all directives. | It is another tool to have installed. `build.ps1` installs it with `winget` when it is missing, so that is one line the first time and nothing thereafter. |

**Inno Setup**, then, at 6.7. What `quill.iss` says, and why:

- **Per-user by default, per-machine offered.** `PrivilegesRequired=lowest` with
  `PrivilegesRequiredOverridesAllowed=dialog`, so a plain double click installs into
  `%LOCALAPPDATA%\Programs\Quill` with no elevation prompt at all, and the dialog offers all users
  for anybody who wants `Program Files`. This is what Visual Studio Code and the other editors of
  this shape do, and it is the difference between an install that is a double click and one that is
  a permission argument.
- **A stable `AppId` GUID.** It is the key everything upgrade-related hangs off: the same GUID means
  the next version replaces this one and Add or Remove Programs shows one entry rather than two.
- **`CloseApplications=yes`**, so upgrading while Quill is open asks to close it through the Restart
  Manager rather than failing on a locked file.
- **Four optional tasks**, all offered on one page: a desktop icon, `quill` on the `PATH`, an *Open
  with Quill* entry on the right click menu of a folder and of a file, and the file associations.
- **Associations are polite.** Quill is registered as an application that *can* open `.md`,
  `.markdown`, `.txt`, `.rs`, `.js`, `.ts`, `.json`, `.toml` and `.yml` — an `OpenWithProgids` entry
  and a `SupportedTypes` list, so it appears in *Open with* and in *Choose another app*. It does not
  take the default association for any of them. A text editor that silently becomes the owner of
  `.json` is a text editor people uninstall.
- **Everything it writes, it removes.** The `PATH` entry, the shell keys and the association entries
  are all `uninsdelete`, and the install directory is `uninsalwaysuninstall`. What the uninstaller
  does **not** touch is `%APPDATA%\Quill` — the settings, the pane sizes, the recent projects and any
  plugins installed from the marketplace. Uninstalling to install a newer version must not throw away
  the person's settings, and there is no case where a silent deletion of their files is the right
  default.
- **`lzma2/max`, solid.** The payload is one 16 MB executable that compresses to about a third of
  that.

### Installing over a copy that is running

Inno asks the Restart Manager which programs are holding the files it is about to replace, and
**Quill does not answer it**. The Restart Manager sends a running program a request to shut down;
Quill's window ignores it, so setup reports "Setup was unable to automatically close all
applications" and, run silently, stops with exit code 5. That was found by trying it, not reasoned
about.

There are three answers and only one of them is right.

`CloseApplications=force` would have the Restart Manager terminate Quill outright. **No**: it would
throw away unsaved changes in a text editor, silently, to save a person one click.

Teaching Quill to answer would be the real fix, and it is a change to the application rather than to
its packaging — it belongs in `quill-app`, not here, and it also matters for a Windows restart, which
today force-closes Quill for exactly the same reason. It is written down here as the follow-up it is.

So the installer stays polite — `CloseApplications=yes` shows a person the list and lets them close
Quill and press Retry — and `build.ps1 -Install`, which is our own automation and not a person, closes
a Quill running from the folder it is about to write to first, with the window's own close rather than
a kill, and gives up with a message if it will not go. The politeness is in the installer where a
stranger meets it, and the convenience is in the script where we do.

`build.ps1` does the whole run: find or install Inno Setup, `cargo build --release`, read the version
out of `Cargo.toml`, run `ISCC` with the version and the binary's path passed in as `/D` defines, and
report the file it wrote to `installer/dist/`. It takes `-Install` to run the result silently, which
is how this ticket's *install it on my Windows computer* is done and repeated.

---

## 5. macOS: a bundle and a disk image

A Mac needs two things that Windows does not. A **bundle**, because an application that is a bare
executable has no icon in the Dock, no name in the menu bar and no way to be dragged into
`/Applications` — and Quill in particular asks the operating system for a transparent, undecorated
window and installs a menu bar along the top of the screen, both of which want a real bundle
identity. And a **disk image**, because that is how a Mac application that is not on the App Store is
delivered: a window with the application on the left and an alias to `/Applications` on the right,
and the person drags one onto the other.

`build.sh` builds the layout by hand — `Contents/MacOS/quill`, `Contents/Resources/Quill.icns`,
`Contents/Info.plist`, `Contents/PkgInfo` — because it is four files and a directory tree, and every
tool that would do it instead (`cargo-bundle`, `cargo-packager`, `create-dmg`) is a dependency to
install and a layer between the script and a format that is this simple.

The plist declares `CFBundleIdentifier` `com.jasonmcaffee.quill`, the version from `Cargo.toml` in
both `CFBundleShortVersionString` and `CFBundleVersion`, `NSHighResolutionCapable` so the window is
not scaled up from 1x, `LSMinimumSystemVersion` 11.0, and `CFBundleDocumentTypes` for the same file
kinds the Windows side registers, as `Alternate` role viewers rather than owners, for the same
reason. `NSSupportsAutomaticGraphicsSwitching` is on, so a laptop with two graphics chips does not
run the discrete one for a text editor.

**A universal binary when it can.** If both `aarch64-apple-darwin` and `x86_64-apple-darwin` are
installed the script builds both and joins them with `lipo`; if only one is, it builds that one and
says which. Either way the bundle works on the machine that built it.

**Signing.** The script signs the bundle ad-hoc — `codesign --sign -` — which is what makes Gatekeeper
treat it as an unsigned application to be allowed through once rather than a damaged one to be
refused outright. Real signing and notarisation need an Apple Developer identity, so the script uses
`$CODESIGN_IDENTITY` when it is set in the environment and falls back to ad-hoc when it is not, and
`README.md` records the two `notarytool` commands that finish the job. That is the line this ticket
stops at, and it is where it says it stops: everything up to the identity is done.

The disk image is `hdiutil create` over a staging folder holding `Quill.app` and a symlink to
`/Applications`, converted to a compressed read-only `UDZO` image.

**Run on a Mac, and what came of it.** This section was written on Windows with no Mac attached, and
said so. It has since been run on an Apple silicon Mac, on 2026-08-25, and needed no correction:
`build.sh` went from a checkout to `Quill.app` and `Quill-0.1.0.dmg` on the first run. Both of the
places this document expected to want a fix — the `rustup target list` check and the ad-hoc
`codesign --deep` call — behaved as written, and `iconutil` built the icon rather than the committed
`quill.icns` being used.

What that run showed, beyond the two files existing. `mdls` reports the bundle's display name as
`Quill` and its version as `0.1.0`, which is what the Dock and the menu bar read, so the bundle does
the job it was added for. `codesign --verify --deep --strict` passes on the bundle, on the copy in
`/Applications` and on the copy inside the mounted image. The image mounts with `Quill.app` beside an
alias to `/Applications`. The installed copy launches with `open -a`, takes the folder given after
`--args`, and writes it to the top of the recent projects list, which is how a test on a machine with
no view of the screen can tell that the installed bundle ran and did something rather than merely
starting.

Installing over a running copy needs nothing on a Mac. The bundle is replaced and the running process
carries on from its old inode until it is quit, so the Restart Manager problem section 4 records for
Windows has no counterpart here.

**What is still not done, and honestly.** A real identity. Ad-hoc signing means `spctl --assess`
rejects the bundle, so a person who downloads the image is told that Apple cannot check it and has to
right click and choose Open once; a copy built on the machine it runs on carries no quarantine flag and
opens with no prompt. That was checked both ways round rather than assumed: the quarantine flag was set
on a copy of the image by hand and `spctl` asked again. Finishing it needs an Apple Developer identity,
`CODESIGN_IDENTITY`, and the two `notarytool` commands in `installer/README.md`.

Two smaller things are left as they are on purpose. The binary is arm64 only unless
`x86_64-apple-darwin` is installed as well, which the script says when it happens. And the image opens
as a plain Finder window rather than one with the icons positioned over a background picture, because
laying that out means driving Finder with AppleScript: a permission prompt and a fragile step, for
decoration.

---

## 6. How this is verified

Layer 4 of the testing in `CLAUDE.md` — the real application — is the only layer that can say
anything about an installer, so that is where the evidence is.

1. `build.ps1` produces `installer/dist/QuillSetup-<version>-x64.exe`.
2. It is installed silently on this machine, and then: the Start Menu has **Quill**, the desktop has
   **Quill**, `quill --print-menus` answers from a new shell, *Open with* on a `.md` file lists Quill,
   right clicking a folder offers *Open with Quill*, and Add or Remove Programs has one entry with
   the icon, the version and a publisher.
3. `quill.exe` is run from the installed location, on a real folder, and the window comes up — which
   is the only way to know the installed copy is not missing something the `target/release` copy had
   beside it.
4. It is uninstalled, and the install directory, the shortcuts, the `PATH` entry and the registry
   keys are all checked to be gone, while `%APPDATA%\Quill` is checked to still be there.
5. It is installed again over itself, to prove the upgrade path.

The same five steps on macOS, all of which have now been done:

1. `installer/macos/build.sh` produces `installer/dist/Quill.app` and `installer/dist/Quill-<version>.dmg`.
2. The bundle is checked: the four files and `_CodeSignature`, `plutil -lint` on the manifest, the
   substituted version and identifier, `sips` on the icon, `lipo -archs` on the binary, and `mdls` for
   the name and version the system will show.
3. The image is mounted and holds `Quill.app` beside an alias to `/Applications`, with the signature
   still valid inside it.
4. `--install` copies it to `/Applications`, `open -a` launches it, and the folder passed to it turns up
   at the top of `recent.txt` — which is how a machine with no view of its own screen can tell the
   installed copy ran and did something.
5. `--install` again over the running copy, which succeeds, and the bundle on disk verifies afterwards.

The screenshot tests are untouched by any of this, and must stay green: `build.rs` adds a resource to
a binary and does not change a pixel the window draws.
