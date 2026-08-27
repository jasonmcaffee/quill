//! Starts Quill.
//!
//! The window is created transparent so that the background opacity setting can let the desktop show
//! through. On macOS the window shadow is turned off as well, because the egui documentation records
//! that a translucent window with a shadow leaves ghosting artefacts behind it. Windows needs more
//! than a transparent window before anything shows through, and `services::windows_transparency` is
//! where that lives and why.
//!
//! Usage: `quill [path] [--opacity N] [--view raw|side|preview] [--menu-bar native|in-window] [--terminal] [--control on|off]`
//!
//! `--print-menus` prints the menus and their shortcuts and stops. The macOS menu bar cannot be looked at
//! from a test, so this is how what it was built from can be read.
//!
//! `path` is a folder to show in the explorer, or a file to open, in which case the explorer shows the
//! folder that file is in. With no path at all it is the current directory — or, when Quill was started
//! from the desktop and the current directory is only wherever the shortcut points, the project that was
//! open last time. `quill_app::starting_folder` is that rule and says why it is drawn so narrowly.
//! Several Quills can run at once, each on its own project, which is what `File -> New Window` and
//! `File -> Recent Projects` start.
//!
//! `--control off` closes the command channel `quill-cli` drives the window down. It is open by
//! default, because a command line that needs to be switched on first is a command line an agent
//! cannot rely on being there; `services::control` records what it is and why it is safe.
//!
//! The switches are there so a starting state can be chosen without clicking, which is what makes it
//! possible to capture the window in a particular state. `--opacity` and `--view` are the same settings the
//! Settings window and the toolbar change. `--menu-bar` chooses where the menus are drawn, which on macOS is
//! the bar along the top of the screen and everywhere else is the window's own title bar; naming it is how
//! the bar inside the window can be looked at on a Mac. `--terminal` opens the terminal at startup.

// Do not open a console window alongside the application on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use quill_app::app::ViewMode;
use quill_app::components::title_bar::MenuPlacement;
use quill_app::QuillApp;

struct Arguments {
    path: Option<PathBuf>,
    opacity: Option<f32>,
    view: Option<ViewMode>,
    menu_bar: Option<MenuPlacement>,
    terminal: bool,
    print_menus: bool,
    /// False when `--control off` was given, or `QUILL_CONTROL=off` is in the environment.
    control: bool,
}

fn parse_arguments() -> Arguments {
    let mut path = None;
    let mut opacity = None;
    let mut view = None;
    let mut menu_bar = None;
    let mut terminal = false;
    let mut print_menus = false;
    // The environment first, so that a switch on the command line beats it, which is the order
    // `clig.dev` sets out for configuration: a flag, then the environment, then a file.
    let mut control = !matches!(
        std::env::var("QUILL_CONTROL").unwrap_or_default().trim(),
        "off" | "no" | "0" | "false"
    );
    let mut rest = std::env::args().skip(1);
    while let Some(argument) = rest.next() {
        match argument.as_str() {
            "--opacity" => {
                opacity = rest.next().and_then(|value| value.parse::<f32>().ok());
            }
            "--view" => {
                view = rest.next().and_then(|value| match value.as_str() {
                    "raw" => Some(ViewMode::Raw),
                    "side" | "side-by-side" => Some(ViewMode::SideBySide),
                    "preview" => Some(ViewMode::Preview),
                    _ => None,
                });
            }
            "--menu-bar" => {
                menu_bar = rest.next().and_then(|value| match value.as_str() {
                    "native" | "screen" => Some(MenuPlacement::Native),
                    "in-window" | "window" => Some(MenuPlacement::InWindow),
                    _ => None,
                });
            }
            "--terminal" => terminal = true,
            "--control" => {
                control = !matches!(
                    rest.next().unwrap_or_default().trim(),
                    "off" | "no" | "0" | "false"
                );
            }
            "--print-menus" => print_menus = true,
            "--help" | "-h" => {
                println!(
                    "Usage: quill [path] [--opacity N] [--view raw|side|preview] [--menu-bar native|in-window] [--terminal] [--control on|off] [--print-menus]"
                );
                println!("  path            a folder to show, or a file to open");
                println!("  --opacity N     background opacity from 0.05 to 1.0");
                println!("  --view MODE     raw, side or preview");
                println!("  --menu-bar WHERE  native for the screen's own bar, in-window for the title bar");
                println!("  --terminal      open the terminal at the bottom");
                println!("  --control WHICH on to let quill-cli drive this window, off to close the channel. On by default.");
                println!("  --print-menus   print the menus and their shortcuts, and stop");
                std::process::exit(0);
            }
            other => path = Some(PathBuf::from(other)),
        }
    }
    Arguments { path, opacity, view, menu_bar, terminal, print_menus, control }
}

