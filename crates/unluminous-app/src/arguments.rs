//! Reading the command line: `unluminous [path] [switches]`.
//!
//! This lives in the library rather than in `main.rs` for the reason [`crate::resolve_target`] does:
//! so that what the command line means can be run by a test. It used to be a loop inside `main`, and
//! nothing could look at it without starting a window.
//!
//! **An argument that begins with a dash is a switch, and an unknown switch is refused.** The loop
//! this replaces ended `other => path = Some(PathBuf::from(other))`, so `unluminous --version` was a
//! *folder* called `--version`: the window opened on it, and — because a window writes its project
//! state beside the project — Unluminous **created the directory** and put `.unluminous/` inside it.
//! `task-1812` found one of those sitting in a repository. Guessing that a mistyped switch is a path
//! is the worst of the three available behaviours; the other two are opening nothing and saying why,
//! which is what happens now.
//!
//! `--version` and `--help` are answers rather than settings: they are printed and nothing opens.
//! `unluminous-cli --version` has always answered, and the window it drives had not, which is how the
//! folder above came to exist.

use std::path::PathBuf;

use crate::app::ViewMode;
use crate::build_info;
use crate::components::title_bar::MenuPlacement;

/// The one line that says what the program takes, quoted by `--help` and by the module comment.
pub const USAGE: &str = "Usage: unluminous [path] [--opacity N] [--view raw|side|preview] \
                         [--menu-bar native|in-window] [--terminal] [--control on|off] \
                         [--print-menus] [--version]";

/// The settings a window opens with, once the command line has been read.
#[derive(Debug, Clone, PartialEq)]
pub struct Arguments {
    /// The folder to show, or the file to open. `None` when nothing was named.
    pub path: Option<PathBuf>,
    pub opacity: Option<f32>,
    pub view: Option<ViewMode>,
    pub menu_bar: Option<MenuPlacement>,
    pub terminal: bool,
    /// `--print-menus`: print the menus and their shortcuts, and stop. It is here rather than an
    /// [`Start::Answer`] because building the menus needs a [`crate::app::actions::MenuState`],
    /// which is the binary's business rather than this module's.
    pub print_menus: bool,
    /// False when `--control off` was given, or `UNLUMINOUS_CONTROL=off` is in the environment.
    pub control: bool,
}

impl Default for Arguments {
    /// Every switch absent, and the command channel open — which is what `--control` defaults to and
    /// why this is written out rather than derived.
    fn default() -> Self {
        Self {
            path: None,
            opacity: None,
            view: None,
            menu_bar: None,
            terminal: false,
            print_menus: false,
            control: true,
        }
    }
}

/// What reading the command line came to.
#[derive(Debug, Clone, PartialEq)]
pub enum Start {
    /// Open a window with these settings.
    Window(Arguments),
    /// A question rather than a request for a window: print this and stop, successfully.
    Answer(String),
    /// The command line was not understood: print this and stop with a failing status.
    Refuse(String),
}

/// Read the command line.
///
/// `arguments` is everything after the program's own name, and `control_setting` is
/// `UNLUMINOUS_CONTROL` from the environment. Both are passed in rather than read here so that this
/// is a rule a test can run.
///
/// The environment is read first, so that a switch on the command line beats it, which is the order
/// `clig.dev` sets out for configuration: a flag, then the environment, then a file.
pub fn read(
    arguments: impl IntoIterator<Item = String>,
    control_setting: Option<&str>,
) -> Start {
    let mut settings = Arguments {
        control: !matches!(control_setting.unwrap_or_default().trim(), "off" | "no" | "0" | "false"),
        ..Arguments::default()
    };
    let mut rest = arguments.into_iter();
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--opacity" => {
                settings.opacity = rest.next().and_then(|value| value.parse::<f32>().ok());
            }
            "--view" => {
                settings.view = rest.next().and_then(|value| match value.as_str() {
                    "raw" => Some(ViewMode::Raw),
                    "side" | "side-by-side" => Some(ViewMode::SideBySide),
                    "preview" => Some(ViewMode::Preview),
                    _ => None,
                });
            }
            "--menu-bar" => {
                settings.menu_bar = rest.next().and_then(|value| match value.as_str() {
                    "native" | "screen" => Some(MenuPlacement::Native),
                    "in-window" | "window" => Some(MenuPlacement::InWindow),
                    _ => None,
                });
            }
            "--terminal" => settings.terminal = true,
            "--control" => {
                settings.control =
                    !matches!(rest.next().unwrap_or_default().trim(), "off" | "no" | "0" | "false");
            }
            "--print-menus" => settings.print_menus = true,
            "--version" | "-V" => return Start::Answer(version()),
            "--help" | "-h" => return Start::Answer(help()),
            // A switch nobody here knows. Said as a sentence, with the one command that lists the
            // switches there are, rather than a usage dump nobody reads to the end of.
            unknown if unknown.starts_with('-') => {
                return Start::Refuse(format!(
                    "unluminous: {unknown} is not one of Unluminous's switches. `unluminous --help` \
                     lists the ones it has. If you meant a file or folder whose name starts with a \
                     dash, put a folder in front of it: `unluminous ./{trimmed}`.",
                    trimmed = unknown.trim_start_matches('-')
                ));
            }
            other => settings.path = Some(PathBuf::from(other)),
        }
    }
    Start::Window(settings)
}

