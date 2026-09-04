//! Two things that have to happen while `unluminate.exe` is being built: the icon and version block go
//! inside it, and the build date is stamped into it.
//!
//! Windows reads a program's icon, the name on its taskbar button, its description in Task Manager
//! and its version in Add or Remove Programs out of a resource block compiled into the executable.
//! An installer cannot supply one — a shortcut can point at an icon file, but the running window and
//! the taskbar button read the exe — so it is put there here.
//!
//! The icon is `installer/icon/unluminate.ico`, which is drawn by `installer/icon` and committed, so a
//! checkout builds without running a drawing program first.
//!
//! A machine with no Windows SDK has no `rc.exe` and cannot compile a resource. That is a warning
//! rather than an error: the build still produces a working `unluminate.exe`, just an unlabelled one, and
//! only the machine that builds the installer needs the labelled one.
//!
//! ## The build date
//!
//! `task-1667` asks the About box to say when the binary was built, so that two builds of one
//! version can be told apart. That is a fact only the build knows, so it is worked out here and
//! handed to the compiler as `UNLUMINATE_BUILD_DATE`, which `build_info::BUILD_DATE` reads.
//!
//! **When this script reruns is the whole design.** The value it emits is part of the crate's
//! fingerprint, so a script that restamped on every invocation would recompile `unluminate-app` and
//! relink every screenshot test each time the clock moved, for no other reason. The
//! `rerun-if-changed` lines below watch the workspace's sources, so the stamp moves exactly when
//! something was going to be rebuilt anyway — which makes it mean "the last build that had anything
//! to build", which is what a person reading it wants it to mean.

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // Watch the sources rather than nothing at all: a build script that names any `rerun-if` at all
    // is trusted to name all of them, so without these the stamp would freeze at the first build.
    println!("cargo:rerun-if-changed=../../crates");
    println!("cargo:rerun-if-changed=../../unluminate-cli");
    println!("cargo:rerun-if-env-changed=UNLUMINATE_BUILD_DATE");
    println!("cargo:rustc-env=UNLUMINATE_BUILD_DATE={}", build_date());

    #[cfg(windows)]
    embed_windows_resources();
}

/// When this build is happening, as `2026-08-25 10:45pm`, in the local time of this machine.
///
/// An environment variable wins, so that a reproducible build can pin it.
fn build_date() -> String {
    if let Ok(given) = std::env::var("UNLUMINATE_BUILD_DATE") {
        if !given.trim().is_empty() {
            return given.trim().to_owned();
        }
    }
    local_time().unwrap_or_else(utc_time)
}

/// The local date and time, asked of the platform.
///
/// Unluminate has no dates library and does not want one: `unluminate_git::blame` computes a civil date from a
/// Unix time with arithmetic rather than a crate. What arithmetic cannot give is this machine's
/// offset from UTC, so the platform is asked for the whole formatted answer — the same choice
/// `unluminate-git` makes when it runs `git` rather than reimplementing it. `None` when the command
/// cannot be run or answers with something that is not a date, which is what makes the UTC fallback
/// reachable.
fn local_time() -> Option<String> {
    let output = if cfg!(windows) {
        std::process::Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", "Get-Date -Format 'yyyy-MM-dd h:mmtt'"])
            .output()
            .ok()?
    } else {
        // `%I` rather than `%-I`: the unpadded form is a GNU extension that BSD `date` on macOS
        // does not have, and `trim_the_hours_leading_zero` takes the padding off afterwards.
        std::process::Command::new("date").arg("+%Y-%m-%d %I:%M%p").output().ok()?
    };
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    // `10:45AM` is what both of them give back, and `10:45am` is what the About box shows.
    let stamped = trim_the_hours_leading_zero(&lower_case_meridiem(text));
    looks_like_a_date(&stamped).then_some(stamped)
}

/// `2026-08-25 10:45AM` becomes `2026-08-25 10:45am`, and nothing else is changed.
fn lower_case_meridiem(text: &str) -> String {
    let mut out = text.to_owned();
    for meridiem in ["AM", "PM"] {
        if out.ends_with(meridiem) {
            out.truncate(out.len() - 2);
            out.push_str(&meridiem.to_lowercase());
        }
    }
    out
}

/// `2026-08-25 09:45am` becomes `2026-08-25 9:45am`, which is how a clock is read aloud.
///
/// Windows is asked for the unpadded hour and gives one; BSD `date` has no unpadded form, so the
/// padding comes off here rather than in two different format strings.
fn trim_the_hours_leading_zero(text: &str) -> String {
    match text.split_once(' ') {
        Some((date, time)) if time.starts_with('0') => format!("{date} {}", &time[1..]),
        _ => text.to_owned(),
    }
}

/// True while a string starts `YYYY-MM-DD`, which is enough to know the platform answered with a
/// date rather than with an error message or a localised format nobody asked for.
fn looks_like_a_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    if bytes.len() < 10 {
        return false;
    }
    bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
}

/// The fallback, when the platform could not be asked: UTC, and labelled as UTC.
///
/// A stamp that says which zone it is in is honest; one silently seven hours out is not. The civil
/// date is Howard Hinnant's `civil_from_days`, which is the same arithmetic `unluminate_git::blame` uses
/// and is repeated here because a build script cannot depend on a crate it is building.
fn utc_time() -> String {
    let seconds = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0);
    let days = seconds.div_euclid(86_400);
    let rest = seconds.rem_euclid(86_400);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02} {:02}:{:02} UTC", rest / 3600, (rest % 3600) / 60)
}

/// A count of days since the epoch as a year, a month and a day, with no table and no leap-year
/// special cases.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    // Shift the epoch to the 1st of March 0000, so the leap day is the last day of the year.
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 { shifted_month + 3 } else { shifted_month - 9 };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

#[cfg(windows)]
fn embed_windows_resources() {
    // From `crates/unluminate-app` up to the repository, then down to the drawn icon.
    const ICON: &str = "../../installer/icon/unluminate.ico";
    println!("cargo:rerun-if-changed={ICON}");

    if !std::path::Path::new(ICON).exists() {
        println!("cargo:warning=unluminate.exe has no icon: {ICON} is missing. Run `cargo run --release --manifest-path installer/icon/Cargo.toml`.");
        return;
    }

    let mut resources = winresource::WindowsResource::new();
    resources
        .set_icon(ICON)
        .set("ProductName", "Unluminate")
        .set("FileDescription", "Unluminate")
        .set("CompanyName", "Jason McAffee")
        .set("LegalCopyright", "Licensed under MIT or Apache-2.0")
        .set("InternalName", "unluminate.exe")
        .set("OriginalFilename", "unluminate.exe");

    if let Err(problem) = resources.compile() {
        println!("cargo:warning=unluminate.exe has no icon or version block: {problem}");
    }
}
