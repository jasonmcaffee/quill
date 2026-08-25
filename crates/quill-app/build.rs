//! Puts the icon and the version block inside `quill.exe`.
//!
//! Windows reads a program's icon, the name on its taskbar button, its description in Task Manager
//! and its version in Add or Remove Programs out of a resource block compiled into the executable.
//! An installer cannot supply one — a shortcut can point at an icon file, but the running window and
//! the taskbar button read the exe — so it is put there here.
//!
//! The icon is `installer/icon/quill.ico`, which is drawn by `installer/icon` and committed, so a
//! checkout builds without running a drawing program first.
//!
//! A machine with no Windows SDK has no `rc.exe` and cannot compile a resource. That is a warning
//! rather than an error: the build still produces a working `quill.exe`, just an unlabelled one, and
//! only the machine that builds the installer needs the labelled one.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    #[cfg(windows)]
    embed_windows_resources();
}

#[cfg(windows)]
fn embed_windows_resources() {
    // From `crates/quill-app` up to the repository, then down to the drawn icon.
    const ICON: &str = "../../installer/icon/quill.ico";
    println!("cargo:rerun-if-changed={ICON}");

    if !std::path::Path::new(ICON).exists() {
        println!("cargo:warning=quill.exe has no icon: {ICON} is missing. Run `cargo run --release --manifest-path installer/icon/Cargo.toml`.");
        return;
    }

    let mut resources = winresource::WindowsResource::new();
    resources
        .set_icon(ICON)
        .set("ProductName", "Quill")
        .set("FileDescription", "Quill")
        .set("CompanyName", "Jason McAffee")
        .set("LegalCopyright", "Licensed under MIT or Apache-2.0")
        .set("InternalName", "quill.exe")
        .set("OriginalFilename", "quill.exe");

    if let Err(problem) = resources.compile() {
        println!("cargo:warning=quill.exe has no icon or version block: {problem}");
    }
}