/// Print the menus, the way both menu bars are built from them.
///
/// The bar along the top of the screen on macOS cannot be read by a test, so this is how what went into it
/// can be checked without looking at the screen.
fn print_menus() {
    use quill_app::app::actions::{self, Entry, MenuState};
    let state = MenuState {
        can_undo: true,
        can_redo: true,
        has_selection: true,
        recent: vec![PathBuf::from("/a/recent/project")],
        terminal_tabs: 1,
        ..MenuState::default()
    };
    fn rows(entries: &[Entry], indent: usize) {
        for entry in entries {
            match entry {
                Entry::Separator => println!("{:indent$}  ---", "", indent = indent),
                Entry::Item { name, shortcut, enabled, checked, .. } => {
                    let keys = shortcut.map(|s| s.label()).unwrap_or_default();
                    let marks = match (enabled, checked) {
                        (false, _) => " (dimmed)",
                        (_, true) => " (on)",
                        _ => "",
                    };
                    println!("{:indent$}  {name:<24}{keys}{marks}", "", indent = indent);
                }
                Entry::Submenu { name, entries } => {
                    println!("{:indent$}  {name} >", "", indent = indent);
                    rows(entries, indent + 4);
                }
            }
        }
    }
    for menu in actions::menus(&state) {
        println!("{}", menu.name);
        rows(&menu.entries, 2);
    }
}

