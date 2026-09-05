//! Getting `unluminous --version` to reach the terminal that asked for it, on Windows.
//!
//! A released Unluminous is built `windows_subsystem = "windows"`, because a text editor started from
//! the desktop must not bring a black console window up beside itself. The cost of that is that the
//! process starts with **no console at all**: `println!` writes to an invalid handle and the bytes go
//! nowhere. So `--version`, `--help` and `--print-menus` each printed into nothing when they were run
//! from a terminal, which is the same as not answering — `task-1812` is the ticket that reported the
//! window having no `--version` when `unluminous-cli` has one, and an answer nobody can read would
//! not have fixed it.
//!
//! [`attach_to_the_calling_terminal`] borrows the console of whatever started this process. It is
//! called **only on the paths that print one answer and exit**, never on the path that opens a
//! window: a window that attached itself to the terminal it was launched from would spend the rest of
//! its life writing eframe's and wgpu's chatter over that person's prompt.
//!
//! Two steps rather than one, because `AttachConsole` alone is not enough. It only fills in the
//! process's standard handles when they are unset, and Windows gives a program in the windows
//! subsystem no usable ones at all — so the console's own output, `CONOUT$`, is opened and put in the
//! standard handle slots by hand. Rust's standard library asks Windows for those handles on every
//! write rather than caching them at startup, so a `println!` after this reaches the terminal.
//!
//! **A handle that is already there is left alone**, which is what keeps `unluminous --version > file`
//! honest. A shell that redirects passes the file down and the standard handle is real; a shell that
//! does not passes nothing and the slot is empty. Overwriting the first case would send the answer to
//! the screen while the file the person asked for stayed empty.
//!
//! The prompt comes back before the text does, because the shell does not wait for a program in the
//! windows subsystem. That is a cosmetic quirk of every graphical program that answers `--version`
//! this way, and it is not worth a second executable to avoid.
//!
//! Everywhere other than Windows this is nothing: the binary is an ordinary one, and its output has
//! always gone where the shell sent it.

/// Borrow the console of whatever started this process, so that printing reaches it.
///
/// Silent when there is no console to borrow — started from the desktop, or from a shortcut — because
/// the caller is about to print and exit either way, and there is nowhere to report the failure to.
#[cfg(windows)]
pub fn attach_to_the_calling_terminal() {
    use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE, STD_OUTPUT_HANDLE};

    // SAFETY: a plain call into kernel32 with the value it documents.
    if unsafe { AttachConsole(ATTACH_PARENT_PROCESS) } == 0 {
        // Nothing started this from a terminal — the desktop, or a shortcut. There is no console to
        // write to and nothing to do about it.
        return;
    }
    point_at_the_console(STD_OUTPUT_HANDLE);
    point_at_the_console(STD_ERROR_HANDLE);
}

/// Point one standard handle at the console, unless it already points somewhere.
///
/// The unless is the redirection case: see this module's comment.
#[cfg(windows)]
fn point_at_the_console(which: windows_sys::Win32::System::Console::STD_HANDLE) {
    use windows_sys::Win32::Foundation::{GENERIC_WRITE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_ATTRIBUTE_NORMAL, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows_sys::Win32::System::Console::{GetStdHandle, SetStdHandle};

    // SAFETY: kernel32 calls with documented values. The handle handed to `SetStdHandle` is one this
    // function just opened and deliberately never closes — the process is about to end, and closing
    // it would leave the standard handle naming something that is gone.
    unsafe {
        let existing = GetStdHandle(which);
        if !existing.is_null() && existing != INVALID_HANDLE_VALUE {
            return;
        }
        // `CONOUT$` is the console's own output, whichever console was just attached to.
        let name: Vec<u16> = "CONOUT$\0".encode_utf16().collect();
        let console = CreateFileW(
            name.as_ptr(),
            GENERIC_WRITE,
            FILE_SHARE_WRITE,
            std::ptr::null(),
            OPEN_EXISTING,
            FILE_ATTRIBUTE_NORMAL,
            std::ptr::null_mut(),
        );
        if console != INVALID_HANDLE_VALUE {
            SetStdHandle(which, console);
        }
    }
}

/// Nothing to do: output has always reached the shell that started this.
#[cfg(not(windows))]
pub fn attach_to_the_calling_terminal() {}
