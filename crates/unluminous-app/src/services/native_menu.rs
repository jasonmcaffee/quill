//! The menu bar along the top of the screen on macOS.
//!
//! `tasks/improvements.md` asks for the menus to be where macOS puts them, which is the bar along the top
//! of the screen rather than inside the window. egui has no way to make one, so this is built with `muda`,
//! the menu library Tauri uses, which talks to AppKit directly.
//!
//! It is built from [`crate::app::actions::menus`], the same list the bar drawn inside the window is built
//! from, so the two cannot disagree about what `File` holds.
//!
//! How it works, and the two things worth knowing about it:
//!
//! The first submenu in a macOS menu bar is the application menu, and AppKit takes its title from the running
//! program rather than from the menu, so Unluminous's own menu is put first. What AppKit calls the running program
//! is the process name, which for a program run straight from `target/release` is the file name, `unluminous`. So
//! the process name is set to `Unluminous` before the bar is built, and the bar reads
//! `Unluminous  File  Edit  View`.
//!
//! A shortcut on a menu item is a key equivalent, and AppKit hands those to the menu before the window
//! sees them. That is why every shortcut in the menus is handled through an action rather than by reading
//! the keyboard: on macOS the key press never reaches egui at all. The one exception is cut, copy and
//! paste, which the window still handles as egui clipboard events, and which is why those entries are
//! marked as not coming from the keyboard.
//!
//! Rebuilding: the menu is rebuilt when the model changes, which is when a project is added to the recent
//! list, when undo becomes possible, or when the view mode changes. It is not rebuilt every frame, because
//! that would flicker while a menu was open.
//!
//! Waking the window: choosing something from the bar along the top of the screen is not an event the window
//! receives, so eframe, which draws only when something has happened, would not draw again and the menu would
//! seem to do nothing until the pointer was moved over the window. So the handler that takes the menu events
//! asks for a repaint as well as remembering the choice. That is what makes `File -> Save` from the menu bar
//! act at once.

use crate::app::actions::{Action, Menu};

#[cfg(target_os = "macos")]
use std::sync::{Arc, Mutex};

/// The macOS menu bar, or nothing at all on a platform that has no such thing.
pub struct NativeMenu {
    #[cfg(target_os = "macos")]
    inner: Option<Inner>,
    /// What has been chosen from the bar and not yet acted on.
    #[cfg(target_os = "macos")]
    chosen: Chosen,
    /// The model the bar was last built from, so it is only rebuilt when something changed.
    last: Vec<Menu>,
}

#[cfg(target_os = "macos")]
struct Inner {
    /// Held so the bar is not dropped while it is the application's menu. AppKit keeps a reference of its
    /// own, but dropping ours would leave nothing to rebuild from.
    #[allow(dead_code)]
    menu: muda::Menu,
    /// What each item's identifier means. The identifier is the position in this list.
    actions: Vec<Action>,
}

/// What has been chosen from the bar and not yet acted on.
///
/// The handler that AppKit calls runs outside a frame, so what it chose is put here and read on the next
/// frame, which the handler also asks for.
#[cfg(target_os = "macos")]
type Chosen = Arc<Mutex<Vec<String>>>;

impl NativeMenu {
    /// Build the bar and hand it to the application. Does nothing on a platform without one.
    ///
    /// `context` is what the menu wakes the window with when something is chosen from the bar.
    pub fn install(menus: &[Menu], context: Option<&egui::Context>) -> Self {
        #[cfg(target_os = "macos")]
        {
            let chosen: Chosen = Arc::new(Mutex::new(Vec::new()));
            let sink = chosen.clone();
            let context = context.cloned();
            muda::MenuEvent::set_event_handler(Some(move |event: muda::MenuEvent| {
                if let Ok(mut waiting) = sink.lock() {
                    waiting.push(event.id.0.clone());
                }
                // Draw again, because nothing else is going to ask.
                if let Some(context) = &context {
                    context.request_repaint();
                }
            }));
            let mut native = Self { inner: None, last: Vec::new(), chosen };
            native.rebuild(menus);
            native
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = (menus, context);
            Self { last: Vec::new() }
        }
    }