fn main() -> eframe::Result {
    let arguments = parse_arguments();

    if arguments.print_menus {
        print_menus();
        std::process::exit(0);
    }

    // Before anything else, so that a panic while the window is being built is written down too. A
    // graphical application on macOS has no standard error to print one to, and a panic that unwinds
    // out of the event loop exits rather than aborting, so the operating system files no crash report
    // either: without this a crash leaves nothing at all behind. `services::crash_log` says more.
    //
    // The backtrace is asked for rather than left to `RUST_BACKTRACE`, because the person whose Quill
    // just disappeared did not set an environment variable before it happened.
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        // SAFETY: nothing else is running yet; this is the first statement of the program.
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    }
    quill_app::services::crash_log::install(quill_app::services::store::folder_for_this_person());

    // Proving the crash log works has to be possible on the machine it matters on, from the installed
    // application, where there is no terminal and no test harness. `QUILL_PANIC_TEST=1 quill` panics
    // here on purpose, and what it writes to crash.log is what a real crash writes.
    if std::env::var_os("QUILL_PANIC_TEST").is_some() {
        panic!("QUILL_PANIC_TEST was set, so this is a panic on purpose");
    }

    // A file argument opens that file and shows the folder it sits in. A folder argument just shows the
    // folder. With no argument at all, the explorer shows the current directory — except when Quill was
    // started from the desktop rather than from a terminal, where the current directory is the folder
    // holding `quill.exe` and the project that was open last time is what was meant. Both rules live in
    // the library so that they are tested.
    //
    // The recent projects are read here rather than waiting for `load_settings`, because which folder to
    // show has to be decided before the window is built. It is still the released binary reading the
    // person's own files, which is the rule `Store` keeps.
    let current_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let program = std::env::current_exe().ok();
    let store = quill_app::services::store::Store::open();
    let recent = store.recent_projects();

    // The windows that were open last time, which `task-1693` asks to have back. Only on a launch
    // from the desktop: `quill .` typed in a folder has to open that folder and nothing else, which
    // is the same rule `starting_folder` keeps about reopening the last project.
    //
    // The list is oldest first, so the **last** entry is the project this process opens itself and
    // every other entry gets a process of its own — a Quill window is a process, which
    // `services::launcher` records as a deliberate decision.
    let restoring = arguments.path.is_none()
        && quill_app::started_from_the_desktop(&current_directory, program.as_deref());
    let mut session = match restoring {
        true => store.open_windows(),
        false => Vec::new(),
    };
    // A project that already has a window is left alone. Two Quills on one folder would be two
    // processes writing one `.quill` folder, and whichever wrote last would win — which is the same
    // reason `OpenFiles::open` shows a file that is already open rather than opening it twice. The
    // instance files are how a running window says which project it is on; `quill-cli instances`
    // reads the same list.
    // `running` rather than `listed`, because a window that was killed rather than closed leaves its
    // instance file behind — and a project skipped on the strength of a dead window is a project
    // that never comes back.
    let already_open: Vec<PathBuf> =
        quill_cli::client::running().into_iter().map(|instance| instance.folder).collect();
    session.retain(|folder| !already_open.iter().any(|open| open == folder));
    let mine = session.pop();

    let fallback = quill_app::starting_folder(
        &current_directory,
        program.as_deref(),
        mine.as_deref().or_else(|| recent.first().map(PathBuf::as_path)),
    );
    let (folder, file) = quill_app::resolve_target(arguments.path.as_deref(), &fallback);

    // The other windows, each with its project named on the command line so that none of them tries
    // to restore the session in its turn. Each reads its own project's geometry, so they come back
    // where they were left.
    for other in &session {
        if *other != folder {
            quill_app::services::launcher::open_window(other);
        }
    }

    // Where this project's window was left, which `task-1693` asks for and which has to be known
    // before the window is built. It is read from the project's own `.quill` folder rather than from
    // the person's settings, because Quill's windows are one per project: a geometry kept per person
    // would open the second window exactly on top of the first.
    let place = quill_app::services::project_state::load(&folder).window;

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Quill")
        .with_inner_size([1100.0, 720.0])
        .with_min_inner_size([640.0, 400.0]);
    if let Some(place) = place.filter(|place| place.is_sensible()) {
        viewport = viewport
            .with_position([place.x, place.y])
            .with_inner_size([place.width, place.height])
            .with_maximized(place.maximised);
    }

    let options = eframe::NativeOptions {
        viewport: viewport
            .with_transparent(true)
            // Quill draws its own title bar, because rounded corners and a translucent background need the
            // operating system's own window frame turned off.
            .with_decorations(false)
            .with_has_shadow(false),
        // Run the event loop the way winit says to run it. eframe's default is `true`, which drives the
        // loop through winit's `run_app_on_demand` so that `run_native` can return to its caller;
        // winit's own documentation for it says "You are strongly encouraged to use
        // `EventLoop::run_app()` for portability, unless you specifically need the ability to re-run a
        // single event loop more than once", and eframe's own comment on the setting says the `false`
        // option "is only there so we can revert if we find any bugs". Quill runs the loop once and
        // then the process ends, so it needs none of what the default buys, and a window was found
        // asleep on macOS inside `run_app_on_demand` with wakes no longer reaching it. This takes that
        // difference out of the picture. `app::HEARTBEAT` is what makes the window recover by itself if
        // a wake is ever lost again.
        run_and_return: false,
        ..Default::default()
    };

    // The desktop shows through the window on macOS with nothing more than `with_transparent`. Windows
    // also needs a swapchain that can carry alpha, which is a wgpu setting rather than a window one.
    #[cfg(windows)]
    let options = quill_app::services::windows_transparency::with_direct_composition(options);

    eframe::run_native(
        "Quill",
        options,
        Box::new(move |cc| {
            let mut app = QuillApp::new(folder);
            app.prepare(&cc.egui_ctx);
            // The settings, the pane sizes and the recent projects come from disk here rather than in
            // `QuillApp::new`, so that a test never reads or writes the settings of the person running it.
            app.load_settings();
            // What was left open in this project last time. After the settings, because it opens files
            // and they have to be set in the font the settings name; before the file argument, so a file
            // named on the command line is the tab that ends up showing.
            app.restore_project();
            if let Some(file) = file {
                app.open_path(&file);
            }
            if let Some(opacity) = arguments.opacity {
                app.settings.opacity = opacity.clamp(0.05, 1.0);
            }
            if let Some(view) = arguments.view {
                app.set_view_mode(view);
            }
            app.menu_placement = arguments.menu_bar.unwrap_or_else(MenuPlacement::for_this_platform);
            if app.menu_placement == MenuPlacement::Native {
                // The bar along the top of the screen. Built here rather than in `prepare`, because it needs
                // a real application to attach itself to and the screenshot tests have none.
                app.install_native_menu();
            }
            if arguments.terminal {
                // Through the one function that shows a tile, as everything else does. Nothing is
                // running yet at this point, so it changes nothing here — but a path that set the
                // flag by hand would be the next one to forget the other tile.
                app.show_the_terminal_tile(true);
            }
            // Last, so that a command arriving at once finds the window as the switches left it.
            if arguments.control {
                app.open_control_channel(&cc.egui_ctx);
            }
            Ok(Box::new(app))
        }),
    )
}
