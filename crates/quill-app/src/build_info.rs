//! What this build of Quill is: its version, and when it was built.
//!
//! `installer/README.md` states the rule that the version lives in `Cargo.toml` and nowhere else.
//! This is the companion rule: it is *read* in one place too, so the About box, the status bar,
//! `quill-cli status` and anything written later cannot come to different answers about what is
//! running.
//!
//! [`BUILD_DATE`] is stamped by `build.rs` while the binary is compiled — see the comment at the top
//! of that file for when it moves and why it does not move more often than that.

/// The version in `Cargo.toml`, which is the one place it is written down.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// When this binary was built, in the local time of the machine that built it: `2026-08-25 10:45pm`.
///
/// A build made where the platform could not be asked for the local time says `UTC` after it, rather
/// than quietly being some hours out.
pub const BUILD_DATE: &str = env!("QUILL_BUILD_DATE");

/// Who wrote it. On the About box, and in `quill.exe`'s version block, where `build.rs` puts it.
pub const DEVELOPER: &str = "Jason McAffee";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_is_the_one_in_cargo_toml() {
        assert!(VERSION.split('.').count() >= 3, "a version has three parts: {VERSION}");
        assert!(VERSION.starts_with(|first: char| first.is_ascii_digit()));
    }

    #[test]
    fn the_build_date_was_stamped_and_starts_with_a_date() {
        // `build.rs` refuses to stamp anything that is not `YYYY-MM-DD` followed by a time, so this
        // failing means the stamp was not emitted at all rather than that it came out oddly.
        let bytes = BUILD_DATE.as_bytes();
        assert!(bytes.len() >= 10, "the build date is missing: {BUILD_DATE:?}");
        assert!(bytes[..4].iter().all(u8::is_ascii_digit), "no year in {BUILD_DATE:?}");
        assert_eq!(bytes[4], b'-', "no year-month separator in {BUILD_DATE:?}");
        assert_eq!(bytes[7], b'-', "no month-day separator in {BUILD_DATE:?}");
        assert!(BUILD_DATE.contains(':'), "no time of day in {BUILD_DATE:?}");
    }
}
