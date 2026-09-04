//! Making sure a session's program goes when the session does.
//!
//! Closing a pseudoterminal is a hangup on Unix: the kernel sends `SIGHUP` to the foreground process
//! group and a shell sitting at a prompt ends. Windows has no such promise. `ClosePseudoConsole`
//! closes the pipes, and a program that is **reading** them notices; a program that is in the middle
//! of something is blocked on something else entirely and notices nothing, and its console host is
//! still there, so when it does come back to the prompt its read does not even end — it waits
//! forever on a console nobody will ever type into again.
//!
//! `task-1769` measured what that costs. 119 `pwsh.exe`, each with a `conhost.exe` beside it, each
//! about 35 MB of commit, none of them with a living parent, accruing at about sixty a day. On this
//! machine free commit is what decides whether a streaming render runs at full speed or at a
//! hundredth of it, so a leak of shells is not untidiness, it is a render that takes forty-five
//! hours instead of twenty-five minutes.
//!
//! So Windows does not get to decide. Every program a [`crate::Session`] starts is put in a **job
//! object** of its own, created with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, and the session holds the
//! only handle to it. That closes every way out at once, which is the point — the paths that leaked
//! were never the tidy one:
//!
//! * a tab is closed, so the session is dropped and the handle with it;
//! * the window is closed with tabs still open, and whatever the exit path does not walk is closed
//!   by the operating system when the process ends;
//! * **Unluminate crashes, or is killed** — the same, because a handle a dead process held is a handle
//!   that is closed, and this is the case that produced most of the 119;
//! * and the program had started programs of its own, which are in the job too and go with it. A
//!   shell that leaks a `node` is the same leak one level down.
//!
//! Everywhere but Windows this is nothing at all, because there the hangup already works.

/// The handle that keeps a session's program alive no longer than the session.
///
/// Dropping it ends the program and everything the program started. Constructing one for a process
/// that cannot be adopted is not an error: the terminal still works, it is only the guarantee that
/// is missing, so this degrades to the old behaviour rather than refusing to open a terminal.
pub struct Reaper {
    #[cfg(windows)]
    job: Option<windows::Job>,
    /// The child itself, held so that a job that could not be made is not the end of it.
    ///
    /// A job is the only thing that survives Unluminate being killed, so it is the one that matters; this
    /// is what is left if `AssignProcessToJobObject` refuses, which it can when the process is
    /// already inside a job that does not allow nesting. It covers the tab and window cases, which
    /// is most of them, and it costs one handle.
    #[cfg(windows)]
    child: Option<windows::Child>,
}

impl Reaper {
    /// Adopt the process behind `child`, which is a raw Windows process handle and is ignored
    /// everywhere else.
    ///
    /// The handle is the one `alacritty_terminal` already holds for the child it started
    /// (`Pty::child_watcher`), borrowed rather than owned: this never closes it, and the watcher goes
    /// on using it to report the exit.
    #[cfg_attr(not(windows), allow(unused_variables))]
    pub fn adopt(child: *mut std::ffi::c_void) -> Self {
        #[cfg(windows)]
        {
            Self { job: windows::Job::holding(child), child: windows::Child::duplicating(child) }
        }
        #[cfg(not(windows))]
        {
            Self {}
        }
    }

    /// A reaper that holds nothing, for a session with no program behind it.
    pub fn detached() -> Self {
        #[cfg(windows)]
        {
            Self { job: None, child: None }
        }
        #[cfg(not(windows))]
        {
            Self {}
        }
    }

    /// End the program now, rather than waiting for this to be dropped.
    ///
    /// This is what [`crate::Session::kill`] calls, and it is deliberately separate from dropping:
    /// a run tile goes on showing the output of a program it stopped, so the session outlives the
    /// program it was running.
    pub fn kill(&mut self) {
        #[cfg(windows)]
        {
            // The job first, because it takes the program's own children with it. The handle is only
            // reached for when there was no job to be had.
            match self.job.take() {
                Some(job) => job.terminate(),
                None => {
                    if let Some(child) = &self.child {
                        child.terminate();
                    }
                },
            }
            self.child = None;
        }
    }

