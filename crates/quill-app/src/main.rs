//! Starts Quill.
//!
//! The window is created transparent so that the background opacity setting can let the desktop show
//! through. On macOS the window shadow is turned off as well, because the egui documentation records
//! that a translucent window with a shadow leaves ghosting artefacts behind it. Windows needs more
//! than a transparent window before anything shows through, and `services::windows_transparency` is
//! where that lives and why.
//!
//! Usage: `quill [path] [--opacity N] [--view raw|side|preview] [--menu-bar native|in-window] [--terminal]`
//!
//! `--print-menus` prints the menus and their shortcuts and stops. The macOS menu bar cannot be looked at
//! from a test, so this is how what it was built from can be read.
//!
//! `path` is a folder to show in the explorer, or a file to open, in which case the explorer shows the
//! folder that file is in. Several Quills can run at once, each on its own project, which is what
//! `File -> New Window` and `File -> Recent Projects` start.
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
}

fn parse_arguments() -> Arguments {
    let mut path = None;
    let mut opacity = None;
    let mut view = None;
    let mut menu_bar = None;
    let mut terminal = false;
    let mut print_menus = false;
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
            "--print-menus" => print_menus = true,
            "--help" | "-h" => {
                println!(
                    "Usage: quill [path] [--opacity N] [--view raw|side|preview] [--menu-bar native|in-window] [--terminal] [--print-menus]"
                );
                println!("  path            a folder to show, or a file to open");
                println!("  --opacity N     background opacity from 0.05 to 1.0");
                println!("  --view MODE     raw, side or preview");
                println!("  --menu-bar WHERE  native for the screen's own bar, in-window for the title bar");
                println!("  --terminal      open the terminal at the bottom");
                println!("  --print-menus   print the menus and their shortcuts, and stop");
                std::process::exit(0);
            }
            other => path = Some(PathBuf::from(other)),
        }
    }
    Arguments { path, opacity, view, menu_bar, terminal, print_menus }
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

    // A file argument opens that file and shows the folder it sits in. A folder argument just shows the
    // folder. With no argument at all, the explorer shows the current directory. The rule lives in the
    // library so that it is tested.
    let fallback = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let (folder, file) = quill_app::resolve_target(arguments.path.as_deref(), &fallback);

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Quill")
            .with_inner_size([1100.0, 720.0])
            .with_min_inner_size([640.0, 400.0])
            .with_transparent(true)
            // Quill draws its own title bar, because rounded corners and a translucent background need the
            // operating system's own window frame turned off.
            .with_decorations(false)
            .with_has_shadow(false),
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
                app.terminal.visible = true;
                app.new_terminal_tab();
            }
            Ok(Box::new(app))
        }),
    )
}
