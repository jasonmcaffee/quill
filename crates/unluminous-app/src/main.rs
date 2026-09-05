//! Starts Unluminous.
//!
//! The window is created transparent so that the background opacity setting can let the desktop show
//! through. On macOS the window shadow is turned off as well, because the egui documentation records
//! that a translucent window with a shadow leaves ghosting artefacts behind it. Windows needs more
//! than a transparent window before anything shows through, and `services::windows_transparency` is
//! where that lives and why.
//!
//! Usage: `unluminous [path] [--opacity N] [--view raw|side|preview] [--menu-bar native|in-window] [--terminal] [--control on|off]`
//!
//! **What the command line means is `unluminous_app::arguments`, not this file.** It is read there so
//! that a test can run it, which is the same reason `resolve_target` and `starting_folder` live in the
//! library; `main` is left with what a test cannot do anyway — printing, exiting, and building a
//! window. `task-1812` is why: the loop that used to be here treated an argument it did not recognise
//! as a path, so `unluminous --version` opened a window on a folder called `--version` and created it.
//!
//! `--version` prints the version and the build date and stops. `--print-menus` prints the menus and
//! their shortcuts and stops; the macOS menu bar cannot be looked at from a test, so this is how what
//! it was built from can be read. Both go through `services::console` first, because a program in the
//! windows subsystem has no console to print to until it borrows one.
//!
//! `path` is a folder to show in the explorer, or a file to open, in which case the explorer shows the
//! folder that file is in. With no path at all it is the current directory — or, when Unluminous was started
//! from the desktop and the current directory is only wherever the shortcut points, the project that was
//! open last time. `unluminous_app::starting_folder` is that rule and says why it is drawn so narrowly.
//! Several Unluminous windows can run at once, each on its own project, which is what `File -> New Window`
//! and `File -> Recent Projects` start.
//!
//! `--control off` closes the command channel `unluminous-cli` drives the window down. It is open by
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

use unluminous_app::arguments::{self, Arguments, Start};
use unluminous_app::components::title_bar::MenuPlacement;
use unluminous_app::services::console;
use unluminous_app::UnluminousApp;

/// Read the command line, and answer it here if it was a question rather than a request for a window.
///
/// Printing and exiting is all this adds to `arguments::read`: the reading itself is a rule with
/// tests over it, and this is the part that has to happen in a real process.
fn settings_or_stop() -> Arguments {
    let line = std::env::args().skip(1);
    let control = std::env::var("UNLUMINOUS_CONTROL").ok();
    match arguments::read(line, control.as_deref()) {
        Start::Window(settings) => settings,
        Start::Answer(said) => {
            console::attach_to_the_calling_terminal();
            println!("{said}");
            std::process::exit(0);
        }
        // Two, rather than one, so that a script can tell a command line it got wrong from one that
        // worked. `unluminous-cli` uses its own exit codes for the same reason.
        Start::Refuse(why) => {
            console::attach_to_the_calling_terminal();
            eprintln!("{why}");
            std::process::exit(2);
        }
    }
}