    /// Whether this reaper is actually holding a program, which is what the tests ask.
    pub fn is_holding(&self) -> bool {
        #[cfg(windows)]
        {
            self.job.is_some() || self.child.is_some()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }

    /// Whether the guarantee that survives Unluminate itself being killed is in place.
    pub fn is_held_by_job(&self) -> bool {
        #[cfg(windows)]
        {
            self.job.is_some()
        }
        #[cfg(not(windows))]
        {
            false
        }
    }
}

impl Drop for Reaper {
    /// The job handle closing is what ends the program; the duplicated handle is only used when
    /// there is no job, and then it has to be told.
    fn drop(&mut self) {
        #[cfg(windows)]
        if self.job.is_none() {
            if let Some(child) = &self.child {
                child.terminate();
            }
        }
    }
}

#[cfg(windows)]
mod windows {
    use std::ffi::c_void;

    use windows_sys::Win32::Foundation::{
        CloseHandle, DuplicateHandle, DUPLICATE_SAME_ACCESS, FALSE, HANDLE,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    /// One job object holding one program and its descendants.
    pub struct Job(HANDLE);

    // The handle is owned by this value and only ever used through it: created here, closed in
    // `Drop`, and never handed to another thread while a second copy exists.
    unsafe impl Send for Job {}
    unsafe impl Sync for Job {}

    impl Job {
        /// A job holding `child`, or `None` when any step of it fails.
        ///
        /// Every failure is the same answer, because there is only one thing to do about any of
        /// them: a terminal that opens without the guarantee is better than a terminal that does not
        /// open. The most likely one by far is a process that has already exited between being
        /// started and being adopted, which is a program that needs no reaping.
        pub fn holding(child: *mut c_void) -> Option<Self> {
            if child.is_null() {
                return None;
            }
            // SAFETY: an unnamed job with default security, then the one limit that makes closing
            // the handle end what is inside it, then the child. Each call is checked before the
            // next is made, and the handle is closed by `Drop` on every path out of here.
            unsafe {
                let handle = CreateJobObjectW(std::ptr::null(), std::ptr::null());
                if handle.is_null() {
                    return None;
                }
                let job = Self(handle);
                let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
                limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let set = SetInformationJobObject(
                    job.0,
                    JobObjectExtendedLimitInformation,
                    std::ptr::addr_of!(limits) as *const c_void,
                    std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
                );
                if set == 0 {
                    return None;
                }
                if AssignProcessToJobObject(job.0, child as HANDLE) == 0 {
                    return None;
                }
                Some(job)
            }
        }

        /// End everything in the job now. The handle is closed by `Drop` straight after.
        pub fn terminate(self) {
            // SAFETY: `self.0` is a job handle this value owns and has not closed.
            unsafe { TerminateJobObject(self.0, 1) };
        }
    }

    impl Drop for Job {
        /// Closing the handle is what ends the program, because of the limit set when it was made.
        fn drop(&mut self) {
            // SAFETY: closed exactly once, here, for a handle this value owns.
            unsafe { CloseHandle(self.0) };
        }
    }

    /// A handle to the program itself, closed when this is dropped.
    pub struct Child(HANDLE);

    // Same argument as `Job`: one owner, one close.
    unsafe impl Send for Child {}
    unsafe impl Sync for Child {}

    impl Child {
        /// Our own copy of `child`, so that ending it never depends on a handle somebody else owns
        /// and may already have closed.
        pub fn duplicating(child: *mut c_void) -> Option<Self> {
            if child.is_null() {
                return None;
            }
            // SAFETY: duplicating a handle this process can already use, into this process.
            unsafe {
                let mut copy: HANDLE = std::ptr::null_mut();
                let me = GetCurrentProcess();
                let ok = DuplicateHandle(me, child as HANDLE, me, &mut copy, 0, FALSE, DUPLICATE_SAME_ACCESS);
                if ok == 0 || copy.is_null() {
                    return None;
                }
                Some(Self(copy))
            }
        }

        /// End the program. A program that has already ended fails this, which is the answer wanted.
        pub fn terminate(&self) {
            // SAFETY: `self.0` is a process handle this value owns and has not closed.
            unsafe { TerminateProcess(self.0, 1) };
        }
    }

    impl Drop for Child {
        fn drop(&mut self) {
            // SAFETY: closed exactly once, here, for a handle this value owns.
            unsafe { CloseHandle(self.0) };
        }
    }
}
