//! Run a program in Unluminous's terminal and write a picture of the window, for a person to look at.
//!
//! `tasks/improvements.md` asks for `claude` and `codex` to be checked in the terminal, and for a resize to
//! be checked as well. Neither can be a comparison test: both programs draw something different every time
//! they run, so an accepted image would fail the next day. So this writes the image and says where it is,
//! and a person or an agent opens it.
//!
//! It builds the real window through the same test harness the screenshot tests use, so what is captured is
//! what the released binary draws, rendered through `wgpu` into an offscreen target rather than onto a
//! screen. That means it can be run over a connection with no display.
//!
//! ```text
//! cargo run --example terminal_capture -- claude
//! cargo run --example terminal_capture -- codex
//! cargo run --example terminal_capture -- --wait 20 claude
//! cargo run --example terminal_capture -- --wait 10 --send "\r" --wait 8 claude
//! ```
//!
//! `--send` types into the terminal, so a program that asks a question can be answered and what it draws
//! next can be captured. `\r` is Return, `\t` is Tab and `\e` is Escape. Each `--send` is typed after the
//! wait in front of it, so the switches read in the order they happen.
//!
//! The pictures go to `design/verification/terminal-<program>.png`, and one more after the tile has been
//! made shorter, which is where a program that has not been told the new size draws in the wrong place.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::vec2;
use egui_kittest::Harness;
use unluminous_app::UnluminousApp;

fn main() {
    let mut arguments = std::env::args().skip(1);
    let mut program = None;
    let mut folder = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    // What to do once the program is running, in the order it was asked for: wait this long, then type
    // this.
    let mut script: Vec<Step> = Vec::new();
    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            "--wait" => {
                if let Some(seconds) = arguments.next().and_then(|v| v.parse().ok()) {
                    script.push(Step::Wait(seconds));
                }
            }
            "--send" => {
                if let Some(text) = arguments.next() {
                    script.push(Step::Send(unescape(&text)));
                }
            }
            "--folder" => {
                if let Some(value) = arguments.next() {
                    folder = PathBuf::from(value);
                }
            }
            "--help" | "-h" => {
                println!(
                    "usage: terminal_capture [--wait SECONDS] [--send TEXT] [--folder PATH] <program> [arguments...]"
                );
                return;
            }
            other => {
                program = Some(other.to_owned());
                break;
            }
        }
    }
    let rest: Vec<String> = arguments.collect();
    let Some(program) = program else {
        eprintln!("say which program to run, for example: terminal_capture claude");
        std::process::exit(2);
    };

    let output = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|path| path.parent())
        .expect("the workspace root is two levels above the crate")
        .join("design/verification");
    std::fs::create_dir_all(&output).expect("make the folder for the captures");

    let window = vec2(1400.0, 900.0);
    let project = folder.clone();
    let mut harness = Harness::builder().with_size(window).wgpu().build_eframe(move |cc| {
        let mut app = UnluminousApp::new(project);
        app.prepare(&cc.egui_ctx);
        app
    });
    harness.run();

    // A taller tile than the usual one, because a full screen program needs room to draw itself.
    harness.state_mut().panes.terminal_height = 520.0;
    harness.state_mut().terminal.visible = true;
    // Through the setting rather than the tab's own copy, because `new_terminal_tab` is what puts the
    // chosen shell on a tab and it is the released binary's path as well as this one's.
    harness.state_mut().settings.terminal_shell = program.clone();
    harness.state_mut().terminal.tabs.settings.args = rest.clone();
    harness.state_mut().terminal.tabs.settings.working_directory = Some(folder.clone());
    harness.state_mut().new_terminal_tab();
    harness.run();

    if harness.state().terminal.tabs.is_empty() {
        eprintln!(
            "{program} did not start: {}",
            harness
                .state()
                .terminal
                .tabs
                .last_error
                .clone()
                .unwrap_or_else(|| "no reason given".to_owned())
        );
        std::process::exit(1);
    }

    // Let it draw, and answer it if there is anything to answer. A program of this kind reads the terminal
    // size, clears the screen and draws itself, and that takes as long as it takes, so the window is run in
    // a loop rather than once.
    if script.is_empty() {
        script.push(Step::Wait(8));
    }
    for step in &script {
        match step {
            Step::Wait(seconds) => {
                let deadline = Instant::now() + Duration::from_secs(*seconds);
                while Instant::now() < deadline {
                    // One frame at a time rather than `run`, which draws until nothing asks for another
                    // frame and gives up after a few: a program with a spinner in it asks for a new frame
                    // for as long as it is running, which is not a fault to stop on.
                    harness.step();
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
            Step::Send(text) => {
                if let Some(session) = harness.state().terminal.tabs.active() {
                    session.send(text.clone().into_bytes());
                }
                println!("typed {text:?}");
                harness.step();
            }
        }
    }

    let size = harness.state().terminal.tabs.active().map(|session| session.size());
    let text = harness
        .state()
        .terminal
        .tabs
        .active()
        .map(|session| session.snapshot().text())
        .unwrap_or_default();
    println!("terminal is {size:?}");
    println!("--- what the terminal holds ---\n{text}\n------------------------------");

    let name = program.replace('/', "-");
    let first = output.join(format!("terminal-{name}.png"));
    save(&mut harness, &first);

    // The same program after the tile has been made shorter and narrower. A program that was not told the
    // new size draws in the wrong place, which is what this second picture shows up.
    harness.state_mut().panes.terminal_height = 300.0;
    harness.state_mut().panes.explorer_width = 420.0;
    for _ in 0..40 {
        harness.step();
        std::thread::sleep(Duration::from_millis(50));
    }
    let resized = harness.state().terminal.tabs.active().map(|session| session.size());
    println!("after the resize the terminal is {resized:?}");
    let second = output.join(format!("terminal-{name}-resized.png"));
    save(&mut harness, &second);
}

/// One thing to do while the program is running.
enum Step {
    Wait(u64),
    Send(String),
}

/// Turn the escapes a shell would swallow into the bytes they stand for, so a Return can be typed from the
/// command line.
fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut characters = text.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            out.push(character);
            continue;
        }
        match characters.next() {
            Some('r') => out.push('\r'),
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('e') => out.push('\u{1b}'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

fn save(harness: &mut Harness<'static, UnluminousApp>, path: &std::path::Path) {
    match harness.render() {
        Ok(image) => {
            image.save(path).expect("save the capture");
            println!("wrote {}", path.display());
        }
        Err(problem) => eprintln!("could not render the window: {problem}"),
    }
}