    /// Build the bar again if the menus have changed since the last time.
    pub fn refresh(&mut self, menus: &[Menu]) {
        if self.last == menus {
            return;
        }
        self.rebuild(menus);
    }

    /// The action a menu was clicked for, if any. Called once a frame.
    #[cfg(target_os = "macos")]
    pub fn poll(&self) -> Option<Action> {
        let inner = self.inner.as_ref()?;
        let waiting: Vec<String> = match self.chosen.lock() {
            Ok(mut waiting) => std::mem::take(&mut *waiting),
            Err(_) => return None,
        };
        for id in waiting {
            if let Ok(index) = id.parse::<usize>() {
                if let Some(action) = inner.actions.get(index) {
                    return Some(action.clone());
                }
            }
        }
        None
    }

    #[cfg(not(target_os = "macos"))]
    pub fn poll(&self) -> Option<Action> {
        None
    }

    #[cfg(target_os = "macos")]
    fn rebuild(&mut self, menus: &[Menu]) {
        use muda::{Menu as NativeBar, Submenu};

        name_the_application();
        let bar = NativeBar::new();
        let mut actions: Vec<Action> = Vec::new();
        for menu in menus {
            let submenu = Submenu::new(&menu.name, true);
            if let Err(problem) = append(&submenu, &menu.entries, &mut actions) {
                eprintln!("Unluminous could not build the {} menu: {problem}", menu.name);
            }
            if let Err(problem) = bar.append(&submenu) {
                eprintln!("Unluminous could not add the {} menu to the bar: {problem}", menu.name);
                return;
            }
        }
        bar.init_for_nsapp();
        self.inner = Some(Inner { menu: bar, actions });
        self.last = menus.to_vec();
    }

    #[cfg(not(target_os = "macos"))]
    fn rebuild(&mut self, menus: &[Menu]) {
        self.last = menus.to_vec();
    }
}

/// Tell AppKit the application is called `Unluminous`, so the application menu says so.
///
/// The name in that menu is the process name, and the process name of a program run from `target/release` is
/// the file name of the program, which is `unluminous` in lower case. `NSProcessInfo` lets it be set, and setting
/// it before the menu bar is built is what makes the bar read `Unluminous` rather than `unluminous`. A packaged
/// application would take the name from its bundle instead, and setting it here does no harm in that case.
#[cfg(target_os = "macos")]
fn name_the_application() {
    use objc2_foundation::{ns_string, NSProcessInfo};
    NSProcessInfo::processInfo().setProcessName(ns_string!("Unluminous"));
}

/// Put Unluminous's entries into a native submenu, remembering what each identifier means.
#[cfg(target_os = "macos")]
fn append(
    submenu: &muda::Submenu,
    entries: &[crate::app::actions::Entry],
    actions: &mut Vec<Action>,
) -> muda::Result<()> {
    use crate::app::actions::Entry;
    use muda::{CheckMenuItem, MenuItem, PredefinedMenuItem, Submenu};

    for entry in entries {
        match entry {
            Entry::Separator => submenu.append(&PredefinedMenuItem::separator())?,
            Entry::Item { name, action, shortcut, enabled, checked, .. } => {
                let id = actions.len().to_string();
                actions.push(action.clone());
                let accelerator = shortcut.as_ref().and_then(accelerator);
                if *checked {
                    let item = CheckMenuItem::with_id(id, name, *enabled, true, accelerator);
                    submenu.append(&item)?;
                } else {
                    let item = MenuItem::with_id(id, name, *enabled, accelerator);
                    submenu.append(&item)?;
                }
            }
            Entry::Submenu { name, entries } => {
                // A real submenu here, because that is what macOS draws. The bar inside the window draws
                // the same entries as a heading with rows under it.
                let inner = Submenu::new(name, true);
                append(&inner, entries, actions)?;
                submenu.append(&inner)?;
            }
        }
    }
    Ok(())
}

