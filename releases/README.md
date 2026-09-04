# Releases

**The downloads live on the GitHub releases page**, one release per version:
<https://github.com/jasonmcaffee/unluminate/releases>. This folder is where the file is staged on its way
there.

```
releases/
  UnluminateSetup-0.2.0-x64.exe   Windows: the Inno Setup installer, uploaded to the v0.2.0 release
  unluminate-0.1.0.dmg            macOS: the application and an alias to /Applications
```

`installer/macos/build.sh` and `installer/windows/build.ps1` write the installers; `installer/dist/`
is their working area and is rewritten on every run. `tools/release.ps1` copies the finished file
here and uploads it, and **the `.exe` and `.dmg` files here are not committed** — a 7 MB installer per
finished task is not something a git repository should carry, and a release asset is a better place
for something a person downloads. (`unluminate-0.1.0.dmg` predates that decision and is still tracked.)

The name carries the version so that two builds cannot be confused for one another, and so that an
older version can be handed to somebody who needs it. Releasing a new version is
`pwsh tools/release.ps1`: it changes the number in `Cargo.toml`, which reaches the executable's
version block, the bundle's `Info.plist`, the Windows installer's Add or Remove Programs entry and
the file names here from that one place.

**Which build am I running?** `Unluminate -> About Unluminate` says, and so does `unluminate-cli status --json`:
both carry the version and the date the binary was built.

## What a person who receives one of these sees

**macOS.** `unluminate-0.1.0.dmg` is signed with a Developer ID and notarised, with the ticket stapled to the
image and to the application inside it, so opening the image and dragging the application across is all
there is to it: no warning, and no network needed to check it. A build made without the certificate is
signed ad-hoc instead, and macOS then says it cannot check the application for malicious software, which
takes a right click on `Unluminate.app` and *Open*, once. Which of the two a file is can be read from the file
itself:

```bash
spctl --assess --type open --context context:primary-signature --verbose=2 releases/unluminate-0.1.0.dmg
xcrun stapler validate releases/unluminate-0.1.0.dmg
```

**Windows.** The installer is unsigned, so SmartScreen offers *More info* and *Run anyway* the first
time a new version is downloaded. Signing it needs a code signing certificate, which is a separate thing
from an Apple one.

`installer/README.md` says how each of these is built and what is needed to sign them.