/// Print the menus, the way both menu bars are built from them.
///
/// The bar along the top of the screen on macOS cannot be read by a test, so this is how what went into it
/// can be checked without looking at the screen.
fn print_menus() {
    use unluminous_app::app::actions::{self, Entry, MenuState};
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
    let arguments = settings_or_stop();
    unluminous_app::services::frame_trace::mark("arguments");

    if arguments.print_menus {
        console::attach_to_the_calling_terminal();
        print_menus();
        std::process::exit(0);
    }

    // Before anything else, so that a panic while the window is being built is written down too. A
    // graphical application on macOS has no standard error to print one to, and a panic that unwinds
    // out of the event loop exits rather than aborting, so the operating system files no crash report
    // either: without this a crash leaves nothing at all behind. `services::crash_log` says more.
    //
    // The backtrace is asked for rather than left to `RUST_BACKTRACE`, because the person whose Unluminous
    // just disappeared did not set an environment variable before it happened.
    if std::env::var_os("RUST_BACKTRACE").is_none() {
        // SAFETY: nothing else is running yet; this is the first statement of the program.
        unsafe { std::env::set_var("RUST_BACKTRACE", "1") };
    }
    unluminous_app::services::crash_log::install(unluminous_app::services::store::folder_for_this_person());
    unluminous_app::services::frame_trace::mark("crash-log");

    // Started here, and by nothing else, because it runs the person's shell profile: an Unluminous started
    // from the Finder or the Dock has `PATH=/usr/bin:/bin:/usr/sbin:/sbin` and cannot find a program
    // installed under the home folder, which is where `claude` and `codex` install themselves. It runs
    // on a thread and takes about a second and a half, so it is asked for as early as there is
    // anything to ask, long before a person can press a button that starts one.
    // `services::login_shell` says what is read and why the profile rather than a list of folders.
    unluminous_app::services::login_shell::start_reading();
    unluminous_app::services::frame_trace::mark("login-shell");

    // Proving the crash log works has to be possible on the machine it matters on, from the installed
    // application, where there is no terminal and no test harness. `UNLUMINOUS_PANIC_TEST=1 unluminous` panics
    // here on purpose, and what it writes to crash.log is what a real crash writes.
    if std::env::var_os("UNLUMINOUS_PANIC_TEST").is_some() {
        panic!("UNLUMINOUS_PANIC_TEST was set, so this is a panic on purpose");
    }

    // A file argument opens that file and shows the folder it sits in. A folder argument just shows the
    // folder. With no argument at all, the explorer shows the current directory — except when Unluminous was
    // started from the desktop rather than from a terminal, where the current directory is the folder
    // holding `unluminous.exe` and the project that was open last time is what was meant. Both rules live in
    // the library so that they are tested.
    //
    // The recent projects are read here rather than waiting for `load_settings`, because which folder to
    // show has to be decided before the window is built. It is still the released binary reading the
    // person's own files, which is the rule `Store` keeps.
    let current_directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let program = std::env::current_exe().ok();
    let store = unluminous_app::services::store::Store::open();
    let recent = store.recent_projects();
    unluminous_app::services::frame_trace::mark("recent-projects");

    // The windows that were open last time, which `task-1693` asks to have back. Only on a launch
    // from the desktop: `unluminous .` typed in a folder has to open that folder and nothing else, which
    // is the same rule `starting_folder` keeps about reopening the last project.
    //
    // The list is oldest first, so the **last** entry is the project this process opens itself and
    // every other entry gets a process of its own — an Unluminous window is a process, which
    // `services::launcher` records as a deliberate decision.
    let restoring = arguments.path.is_none()
        && unluminous_app::started_from_the_desktop(&current_directory, program.as_deref());
    let mut session = match restoring {
        true => store.open_windows(),
        false => Vec::new(),
    };
    // A project that already has a window is left alone. Two Unluminous windows on one folder would be two
    // processes writing one `.unluminous` folder, and whichever wrote last would win — which is the same
    // reason `OpenFiles::open` shows a file that is already open rather than opening it twice. The
    // instance files are how a running window says which project it is on; `unluminous-cli instances`
    // reads the same list.
    // `running` rather than `listed`, because a window that was killed rather than closed leaves its
    // instance file behind — and a project skipped on the strength of a dead window is a project
    // that never comes back.
    //
    // Asked **only when there is a session to filter**, which is the desktop launch. Every other
    // start — `unluminous .` in a terminal, `unluminous-cli launch`, `File -> New Window`, a file
    // opened from the shell — names its folder, so `session` is empty and there is nothing for this
    // answer to say anything about. It used to be asked on every start regardless, and `task-1805`
    // measured that at **414 ms of a 1234 ms startup**: a third of the time to a usable window spent
    // working out something that was then thrown away.
    if !session.is_empty() {
        let already_open: Vec<PathBuf> =
            unluminous_cli::client::running().into_iter().map(|instance| instance.folder).collect();
        session.retain(|folder| !already_open.iter().any(|open| open == folder));
    }
    unluminous_app::services::frame_trace::mark("running-instances");
    let mine = session.pop();

    let fallback = unluminous_app::starting_folder(
        &current_directory,
        program.as_deref(),
        mine.as_deref().or_else(|| recent.first().map(PathBuf::as_path)),
    );
    let (folder, file) = unluminous_app::resolve_target(arguments.path.as_deref(), &fallback);

    // The other windows, each with its project named on the command line so that none of them tries
    // to restore the session in its turn. Each reads its own project's geometry, so they come back
    // where they were left.
    for other in &session {
        if *other != folder {
            unluminous_app::services::launcher::open_window(other);
        }
    }

    // Where this project's window was left, which `task-1693` asks for and which has to be known
    // before the window is built. It is read from the project's own `.unluminous` folder rather than from
    // the person's settings, because Unluminous's windows are one per project: a geometry kept per person
    // would open the second window exactly on top of the first.
    let place = unluminous_app::services::project_state::load(&folder).window;
    unluminous_app::services::frame_trace::mark("project-state");

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Unluminous")
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
            // Unluminous draws its own title bar, because rounded corners and a translucent background need the
            // operating system's own window frame turned off.
            .with_decorations(false)
            .with_has_shadow(false),
        // Run the event loop the way winit says to run it. eframe's default is `true`, which drives the
        // loop through winit's `run_app_on_demand` so that `run_native` can return to its caller;
        // winit's own documentation for it says "You are strongly encouraged to use
        // `EventLoop::run_app()` for portability, unless you specifically need the ability to re-run a
        // single event loop more than once", and eframe's own comment on the setting says the `false`
        // option "is only there so we can revert if we find any bugs". Unluminous runs the loop once and
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
    let options = unluminous_app::services::windows_transparency::with_direct_composition(options);

    eframe::run_native(
        "Unluminous",
        options,
        Box::new(move |cc| {
            unluminous_app::services::frame_trace::mark("eframe-window");
            let mut app = UnluminousApp::new(folder);
            unluminous_app::services::frame_trace::mark("app-new");
            app.prepare(&cc.egui_ctx);
            unluminous_app::services::frame_trace::mark("prepare");
            // The settings, the pane sizes and the recent projects come from disk here rather than in
            // `UnluminousApp::new`, so that a test never reads or writes the settings of the person running it.
            app.load_settings();
            unluminous_app::services::frame_trace::mark("settings");
            // What was left open in this project last time. After the settings, because it opens files
            // and they have to be set in the font the settings name; before the file argument, so a file
            // named on the command line is the tab that ends up showing.
            app.restore_project();
            unluminous_app::services::frame_trace::mark("restore-project");
            if let Some(file) = file {
                // A file named on the command line that will not open leaves its reason in the
                // status bar of the window that has just come up, which is where a person is
                // looking. Unluminous still starts.
                let _ = app.open_path(&file);
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
            unluminous_app::services::frame_trace::mark("ready");
            Ok(Box::new(app))
        }),
    )
}