/// Turn one of Unluminous's shortcuts into a macOS key equivalent.
#[cfg(target_os = "macos")]
fn accelerator(shortcut: &crate::app::actions::Shortcut) -> Option<muda::accelerator::Accelerator> {
    use muda::accelerator::{Accelerator, Modifiers};

    let mut modifiers = Modifiers::empty();
    if shortcut.command {
        modifiers |= muda::accelerator::CMD_OR_CTRL;
    }
    if shortcut.ctrl {
        modifiers |= Modifiers::CONTROL;
    }
    if shortcut.shift {
        modifiers |= Modifiers::SHIFT;
    }
    if shortcut.alt {
        modifiers |= Modifiers::ALT;
    }
    let code = code(shortcut.key)?;
    Some(Accelerator::new(Some(modifiers), code))
}

/// The key code AppKit knows a key by.
///
/// Every letter, digit, function key and punctuation key is here rather than only the ones the menus use
/// today. It used to be only those, and the test below caught what that costs: `Find in Files...` arrived
/// with `Cmd+Shift+F`, `F` was not in the list, and the entry would have appeared in the menu bar with no
/// shortcut at all and no failure anywhere — a silent fault, on macOS only, in a menu nothing on this
/// platform can look at from a test.
///
/// A key that is still not listed shows the entry without a shortcut rather than with the wrong one, and
/// the test says which key it was.
#[cfg(target_os = "macos")]
fn code(key: egui::Key) -> Option<muda::accelerator::Code> {
    use egui::Key;
    use muda::accelerator::Code;
    Some(match key {
        Key::A => Code::KeyA,
        Key::B => Code::KeyB,
        Key::C => Code::KeyC,
        Key::D => Code::KeyD,
        Key::E => Code::KeyE,
        Key::F => Code::KeyF,
        Key::G => Code::KeyG,
        Key::H => Code::KeyH,
        Key::I => Code::KeyI,
        Key::J => Code::KeyJ,
        Key::K => Code::KeyK,
        Key::L => Code::KeyL,
        Key::M => Code::KeyM,
        Key::N => Code::KeyN,
        Key::O => Code::KeyO,
        Key::P => Code::KeyP,
        Key::Q => Code::KeyQ,
        Key::R => Code::KeyR,
        Key::S => Code::KeyS,
        Key::T => Code::KeyT,
        Key::U => Code::KeyU,
        Key::V => Code::KeyV,
        Key::W => Code::KeyW,
        Key::X => Code::KeyX,
        Key::Y => Code::KeyY,
        Key::Z => Code::KeyZ,
        Key::Num0 => Code::Digit0,
        Key::Num1 => Code::Digit1,
        Key::Num2 => Code::Digit2,
        Key::Num3 => Code::Digit3,
        Key::Num4 => Code::Digit4,
        Key::Num5 => Code::Digit5,
        Key::Num6 => Code::Digit6,
        Key::Num7 => Code::Digit7,
        Key::Num8 => Code::Digit8,
        Key::Num9 => Code::Digit9,
        Key::F1 => Code::F1,
        Key::F2 => Code::F2,
        Key::F3 => Code::F3,
        Key::F4 => Code::F4,
        Key::F5 => Code::F5,
        Key::F6 => Code::F6,
        Key::F7 => Code::F7,
        Key::F8 => Code::F8,
        Key::F9 => Code::F9,
        Key::F10 => Code::F10,
        Key::F11 => Code::F11,
        Key::F12 => Code::F12,
        Key::F13 => Code::F13,
        Key::F14 => Code::F14,
        Key::F15 => Code::F15,
        Key::F16 => Code::F16,
        Key::F17 => Code::F17,
        Key::F18 => Code::F18,
        Key::F19 => Code::F19,
        Key::F20 => Code::F20,
        Key::Comma => Code::Comma,
        Key::Period => Code::Period,
        Key::Semicolon => Code::Semicolon,
        Key::Slash => Code::Slash,
        Key::Backslash => Code::Backslash,
        Key::OpenBracket => Code::BracketLeft,
        Key::CloseBracket => Code::BracketRight,
        Key::Quote => Code::Quote,
        Key::Backtick => Code::Backquote,
        // `+` is the shifted `=` on the keys AppKit names, so a shortcut asking for plus is the
        // equals key with no shift on the accelerator: `Cmd+=` is what the bar shows, and the same
        // press is what `Shortcut::matches` accepts inside the window.
        Key::Plus | Key::Equals => Code::Equal,
        Key::Minus => Code::Minus,
        Key::Tab => Code::Tab,
        Key::Enter => Code::Enter,
        Key::Space => Code::Space,
        Key::Escape => Code::Escape,
        Key::Backspace => Code::Backspace,
        Key::Delete => Code::Delete,
        Key::Insert => Code::Insert,
        Key::Home => Code::Home,
        Key::End => Code::End,
        Key::PageUp => Code::PageUp,
        Key::PageDown => Code::PageDown,
        Key::ArrowUp => Code::ArrowUp,
        Key::ArrowDown => Code::ArrowDown,
        Key::ArrowLeft => Code::ArrowLeft,
        Key::ArrowRight => Code::ArrowRight,
        _ => return None,
    })
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use crate::app::actions::Shortcut;

    #[test]
    fn every_shortcut_in_the_menus_has_a_key_code() {
        // A key with no code would be shown without its shortcut, which is a silent fault, so every key
        // the menus use is checked here rather than found by looking at the bar.
        fn walk(entries: &[crate::app::actions::Entry], missing: &mut Vec<String>) {
            for entry in entries {
                match entry {
                    crate::app::actions::Entry::Item { name, shortcut: Some(shortcut), .. } => {
                        if code(shortcut.key).is_none() {
                            missing.push(format!("{name} wants {}", shortcut.label()));
                        }
                    }
                    crate::app::actions::Entry::Submenu { entries, .. } => walk(entries, missing),
                    _ => {}
                }
            }
        }
        let mut missing = Vec::new();
        for menu in crate::app::actions::menus(&crate::app::actions::MenuState::default()) {
            walk(&menu.entries, &mut missing);
        }
        assert!(missing.is_empty(), "these entries would lose their shortcut: {missing:?}");
    }

    #[test]
    fn the_application_is_called_unluminous_rather_than_the_name_of_the_program_file() {
        // The name in the application menu is the process name, and the program file is `unluminous` in lower
        // case. This checks the setting took; what the bar draws can only be seen by looking at it.
        use objc2_foundation::NSProcessInfo;
        name_the_application();
        assert_eq!(NSProcessInfo::processInfo().processName().to_string(), "Unluminous");
    }

    #[test]
    fn the_command_key_becomes_the_apple_key_and_the_others_come_through() {
        use muda::accelerator::{Code, Modifiers};
        let plain = accelerator(&Shortcut::command(egui::Key::S)).expect("Cmd+S has a code");
        assert!(plain.matches(Modifiers::SUPER, Code::KeyS), "Cmd+S should be the Apple key and S");

        let with_shift =
            accelerator(&Shortcut::command_shift(egui::Key::O)).expect("Cmd+Shift+O has a code");
        assert!(with_shift.matches(Modifiers::SUPER | Modifiers::SHIFT, Code::KeyO));

        let terminal = accelerator(&Shortcut::control(egui::Key::Backtick)).expect("Ctrl+` has a code");
        assert!(
            terminal.matches(Modifiers::CONTROL, Code::Backquote),
            "the terminal's shortcut is the control key, not the Apple key"
        );
    }
}
