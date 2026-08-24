//! Starts Quill.
//!
//! The window is created transparent so that the background opacity slider can let the desktop show
//! through. On macOS the window shadow is turned off as well, because the egui documentation records
//! that a translucent window with a shadow leaves ghosting artefacts behind it.
//!
//! Usage: `quill [path] [--opacity N] [--view raw|side|preview]`
//!
//! `path` is a folder to show in the explorer, or a `.md` or `.txt` file to open, in which case the
//! explorer shows the folder that file is in. `--opacity` sets the background opacity between 0.05 and
//! 1.0, which is the same setting the opacity menu changes. `--view` chooses which of the three ways of
//! looking at the file it starts on. Both are there so a starting state can be chosen without clicking,
//! which is what makes it possible to capture the window in a particular state.

// Do not open a console window alongside the application on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use quill_app::app::ViewMode;
use quill_app::QuillApp;

struct Arguments {
    path: Option<PathBuf>,
    opacity: Option<f32>,
    view: Option<ViewMode>,
}

fn parse_arguments() -> Arguments {
    let mut path = None;
    let mut opacity = None;
    let mut view = None;
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
            "--help" | "-h" => {
                println!("Usage: quill [path] [--opacity N] [--view raw|side|preview]");
                println!("  path         a folder to show, or a .md or .txt file to open");
                println!("  --opacity N  background opacity from 0.05 to 1.0");
                println!("  --view MODE  raw, side or preview");
                std::process::exit(0);
            }
            other => path = Some(PathBuf::from(other)),
        }
    }
    Arguments { path, opacity, view }
}

fn main() -> eframe::Result {
    let arguments = parse_arguments();

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

    eframe::run_native(
        "Quill",
        options,
        Box::new(move |cc| {
            let mut app = QuillApp::new(folder);
            app.prepare(&cc.egui_ctx);
            if let Some(file) = file {
                app.open_path(&file);
            }
            if let Some(opacity) = arguments.opacity {
                app.opacity = opacity.clamp(0.05, 1.0);
            }
            if let Some(view) = arguments.view {
                app.view_mode = view;
            }
            Ok(Box::new(app))
        }),
    )
}
