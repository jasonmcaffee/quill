# Releases

The finished installers, one file per version, named after the version in `Cargo.toml`:

```
releases/
  quill-0.1.0.dmg          macOS: the application and an alias to /Applications
  QuillSetup-0.1.0-x64.exe Windows: the Inno Setup installer
```

`installer/macos/build.sh` and `installer/windows/build.ps1` write here. `installer/dist/` is their
working area — the bundle, the staging folder for the image — and is rewritten on every run; this is what
is kept.

The name carries the version so that two builds cannot be confused for one another, and so that an
older version can be handed to somebody who needs it. Releasing a new version is changing the number in
`Cargo.toml`: it reaches the executable's version block, the bundle's `Info.plist`, the Windows
installer's Add or Remove Programs entry and the file names here from that one place.

## What a person who receives one of these sees

**macOS.** `quill-0.1.0.dmg` is signed with a Developer ID and notarised, with the ticket stapled to the
image and to the application inside it, so opening the image and dragging the application across is all
there is to it: no warning, and no network needed to check it. A build made without the certificate is
signed ad-hoc instead, and macOS then says it cannot check the application for malicious software, which
takes a right click on `Quill.app` and *Open*, once. Which of the two a file is can be read from the file
itself:

```bash
spctl --assess --type open --context context:primary-signature --verbose=2 releases/quill-0.1.0.dmg
xcrun stapler validate releases/quill-0.1.0.dmg
```

**Windows.** The installer is unsigned, so SmartScreen offers *More info* and *Run anyway* the first
time a new version is downloaded. Signing it needs a code signing certificate, which is a separate thing
from an Apple one.

`installer/README.md` says how each of these is built and what is needed to sign them.