/// What `--version` says: the same two facts the About box shows, from the same two constants.
///
/// Shaped like `unluminous-cli --version`, which prints `unluminous-cli 0.37.0`, so that the two
/// answer the same question the same way.
pub fn version() -> String {
    format!("unluminous {}\nbuilt {}", build_info::VERSION, build_info::BUILD_DATE)
}

/// What `--help` says.
pub fn help() -> String {
    [
        USAGE,
        "  path            a folder to show, or a file to open",
        "  --opacity N     background opacity from 0.05 to 1.0",
        "  --view MODE     raw, side or preview",
        "  --menu-bar WHERE  native for the screen's own bar, in-window for the title bar",
        "  --terminal      open the terminal at the bottom",
        "  --control WHICH on to let unluminous-cli drive this window, off to close the channel. On by default.",
        "  --print-menus   print the menus and their shortcuts, and stop",
        "  --version       print the version and the build date, and stop",
    ]
    .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `read`, taking the arguments the way a person types them.
    fn read_line(line: &[&str]) -> Start {
        read(line.iter().map(|part| part.to_string()), None)
    }

    fn window(line: &[&str]) -> Arguments {
        match read_line(line) {
            Start::Window(settings) => settings,
            other => panic!("expected a window from {line:?}, got {other:?}"),
        }
    }

    #[test]
    fn nothing_at_all_opens_a_window_with_no_settings_and_the_channel_open() {
        assert_eq!(window(&[]), Arguments::default());
    }

    #[test]
    fn a_path_is_the_project() {
        assert_eq!(window(&["C:/jason/dev/unluminous"]).path.unwrap(), PathBuf::from("C:/jason/dev/unluminous"));
    }

    #[test]
    fn the_switches_are_read() {
        let settings = window(&[
            "--opacity", "0.5", "--view", "preview", "--menu-bar", "in-window", "--terminal",
            "--print-menus", "/a/project",
        ]);
        assert_eq!(settings.opacity, Some(0.5));
        assert_eq!(settings.view, Some(ViewMode::Preview));
        assert_eq!(settings.menu_bar, Some(MenuPlacement::InWindow));
        assert!(settings.terminal);
        assert!(settings.print_menus);
        assert_eq!(settings.path.unwrap(), PathBuf::from("/a/project"));
    }

    #[test]
    fn control_is_open_by_default_and_the_switch_beats_the_environment() {
        assert!(window(&[]).control);
        assert!(!window(&["--control", "off"]).control);
        let Start::Window(from_environment) = read(std::iter::empty(), Some("off")) else {
            panic!("a window");
        };
        assert!(!from_environment.control);
        let Start::Window(switch_wins) = read(["--control".to_string(), "on".to_string()], Some("off"))
        else {
            panic!("a window");
        };
        assert!(switch_wins.control);
    }

    /// The fault `task-1812` reported: `--version` opened a window on a folder of that name and
    /// created it. It is an answer now, and no window is asked for at all.
    #[test]
    fn version_is_answered_rather_than_opened_as_a_folder() {
        let Start::Answer(said) = read_line(&["--version"]) else {
            panic!("--version has to be an answer, not a window on a folder called --version");
        };
        assert!(said.starts_with("unluminous "), "it names the program: {said:?}");
        assert!(said.contains(build_info::VERSION), "it names the version: {said:?}");
        assert!(said.contains(build_info::BUILD_DATE), "it names the build date: {said:?}");
    }

    #[test]
    fn help_is_answered_and_lists_every_switch_that_is_read() {
        let Start::Answer(said) = read_line(&["--help"]) else { panic!("--help is an answer") };
        for switch in
            ["--opacity", "--view", "--menu-bar", "--terminal", "--control", "--print-menus", "--version"]
        {
            assert!(said.contains(switch), "--help does not mention {switch}:\n{said}");
        }
    }

    /// The other half of the same fault: any mistyped switch made a folder, not just `--version`.
    #[test]
    fn an_unknown_switch_is_refused_rather_than_taken_for_a_path() {
        for typed in ["--verison", "--wat", "-v", "--opacty"] {
            let Start::Refuse(said) = read_line(&[typed]) else {
                panic!("{typed} has to be refused, not opened as a folder called {typed}");
            };
            assert!(said.contains(typed), "the refusal quotes what was typed: {said:?}");
            assert!(said.contains("--help"), "the refusal says where the switches are: {said:?}");
        }
    }

    /// A refusal stops there. The point is that nothing is opened and nothing is created, so a
    /// path after the mistake must not quietly become the project either.
    #[test]
    fn a_refusal_wins_over_the_rest_of_the_line() {
        assert!(matches!(read_line(&["--wat", "/a/project"]), Start::Refuse(_)));
        assert!(matches!(read_line(&["/a/project", "--wat"]), Start::Refuse(_)));
    }
}
