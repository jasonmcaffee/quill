//! What the menus hold, and one list of everything a menu can ask Unluminate to do.
//!
//! There are two menu bars. On macOS the menus belong in the bar along the top of the screen, which is
//! built with `muda` in `services::native_menu`. Everywhere else they are drawn inside the window by
//! `components::menu_bar`. Both are built from [`menus`], so there is one place that decides what
//! `File` holds and what its shortcuts are, and the two bars cannot drift apart.
//!
//! [`Action`] is what a menu produces. Nothing here does anything: `UnluminateApp::run_action` is the one
//! place an action turns into a change, so the menu, the keyboard and a test all go down the same path.

use std::path::PathBuf;

use crate::app::ViewMode;

/// Everything a menu, or a keyboard shortcut belonging to a menu, can ask for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Another Unluminate window, on its own project.
    NewWindow,
    /// Choose a folder and open it in a window of its own, leaving this one as it is.
    ///
    /// A project is a window, the way the reference editor has it, so there is one entry rather than the two there
    /// used to be. `Open Folder` used to replace the project in this window and `Open Folder in New
    /// Window` sat under it doing what this does now; `task-1658` asks for the second behaviour and
    /// two entries that do the same thing are worse than one.
    OpenFolder,
    /// Choose a file and open it in the editor.
    OpenFile,
    /// Ask for an HTTP address or HTML path and render it in a tab.
    OpenWebAddress,
    /// Render one HTML file selected in the explorer.
    OpenInBrowser(PathBuf),
    /// Open the `Go to File` modal: type part of a name, and open the file it finds.
    ///
    /// A different thing from [`Action::OpenFile`], which asks the platform for a file anywhere on
    /// the disk. This searches the project that is open, which is what a person means nine times out
    /// of ten and is what the reference editor's `Go to File` is.
    GoToFile,
    /// Open the `Find in Files` modal: search every file in the project for some text.
    FindInFiles,
    /// Ask the releases page whether a newer Unluminate exists. `task-1804` §6.
    CheckForUpdates,
    /// Open the Find bar over the file that is showing. `task-1804` §3.1.
    Find,
    /// The same bar with its Replace row already open.
    Replace,
    /// Go to the next match of the Find bar's search, wrapping round the end of the file.
    FindNext,
    /// And the previous one.
    FindPrevious,
    /// Go to where the word at the caret is defined.
    ///
    /// One candidate is a jump; several open the modal listing them, ranked; none says so in the
    /// status bar. Asked from the caret on the menu and the keyboard, and from the pointer by
    /// `Ctrl/Cmd+Click`, which is the same thing with a different offset. On the definition itself
    /// it pivots to the references — the reference editor calls the whole command "Go to Declaration or
    /// Usages", and going to a definition from the definition has no other meaning.
    GoToDefinition,
    /// Open the references modal on the word at the caret: every place that name is used.
    FindReferences,
    /// Open the rename modal on the word at the caret.
    RenameSymbol,
    /// Offer the names the word being typed at the caret could become.
    ///
    /// The same list the popup opens on its own, asked for by hand — so it works from **one**
    /// character rather than two, and it works inside a comment or a string, where the automatic
    /// popup deliberately never opens: somebody who asks in a doc comment deserves the file's words.
    /// With no identifier character to the left of the caret at all it says so in the status bar,
    /// which is what every honest miss in Unluminate does.
    CompleteWord,
    /// Go back to where the caret was before the last jump, reopening the tab if it was closed.
    NavigateBack,
    /// The mirror of it, pushed by [`Action::NavigateBack`] and cleared by any new jump.
    NavigateForward,
    /// Open a project that has been open before, in a window of its own.
    OpenRecent(PathBuf),
    /// Forget the recent projects.
    ForgetRecent,
    Save,
    SaveAs,
    CloseWindow,
    /// Open the Settings modal.
    Settings,
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    /// Show the raw source, the source and preview together, or the preview.
    SetViewMode(ViewMode),
    /// Show or hide the file explorer.
    ToggleExplorer,
    /// Show or hide the editing area: the pane to the right of the explorer, holding the tabs.
    ///
    /// `task-28` asks for it as a toggle in the rail under the folder icon. Hiding it gives the whole width to
    /// whatever panels are showing, which is what somebody reading a project rather than editing one wants.
    /// **It can never leave an empty window**: hiding it with no panel showing shows the explorer as well, and
    /// hiding the last panel while it is hidden brings it back. See `UnluminateApp::run_action`.
    ToggleEditor,
    /// Fill the window with the pane that holds the keyboard, and put everything back. `task-1771`.
    ToggleMaximisedPane,
    /// Show or hide the column of line numbers down the left of the editing area.
    ToggleLineNumbers,
    /// Set the editor's text one size larger, or one smaller, walking the sizes the Settings window
    /// offers. It is the same setting the dialog holds, so it reaches every open file and is still
    /// there next time Unluminate starts.
    ChangeFontSize { larger: bool },
    /// Put the editor's text back to the size a new Unluminate has.
    ResetFontSize,
    /// Show or hide the terminal along the bottom.
    ToggleTerminal,
    /// Show or hide the run tile along the bottom.
    ///
    /// The bottom of the window shows **one** of the two, so showing this one puts the terminal
    /// away and the other way round — two grids stacked take the editing area below the fold of
    /// anything, which is the choice the reference editor's bottom tool windows make too.
    ToggleRunTile,
    /// Show or hide the debug tile along the bottom.
    ///
    /// The third of the three tiles the bottom of the window can hold, on exactly the same terms as
    /// the other two: showing one puts the other two away.
    ToggleDebugTile,
    /// Move a panel to an edge of the window — `task-1697`.
    ///
    /// The same change the drag makes, so the pointer and the menu go down one path:
    /// `UnluminateApp::dock_the_panel`. Where in that side is not part of the action, because a menu row
    /// can only say "the left"; the drag is what says whether that means before or after whatever is
    /// there already, and `unluminate-cli panel dock --position` is what says it in a script.
    Dock { panel: crate::app::dock::Panel, side: crate::app::dock::Side },
    /// Put every panel back where a new Unluminate has it.
    ResetPanelLayout,
    /// Anything on the Run menu, or on the run widget in the title bar.
    Run(RunAction),
    /// Anything on the Run menu's debug half, the debug tile or the gutter's own menu.
    Debug(DebugAction),
    /// Close the file tab that is showing.
    CloseTab,
    /// Show the next file tab in this pane, wrapping round at the end.
    NextTab,
    /// Show the previous file tab in this pane.
    PreviousTab,
    /// Put a pane to the right of the one with the keyboard and move the tab that is showing into it.
    ///
    /// The reference editor's `Split Right` shows the same file in **both** splits and Unluminate cannot, because two
    /// tabs on one file would be two documents over one path. This is the reference editor's `Split and Move
    /// Right` under the name a person looks for; `tasks/task-1664-split-view-tdd.md` §3 records what
    /// was weighed. Splitting a pane that holds one tab opens the new pane empty instead, because
    /// taking its only tab away would leave the window looking exactly as it did.
    SplitRight,
    /// Move the tab that is showing into the pane beside it.
    MoveTabRight,
    MoveTabLeft,
    /// Fold the pane that has the keyboard into the one beside it.
    Unsplit,
    /// Every tab back into one pane.
    UnsplitAll,
    /// Put the keyboard in the next pane, wrapping round at the right hand end.
    NextPane,
    PreviousPane,
    /// Scroll the explorer to the file that is showing, opening out the folders above it.
    ///
    /// It happens on its own when the file that is showing changes; this is the same thing asked for
    /// by hand, which is the reference editor's button of the same name.
    SelectOpenFile,
    /// Another terminal tab.
    NewTerminalTab,
    /// Close the terminal tab that is showing.
    CloseTerminalTab,
    /// Call the terminal tab that is showing something else. The name is asked for first.
    ///
    /// The name a person types beats the title the program sets, which is the whole point: `claude`
    /// sets a title on every prompt, so a rename a program could overwrite would last seconds. An
    /// empty name puts the tab back to being named after its program.
    RenameTerminalTab,
    /// Make an empty file in this folder. The name is asked for first.
    NewFile(PathBuf),
    /// Make a folder inside this one. The name is asked for first.
    NewFolder(PathBuf),
    /// Hold this path to be moved when something is pasted.
    CutPath(PathBuf),
    /// Hold this path to be copied when something is pasted.
    CopyPath(PathBuf),
    /// Put this path's text on the system clipboard.
    CopyPathReference(PathBuf),
    /// Put whatever was cut or copied into this folder.
    PasteInto(PathBuf),
    /// Rename this file or folder. The new name is asked for first, and what refers to it follows
    /// it, because a rename is a move to a new name.
    RenamePath(PathBuf),
    /// Throw this file or folder away. The question is asked first, and where it goes is
    /// `services::recycle`'s answer.
    DeletePath(PathBuf),
    /// Show this path in the platform's file manager.
    RevealPath(PathBuf),
    /// Read this folder again, and this file again if it is open.
    ReloadPath(PathBuf),
    /// Anything on the Git menu.
    Git(GitAction),
    /// Mark the selected passage in one of the four colours the menu offers.
    ///
    /// The colour chosen in the wheel is not here, because an action is named and a name cannot
    /// carry a colour and an opacity. Both go through `UnluminateApp::highlight_selection`, so there is
    /// still one place a passage is marked.
    Highlight(HighlightColor),
    /// Take away the mark under the caret.
    ClearHighlight,
    /// Take away every mark in the file that is showing.
    ClearHighlights,
    /// Anything on the `View -> Folding` menu.
    Fold(FoldAction),
    /// Unluminate's own about box, which is a line in the status bar rather than a window.
    About,
    Quit,
    /// Show or hide a pane a plugin contributed, named `<plugin id>/<pane id>`.
    ///
    /// A `String` where every other variant is a unit or a small value, because the set of panes is
    /// decided when the manifests are read rather than at compile time. That is the one property the
    /// four docked panels could not have, and it is why the pane is named rather than numbered.
    PluginPane { pane: String },
    /// Run a command a plugin declared, from its own menu.
    ///
    /// The plugin's id and the command's name, which are exactly what `unluminate-cli plugin run` carries and
    /// what a button inside the pane calls, so all three reach `UiProvider::command` by one path.
    PluginCommand { plugin: String, command: String },
    /// Open a plugin's own tab in the editing area, named `<plugin id>/<tab id>`.
    PluginTab { tab: String },

}

/// The six things that can be done to the blocks of the file that is showing.
///
/// All six are about **the file that is showing** and take nothing, which is what makes them
/// ordinary parameterless actions the View menu, the right click menus, the keyboard and
/// `unluminate-cli action run` can all ask for. `tasks/task-1686-folding-tdd.md` section 8, and
/// `tasks/task-1707-recursive-folding-tdd.md` for the two recursive ones.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FoldAction {
    /// Collapse or expand the innermost block the caret is in.
    Toggle,
    /// Collapse every block in the file.
    All,
    /// Show all again.
    None_,
    /// Collapse every block that does not hold a marked passage — the ticket's `Collapse All But
    /// Highlighted`. With nothing marked it falls back to the selection.
    Others,
    /// Collapse the innermost block the caret is in, and every block inside it.
    CollapseRecursively,
    /// Expand the innermost block the caret is in, and every block inside it.
    ExpandRecursively,
}

impl FoldAction {
    pub const ALL: [FoldAction; 6] = [
        FoldAction::Toggle,
        FoldAction::All,
        FoldAction::None_,
        FoldAction::Others,
        FoldAction::CollapseRecursively,
        FoldAction::ExpandRecursively,
    ];

    /// What the command line calls it, after the `fold-` prefix.
    pub fn name(self) -> &'static str {
        match self {
            FoldAction::Toggle => "toggle",
            FoldAction::All => "all",
            FoldAction::None_ => "none",
            FoldAction::Others => "others",
            FoldAction::CollapseRecursively => "collapse-recursive",
            FoldAction::ExpandRecursively => "expand-recursive",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.name() == name)
    }

    /// What the menu row says.
    pub fn label(self) -> &'static str {
        match self {
            FoldAction::Toggle => "Collapse or Expand Block",
            FoldAction::All => "Collapse All",
            FoldAction::None_ => "Expand All",
            FoldAction::Others => "Collapse All But Highlighted",
            FoldAction::CollapseRecursively => "Collapse Recursively",
            FoldAction::ExpandRecursively => "Expand Recursively",
        }
    }

    /// The key it is on.
    ///
    /// The reference editor puts folding on the numeric keypad, which half the keyboards in this house have not
    /// got, so its `Ctrl+.` — the key it folds a selection with — is the one taken here and the
    /// rest are built round it. The full stop is next to the comma that already opens Settings,
    /// which is what makes a set of six worth remembering: the more modifiers, the wider the reach,
    /// so the two recursive ones are the same two keys with the one remaining modifier added.
    pub fn shortcut(self) -> Shortcut {
        match self {
            FoldAction::Toggle => Shortcut::command(egui::Key::Period),
            FoldAction::All => Shortcut::command_shift(egui::Key::Period),
            FoldAction::None_ => Shortcut::command_shift(egui::Key::Comma),
            FoldAction::Others => Shortcut { alt: true, ..Shortcut::command(egui::Key::Period) },
            FoldAction::CollapseRecursively => {
                Shortcut { alt: true, shift: true, ..Shortcut::command(egui::Key::Period) }
            }
            FoldAction::ExpandRecursively => {
                Shortcut { alt: true, shift: true, ..Shortcut::command(egui::Key::Comma) }
            }
        }
    }
}

/// The four colours the editor's right click menu offers as blocks.
///
/// Four rather than a list, because a marker pen has a handful of colours and a person choosing
/// between twenty has been given a job rather than a tool. Anything else is the colour wheel, which
/// carries a value no menu entry could name and so is not an [`Action`] at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HighlightColor {
    Yellow,
    Green,
    Blue,
    Pink,
}

impl HighlightColor {
    pub const ALL: [HighlightColor; 4] =
        [HighlightColor::Yellow, HighlightColor::Green, HighlightColor::Blue, HighlightColor::Pink];

    /// What the menu row says, and what the command line calls it once it is lower case.
    pub fn label(&self) -> &'static str {
        match self {
            HighlightColor::Yellow => "Yellow",
            HighlightColor::Green => "Green",
            HighlightColor::Blue => "Blue",
            HighlightColor::Pink => "Pink",
        }
    }

    /// The colour itself, from the closed palette in `theme::color`.
    pub fn rgba(&self) -> unluminate_core::Rgba {
        use crate::theme::color;
        match self {
            HighlightColor::Yellow => color::HIGHLIGHT_YELLOW,
            HighlightColor::Green => color::HIGHLIGHT_GREEN,
            HighlightColor::Blue => color::HIGHLIGHT_BLUE,
            HighlightColor::Pink => color::HIGHLIGHT_PINK,
        }
    }

    /// The same colour as egui wants it, for the block the menu draws.
    pub fn color(&self) -> egui::Color32 {
        crate::theme::color32(self.rgba())
    }

    /// The colour of this name: one of the four, or `#rrggbb` / `#rrggbbaa`.
    ///
    /// The command line takes both, so `--color green` and `--color "#7FCA9866"` are both a colour
    /// and neither needs the other explained.
    pub fn parse(name: &str) -> Option<unluminate_core::Rgba> {
        let name = name.trim();
        for colour in Self::ALL {
            if colour.label().eq_ignore_ascii_case(name) {
                return Some(colour.rgba());
            }
        }
        unluminate_core::Rgba::parse(name)
    }

    /// The four names a person can type, for a message that has to list them.
    pub fn names() -> String {
        Self::ALL
            .iter()
            .map(|colour| colour.label().to_ascii_lowercase())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

impl Action {
    /// True when a text box that has the keyboard does this for itself.
    ///
    /// Undo, redo and select all mean the box being typed in and not the document while one of the
    /// window's text boxes has the keyboard, which is what every editor does and what egui's own
    /// text box already implements. Without this, control and Z in the explorer's filter box cleared
    /// the box and undid an edit in the file behind it with the one press.
    ///
    /// Three, and no more. Cut, copy and paste are already marked as not coming from the keyboard,
    /// because the platform delivers them as clipboard events, so they never reach the keyboard
    /// watcher at all. Everything else on every menu keeps working while a box has the keyboard:
    /// control and S in a search box saves the file, as it does in every other editor.
    pub fn belongs_to_a_focused_text_box(&self) -> bool {
        matches!(self, Action::Undo | Action::Redo | Action::SelectAll)
    }
}

/// Everything the Run menu and the run widget can ask for.
///
/// A group of its own rather than six more variants of [`Action`], for the reason [`GitAction`] is
/// one: they all go to one place, and a menu's worth of entries reads better as a list than as six
/// more lines in an enum shared with `Save`.
///
/// The three that take a name take `None` to mean **the configuration the widget has chosen**, so
/// one entry serves the menu, the widget, the keyboard and the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunAction {
    /// Start a configuration. Starting one that is already running stops it and starts it again —
    /// a rerun, not a second copy, which is `task-1683` §5.2 and the reference editor's own default.
    Start(Option<String>),
    /// Stop it: politely the first time, and for good the second.
    Stop(Option<String>),
    /// Stop it and start it again, whatever state it was in.
    Rerun(Option<String>),
    /// Choose a configuration without running it, which is what clicking a row of the widget's
    /// flyout does.
    Select(String),
    /// Run the file that is showing, through its language's `run.file`. **Absent** rather than
    /// dimmed for a file whose language names no command, which is the rule the three
    /// code-navigation entries already follow.
    CurrentFile,
    /// Open the `Run Configurations` modal.
    Edit,
}

impl RunAction {
    /// The name the command line calls this entry, after the `run-` that [`Action::name`] puts in
    /// front of it.
    pub fn name(&self) -> &'static str {
        match self {
            RunAction::Start(_) => "start",
            RunAction::Stop(_) => "stop",
            RunAction::Rerun(_) => "rerun",
            RunAction::Select(_) => "select",
            RunAction::CurrentFile => "current-file",
            RunAction::Edit => "edit",
        }
    }

    /// The entry of this name. `named` is the configuration it is about, and `None` means the one
    /// the widget has chosen.
    pub fn from_name(name: &str, named: Option<String>) -> Option<RunAction> {
        Some(match name {
            "start" => RunAction::Start(named),
            "stop" => RunAction::Stop(named),
            "rerun" => RunAction::Rerun(named),
            "select" => RunAction::Select(named.unwrap_or_default()),
            "current-file" => RunAction::CurrentFile,
            "edit" => RunAction::Edit,
            _ => return None,
        })
    }

    /// The configuration this entry names, if it names one.
    pub fn configuration(&self) -> Option<&str> {
        match self {
            RunAction::Start(named) | RunAction::Stop(named) | RunAction::Rerun(named) => {
                named.as_deref()
            }
            RunAction::Select(named) => Some(named),
            RunAction::CurrentFile | RunAction::Edit => None,
        }
    }
}

/// Everything the Run menu's debug half, the debug tile and the gutter's menu can ask for.
///
/// A group of its own for [`RunAction`]'s reason: they all go to one place, and a menu's worth of
/// entries reads better as a list than as twelve more lines in an enum shared with `Save`.
///
/// [`DebugAction::Start`] takes `None` to mean **the configuration the widget has chosen**, exactly
/// as `RunAction::Start` does, so one entry serves the menu, the keyboard and the command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DebugAction {
    /// Start a configuration under its debugger. **Debug is Run, under a debugger**: the same
    /// configuration the play button starts, which is the reference editor's own model.
    Start(Option<String>),
    /// Debug the file that is showing, through its language's `run.file`. Offered only where
    /// `Run Current File` is offered **and** the language names an adapter.
    CurrentFile,
    /// End the session: politely the first time, and for good the second.
    Stop,
    /// Run to the next breakpoint. The reference editor's `F9`.
    Resume,
    /// Run this line and stop on the next. `F8`.
    StepOver,
    /// Go into the call on this line. `F7`.
    StepInto,
    /// Finish this function and stop in the caller. `Shift+F8`.
    StepOut,
    /// Stop a program that is running, which is the one of the five that is not a step.
    Pause,
    /// Run to the line the caret is on. `Alt+F9`. DAP has no request for it: it is a temporary
    /// breakpoint, a resume, and the breakpoint taken away at the next stop.
    RunToCursor,
    /// Put a breakpoint on the line in question, or take away the one that is there. `Ctrl+F8`.
    ToggleBreakpoint,
    /// Open the modal that edits a breakpoint's condition, its log message and whether it is on.
    EditBreakpoint,
    /// Switch the breakpoint on the line in question off without taking it away, or back on again.
    /// The reference editor's `Disable Breakpoint`, which is a different thing from removing one: a disabled
    /// breakpoint keeps its condition and is drawn hollow.
    ToggleBreakpointEnabled,
    /// Show the value of the name at the caret in a popup, which is the value tooltip asked for by
    /// hand. `Ctrl/Cmd+Alt+F8`, which is the reference editor's own Quick Evaluate chord. `task-1696`.
    ShowValue,
    /// Open the expression box. `Alt+F8`.
    EvaluateExpression,
    /// Show the debug tile along the bottom, or put it away.
    ToggleTile,
    /// Install the named debug adapter, by running its own install command in the run tile.
    ///
    /// `task-1692`: an adapter that is missing is one press away from being installed, and the press
    /// runs a visible command in a terminal rather than the editor reaching out for anything. The
    /// name is the adapter's — `lldb`, `node` — and an empty one means the one the session that could
    /// not start was asking for.
    InstallAdapter(String),
}

impl DebugAction {
    /// The name the command line calls this entry, after the `debug-` that [`Action::name`] puts in
    /// front of it.
    pub fn name(&self) -> &'static str {
        match self {
            DebugAction::Start(_) => "start",
            DebugAction::CurrentFile => "current-file",
            DebugAction::Stop => "stop",
            DebugAction::Resume => "resume",
            DebugAction::StepOver => "step-over",
            DebugAction::StepInto => "step-into",
            DebugAction::StepOut => "step-out",
            DebugAction::Pause => "pause",
            DebugAction::RunToCursor => "run-to-cursor",
            DebugAction::ToggleBreakpoint => "toggle-breakpoint",
            DebugAction::EditBreakpoint => "edit-breakpoint",
            DebugAction::ToggleBreakpointEnabled => "toggle-breakpoint-enabled",
            DebugAction::ShowValue => "show-value",
            DebugAction::EvaluateExpression => "evaluate",
            DebugAction::ToggleTile => "toggle-tile",
            DebugAction::InstallAdapter(_) => "install",
        }
    }

    /// The entry of this name. `named` is the configuration it is about, and `None` means the one
    /// the widget has chosen.
    pub fn from_name(name: &str, named: Option<String>) -> Option<DebugAction> {
        Some(match name {
            "start" => DebugAction::Start(named),
            "current-file" => DebugAction::CurrentFile,
            "stop" => DebugAction::Stop,
            "resume" => DebugAction::Resume,
            "step-over" => DebugAction::StepOver,
            "step-into" => DebugAction::StepInto,
            "step-out" => DebugAction::StepOut,
            "pause" => DebugAction::Pause,
            "run-to-cursor" => DebugAction::RunToCursor,
            "toggle-breakpoint" => DebugAction::ToggleBreakpoint,
            "edit-breakpoint" => DebugAction::EditBreakpoint,
            "toggle-breakpoint-enabled" => DebugAction::ToggleBreakpointEnabled,
            "show-value" => DebugAction::ShowValue,
            "evaluate" => DebugAction::EvaluateExpression,
            "toggle-tile" => DebugAction::ToggleTile,
            "install" => DebugAction::InstallAdapter(named.unwrap_or_default()),
            _ => return None,
        })
    }

    /// The configuration this entry names, if it names one.
    pub fn configuration(&self) -> Option<&str> {
        match self {
            DebugAction::Start(named) => named.as_deref(),
            // Not a configuration but an adapter's name, which travels in the same field because it
            // is the same thing to the command line: the one word this entry is about.
            DebugAction::InstallAdapter(adapter) => Some(adapter.as_str()).filter(|name| !name.is_empty()),
            _ => None,
        }
    }
}

/// Everything the Git menu can ask for.
///
/// A group of its own rather than twenty more variants of [`Action`], because they all go to one
/// place and because a menu with twenty entries reads better as a list than as twenty lines in an
/// enum shared with `Save`.
///
/// The ones that take a path take `None` to mean the file that is open, so one entry serves both the
/// Git menu and a right click on a row in the explorer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitAction {
    /// Open the commit panel, or shut it when it is already open. The rail's git button and the menu
    /// entry are the same thing, so pressing the button twice puts the panel away again.
    Commit,
    /// Stage a path.
    Add(Option<PathBuf>),
    /// Show what changed in a file against what git has.
    ShowDiff(Option<PathBuf>),
    /// The same, against a revision that is asked for.
    CompareWithRevision(Option<PathBuf>),
    /// The commits that touched a file, or the whole repository when there is no path.
    ShowHistory(Option<PathBuf>),
    /// The commit that is checked out.
    ShowCurrentRevision,
    /// Throw away the changes to a path. Asked about first, because there is no undo for it.
    Rollback(Option<PathBuf>),
    /// Annotate the open file with git blame, or stop annotating it.
    Annotate,
    Push,
    Pull,
    Fetch,
    Merge,
    Rebase,
    /// Finish, or abandon, a merge or a rebase that stopped on a conflict.
    Continue,
    Abort,
    Branches,
    NewBranch,
    NewTag,
    ResetHead,
    Stash,
    Unstash,
    Remotes,
    Clone,
    /// Open the file where a path is ignored without changing what is committed.
    Exclude,
    /// Read the repository again.
    Refresh,
}

/// A keyboard shortcut, held as egui's own key and modifiers so that matching a key press is a
/// comparison rather than a translation.
///
/// `command` is the Apple key on macOS and the control key on Windows, which is what egui reports for a
/// shortcut on either platform, and what a person means by the modifier a menu shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Shortcut {
    pub key: egui::Key,
    pub command: bool,
    pub shift: bool,
    pub alt: bool,
    /// The control key itself, as distinct from `command`. Only the terminal uses it, for Control and
    /// backtick, because that is the shortcut every editor with a terminal uses.
    pub ctrl: bool,
}

impl Shortcut {
    /// The command key and a key, which is nearly every shortcut Unluminate has.
    pub const fn command(key: egui::Key) -> Self {
        Self { key, command: true, shift: false, alt: false, ctrl: false }
    }

    pub const fn command_shift(key: egui::Key) -> Self {
        Self { key, command: true, shift: true, alt: false, ctrl: false }
    }

    pub const fn control(key: egui::Key) -> Self {
        Self { key, command: false, shift: false, alt: false, ctrl: true }
    }

    pub const fn control_shift(key: egui::Key) -> Self {
        Self { key, command: false, shift: true, alt: false, ctrl: true }
    }

    /// A key with no modifier at all.
    ///
    /// One entry uses it — `Delete` in the explorer's menu — and it is marked as not coming from
    /// the keyboard, because a bare key that the menu watcher fired would delete a file every time
    /// somebody pressed Delete while typing.
    pub const fn plain(key: egui::Key) -> Self {
        Self { key, command: false, shift: false, alt: false, ctrl: false }
    }

    /// True when this key press is this shortcut, and not a longer one that happens to include it.
    ///
    /// The two platforms have to be told apart, because they do not agree about what the control key
    /// is. On macOS the Apple key and the control key are two keys, and egui reports the Apple key as
    /// both `command` and `mac_cmd` while leaving `ctrl` alone, so the two can be compared
    /// separately. Everywhere else they are **one key**: egui sets `command` equal to `ctrl`, so a
    /// press of control satisfies a shortcut asking for either, and a shortcut asking for both would
    /// be asking for the same key twice.
    ///
    /// Comparing all four fields on both platforms, which this used to do, meant that on Windows
    /// every shortcut in the bar was unreachable: `Ctrl+S` arrives with `command` and `ctrl` both
    /// set, so `Save`, which asks for `command` and not `ctrl`, never matched. The tests did not
    /// catch it because they built a modifier set with `command` set and `ctrl` clear, which is a
    /// combination Windows never produces. They now build the set the platform really sends.
    pub fn matches(&self, key: egui::Key, modifiers: &egui::Modifiers) -> bool {
        // `+` and `=` are one key on nearly every layout, and `+` is the shifted one, so a shortcut
        // asking for plus accepts either and does not care whether shift is held: what a person
        // means by "control and plus" is that key, however their keyboard happens to label it. The
        // numeric keypad sends plus with no shift at all, which is the same key press again. Every
        // other shortcut still compares shift exactly, which is what keeps `Cmd+S` and `Cmd+Shift+S`
        // apart.
        if self.key == egui::Key::Plus {
            if !matches!(key, egui::Key::Plus | egui::Key::Equals) || modifiers.alt != self.alt {
                return false;
            }
        } else if self.key != key || modifiers.shift != self.shift || modifiers.alt != self.alt {
            return false;
        }
        if cfg!(target_os = "macos") {
            modifiers.command == self.command && (modifiers.ctrl && !modifiers.mac_cmd) == self.ctrl
        } else {
            // Either flag counts as the control key being held. The platform sets both, and
            // `egui::Modifiers::COMMAND` — which is what a test presses — sets only `command`.
            let control = modifiers.ctrl || modifiers.command;
            control == (self.command || self.ctrl)
        }
    }

    /// What a menu shows to the right of the entry, spelled out in words.
    ///
    /// Words rather than the Apple symbols: the command symbol at U+2318 is in egui's fonts but the shift
    /// symbol at U+21E7 is not, and it came out as an empty box. Mixing one symbol with one word reads
    /// worse than spelling both, and words work on either platform.
    pub fn label(&self) -> String {
        let mut parts: Vec<&str> = Vec::new();
        if self.command {
            parts.push(if cfg!(target_os = "macos") { "Cmd" } else { "Ctrl" });
        }
        if self.ctrl {
            parts.push("Ctrl");
        }
        if self.alt {
            parts.push(if cfg!(target_os = "macos") { "Option" } else { "Alt" });
        }
        if self.shift {
            parts.push("Shift");
        }
        let key = key_name(self.key);
        parts.push(key);
        parts.join("+")
    }
}

/// The name of a key as a menu spells it.
pub fn key_name(key: egui::Key) -> &'static str {
    match key {
        egui::Key::Comma => ",",
        // The full stop beside it, spelled the same way, or one menu would read `Ctrl+Shift+,` and
        // the row under it `Ctrl+Shift+Period`. egui's own name for it is the word.
        egui::Key::Period => ".",
        egui::Key::Backtick => "`",
        egui::Key::Num0 => "0",
        egui::Key::Num1 => "1",
        egui::Key::Num2 => "2",
        egui::Key::Num3 => "3",
        other => other.name(),
    }
}

/// One row of a menu.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// Something to do, with the shortcut a menu shows next to it.
    Item {
        name: String,
        action: Action,
        shortcut: Option<Shortcut>,
        /// False when it cannot be done just now, such as undo with nothing to undo. The row is drawn
        /// dimmed and takes no clicks.
        enabled: bool,
        /// True when it is switched on, such as the view mode that is showing.
        checked: bool,
        /// False when the shortcut belongs to something else and this menu must not watch the keyboard
        /// for it.
        ///
        /// Cut, copy and paste are the only ones. Inside the window they are delivered by egui as
        /// clipboard events rather than as key presses, because that is how the platform hands over the
        /// clipboard, so watching for the key press as well would do the work twice.
        keyboard: bool,
    },
    /// A line between two groups.
    Separator,
    /// A menu inside a menu, which is what Recent Projects is.
    Submenu { name: String, entries: Vec<Entry> },
}

impl Entry {
    fn item(name: &str, action: Action) -> Self {
        Entry::Item {
            name: name.to_owned(),
            action,
            shortcut: None,
            enabled: true,
            checked: false,
            keyboard: true,
        }
    }

    fn with_shortcut(name: &str, action: Action, shortcut: Shortcut) -> Self {
        match Entry::item(name, action) {
            Entry::Item { name, action, enabled, checked, keyboard, .. } => Entry::Item {
                name,
                action,
                shortcut: Some(shortcut),
                enabled,
                checked,
                keyboard,
            },
            other => other,
        }
    }

    fn enabled(self, yes: bool) -> Self {
        match self {
            Entry::Item { name, action, shortcut, checked, keyboard, .. } => {
                Entry::Item { name, action, shortcut, enabled: yes, checked, keyboard }
            }
            other => other,
        }
    }

    fn checked(self, yes: bool) -> Self {
        match self {
            Entry::Item { name, action, shortcut, enabled, keyboard, .. } => {
                Entry::Item { name, action, shortcut, enabled, checked: yes, keyboard }
            }
            other => other,
        }
    }

    /// Mark an entry whose shortcut is delivered another way, so the keyboard watcher leaves it alone.
    fn not_from_the_keyboard(self) -> Self {
        match self {
            Entry::Item { name, action, shortcut, enabled, checked, .. } => {
                Entry::Item { name, action, shortcut, enabled, checked, keyboard: false }
            }
            other => other,
        }
    }
}

/// One menu in the bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Menu {
    pub name: String,
    pub entries: Vec<Entry>,
}

/// What the menus need to know about the window to say what can be done and what is switched on.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MenuState {
    /// The menus the plugins that are switched on contributed, in the order the plugins are listed.
    ///
    /// Worked out from `plugins::Surfaces` when the plugins are read rather than once a frame, so a menu
    /// appears and disappears with its plugin and nothing walks a manifest while the window is drawing.
    pub plugin_menus: Vec<PluginMenu>,
    pub can_undo: bool,
    pub can_redo: bool,
    pub has_selection: bool,
    /// True while the Find bar is open, which is what decides whether Find Next can be used.
    /// `task-1804`.
    pub finding: bool,
    pub recent: Vec<PathBuf>,
    pub view_mode: ViewMode,
    /// True when the open file has a preview worth switching to, which is what dims the three view
    /// mode entries for a source file. The toolbar asks the same question of the same function, so
    /// the menu and the buttons cannot disagree about whether there is a preview.
    pub can_preview: bool,
    /// Which of the two kinds of preview it is, so the three entries say `Mermaid` over a diagram
    /// and `Markdown` over prose. The buttons take the same answer from the same function.
    pub preview_kind: crate::services::file_kind::PreviewKind,
    pub explorer_visible: bool,
    /// Whether the editing area is showing, so `View` says `Hide Editor` or `Show Editor`. `task-28`.
    pub editor_visible: bool,
    /// Whether one pane is filling the window, which is what makes the entry read `Restore Pane`.
    pub maximised: bool,
    /// Which edge each panel is docked to, so a panel's own menu can tick the side it is already on
    /// — `task-1697`. The whole arrangement rather than four sides, because it is one value the
    /// window already holds and its `Default` is the arrangement Unluminate ships with.
    pub dock: crate::app::dock::Layout,
    pub line_numbers: bool,
    pub terminal_visible: bool,
    pub terminal_tabs: usize,
    /// True when the folder that is open is in a git repository. With none, every git entry is
    /// dimmed rather than absent, so the menu does not change shape depending on where you are.
    pub in_repository: bool,
    /// True when a file is open that git could say something about.
    pub has_file: bool,
    /// True when the open file has been annotated with blame.
    pub annotated: bool,
    /// Set while a merge or a rebase has stopped on a conflict.
    pub unfinished: Option<&'static str>,
    /// How many files are open, which is what decides whether the tab entries can be used.
    pub open_files: usize,
    /// How many panes the editing area is divided into, which is what dims `Unsplit`.
    pub panes: usize,
    /// Which pane has the keyboard, which is what dims `Move Left` at the left hand end.
    pub pane: usize,
    /// How many tabs are in the pane that has the keyboard, which is what decides whether the tab
    /// entries can be used and what `Split Right` will do.
    pub tabs_in_pane: usize,
    /// How many passages are marked in the file that is showing, which is what decides whether
    /// `Clear All Highlights` can be used.
    pub highlights: usize,
    /// True when the caret is inside a marked passage, so there is one to clear.
    pub on_a_highlight: bool,
    /// True when blocks in the open file can be collapsed at all, which is everything but a
    /// picture. **Absent** rather than dimmed when it is false, like the code navigation entries.
    pub folding_applies: bool,
    /// How many blocks the open file has, which is what dims `Collapse All`.
    pub foldable: usize,
    /// How many of them are collapsed, which is what dims `Expand All`.
    pub folded: usize,
    /// True when the open file's language has said what a definition looks like, which is what puts
    /// `Go to Definition` on the menu. **Absent** rather than dimmed when it is false: a control
    /// that can never apply to this kind of file is not a control that is unavailable just now.
    pub definitions_apply: bool,
    /// True when the open file has a language at all, which is what `Find References` and
    /// `Rename Symbol` need — neither needs a definition, so a stylesheet keeps both.
    pub symbols_apply: bool,
    /// True when a switched-on plugin claims the open file, which is what puts `Complete Word` on
    /// the menu. **Absent** rather than dimmed when it is false, like the three entries above it:
    /// prose and a picture have no words worth offering and never will.
    pub completion_applies: bool,
    /// Whether there is anywhere to go back to, and forward to.
    pub can_go_back: bool,
    pub can_go_forward: bool,
    /// The run configuration the widget has chosen, if one is. What `Run <name>` is named after,
    /// and what dims the whole top of the Run menu when there is none.
    pub run_selected: Option<String>,
    /// Every configuration the project offers, in the order the widget's flyout lists them:
    /// permanents, then temporaries, then what the detectors suggest.
    pub run_names: Vec<String>,
    /// True when the chosen configuration is running, which is what un-dims `Stop`.
    pub run_active: bool,
    /// True when the open file's language has said how one file of it is run, which is what puts
    /// `Run Current File` on the menu. **Absent** rather than dimmed when it is false, like the
    /// code-navigation entries above: a `.rs` file will never have one, because running one file
    /// of a Cargo project is not a thing cargo does.
    pub run_file_applies: bool,
    /// True when the run tile is the one showing along the bottom.
    pub run_tile_visible: bool,
    /// True when the open file's language names a debugger, which is what puts the whole debug half
    /// of the Run menu and the two gutter entries in front of it. **Absent** rather than dimmed when
    /// it is false, like the code-navigation entries: a stylesheet has nothing to step through.
    pub debug_applies: bool,
    /// True while a session exists at all, which is what un-dims `Stop Debugging`.
    pub debug_active: bool,
    /// True while the program is stopped somewhere, which is what un-dims the stepping entries.
    pub debug_paused: bool,
    /// True when the debug tile is the one showing along the bottom.
    pub debug_tile_visible: bool,
    /// True when the line the gutter's menu was opened over already has a breakpoint, which is what
    /// decides whether that menu offers to remove one or to set one.
    pub on_a_breakpoint: bool,
    /// True when that breakpoint is switched on, which is what `Disable Breakpoint` says.
    pub breakpoint_enabled: bool,
}

/// The whole menu bar: `Unluminate`, `File`, `Edit` and `View`, in that order.
///
/// `Unluminate` comes first because that is where the application's own entries belong, and because macOS puts
/// the application menu first whatever it is called. Inside the window it is drawn first for the same
/// reason, so the bar reads `Unluminate  File  Edit  View` on both platforms.
pub fn menus(state: &MenuState) -> Vec<Menu> {
    let mut found = vec![
        unluminate_menu(),
        file_menu(state),
        edit_menu(state),
        find_menu(state),
        view_menu(state),
        run_menu(state),
        git_menu(state),
    ];
    // Then one menu per plugin that contributed one, in the order the plugins are listed. **After**
    // Unluminate's own seven, so `Unluminate`, `File`, `Edit`, `Find`, `View`, `Run` and `Git` never
    // move: a menu bar whose entries shift when a plugin is installed is a menu bar somebody's hand
    // has to relearn.
    //
    // A plugin cannot add an entry to one of the six. VS Code allows that through about forty named
    // anchors with `when` expressions, which is the largest part of its contribution model and the
    // hardest to keep tested. `tasks/ui-plugin-architecture.md` §10 records what adding anchors would
    // take; it changes nothing here.
    found.extend(state.plugin_menus.iter().map(plugin_menu));
    found
}

/// One plugin's menu, turned from what its manifest said into the entries the menu bar draws.
///
/// **No entry carries a shortcut.** Unluminate has a test that no two menu items claim one key equivalent,
/// because two items claiming one chord is a real fault on macOS, and a manifest that could claim
/// `Cmd+S` would be able to break that test from outside the repository. A plugin's command is reachable
/// from its menu, from its own pane and from the command line, which is three ways.
fn plugin_menu(menu: &PluginMenu) -> Menu {
    Menu { name: menu.name.clone(), entries: plugin_entries(&menu.plugin, &menu.items) }
}

fn plugin_entries(
    plugin: &str,
    items: &[crate::services::plugins::MenuItem],
) -> Vec<Entry> {
    use crate::services::plugins::MenuItem;
    items
        .iter()
        .map(|item| match item {
            MenuItem::Separator => Entry::Separator,
            MenuItem::Command { command, label } => Entry::Item {
                name: label.clone(),
                action: Action::PluginCommand {
                    plugin: plugin.to_owned(),
                    command: command.clone(),
                },
                shortcut: None,
                enabled: true,
                checked: false,
                keyboard: false,
            },
            MenuItem::Submenu { label, items } => {
                Entry::Submenu { name: label.clone(), entries: plugin_entries(plugin, items) }
            }
        })
        .collect()
}

/// One plugin's menu as the window holds it: what the manifest said, and which plugin said it.
#[derive(Debug, Clone, PartialEq)]
pub struct PluginMenu {
    /// The `plugin.id`, which is what a chosen entry's action carries.
    pub plugin: String,
    /// `menu.name`.
    pub name: String,
    pub items: Vec<crate::services::plugins::MenuItem>,
}

/// What runs a configuration from the keyboard, per platform.
///
/// The reference editor's own, which is what somebody who has used one will try: `Shift+F10` on Windows and
/// `Ctrl+R` on macOS. They differ because the platforms do, not because Unluminate has an opinion.
pub fn run_shortcut() -> Shortcut {
    if cfg!(target_os = "macos") {
        Shortcut::control(egui::Key::R)
    } else {
        Shortcut { key: egui::Key::F10, command: false, shift: true, alt: false, ctrl: false }
    }
}

/// And what stops one: `Ctrl+F2` on Windows, `Cmd+F2` on macOS. The reference editor's again.
pub fn stop_shortcut() -> Shortcut {
    if cfg!(target_os = "macos") {
        Shortcut::command(egui::Key::F2)
    } else {
        Shortcut::control(egui::Key::F2)
    }
}

/// What starts a configuration under its debugger: `Shift+F9` on Windows, `Ctrl+D` on macOS.
///
/// The reference editor's own, and the ticket says to mimic it. As with [`run_shortcut`] the two platforms
/// differ because the platforms do, not because Unluminate has an opinion.
pub fn debug_shortcut() -> Shortcut {
    if cfg!(target_os = "macos") {
        Shortcut::control(egui::Key::D)
    } else {
        Shortcut { key: egui::Key::F9, command: false, shift: true, alt: false, ctrl: false }
    }
}

/// A bare function key, which is what the four stepping shortcuts are.
///
/// They are the reference editor's exactly — `F9`, `F8`, `F7`, `Shift+F8` — because the ticket says mimic
/// The reference editor and because these are the keys a person's hands already know. A bare key is safe here
/// where it would not be for `Delete`: a function key types nothing, and every one of these entries
/// is **dimmed** unless a session is stopped, so `F8` in an editor with no debugger running matches
/// no entry and costs nothing.
const fn function_key(key: egui::Key) -> Shortcut {
    Shortcut::plain(key)
}

/// The `Run` menu, between `View` and `Git`, because that is where the reference editor has one and where
/// people will look for it.
///
/// The name of the chosen configuration is **live in the entry**, exactly as the Git menu's entries
/// already name the branch, so `Run Dev server` says what pressing it will do rather than leaving
/// it to be guessed from the widget at the other end of the bar.
///
/// Every entry is an [`Action`] dispatched in `UnluminateApp::run_action`, so the widget, the menu, the
/// rail and the keyboard cannot come to disagree about what running means.
pub fn run_menu(state: &MenuState) -> Menu {
    let chosen = state.run_selected.clone();
    let has_chosen = chosen.is_some();
    let named = |verb: &str| match &chosen {
        Some(name) => format!("{verb} {name}"),
        None => verb.to_owned(),
    };
    let mut entries = vec![
        Entry::with_shortcut(&named("Run"), Action::Run(RunAction::Start(None)), run_shortcut())
            .enabled(has_chosen),
    ];
    // Absent rather than dimmed for a file whose language names no command: a control that can
    // never apply to this kind of file is not a control that is unavailable just now.
    if state.run_file_applies {
        entries.push(Entry::item("Run Current File", Action::Run(RunAction::CurrentFile)));
    }
    entries.push(
        Entry::with_shortcut(&named("Stop"), Action::Run(RunAction::Stop(None)), stop_shortcut())
            .enabled(state.run_active),
    );
    entries.push(Entry::item("Rerun", Action::Run(RunAction::Rerun(None))).enabled(has_chosen));
    // The configurations themselves, each as an entry that runs it. Only when there are any: a
    // separator with nothing under it is a line for no reason.
    if !state.run_names.is_empty() {
        entries.push(Entry::Separator);
        for name in &state.run_names {
            entries.push(
                Entry::item(name, Action::Run(RunAction::Start(Some(name.clone()))))
                    .checked(chosen.as_deref() == Some(name.as_str())),
            );
        }
    }
    entries.push(Entry::Separator);
    entries.push(Entry::item("Edit Configurations...", Action::Run(RunAction::Edit)));
    entries.extend(debug_entries(state, &chosen));
    Menu { name: "Run".to_owned(), entries }
}

/// The Run menu's debug half.
///
/// **Absent altogether when the file's language names no debugger**, which is Unluminate's rule for a
/// control that can never apply and is the same answer `Go to Definition` gets for a picture: a
/// stylesheet has nothing to step through and never will, so it gets no half-menu of dimmed entries
/// explaining that.
///
/// Within it, the entries are **dimmed** rather than absent when they cannot apply this instant —
/// stepping while the program is running — because "in a moment" is exactly what dimming means.
fn debug_entries(state: &MenuState, chosen: &Option<String>) -> Vec<Entry> {
    if !state.debug_applies {
        return Vec::new();
    }
    let named = |verb: &str| match chosen {
        Some(name) => format!("{verb} {name}"),
        None => verb.to_owned(),
    };
    let mut entries = vec![
        Entry::Separator,
        Entry::with_shortcut(
            &named("Debug"),
            Action::Debug(DebugAction::Start(None)),
            debug_shortcut(),
        )
        .enabled(chosen.is_some()),
    ];
    if state.run_file_applies {
        entries.push(Entry::item("Debug Current File", Action::Debug(DebugAction::CurrentFile)));
    }
    entries.push(
        Entry::item("Stop Debugging", Action::Debug(DebugAction::Stop)).enabled(state.debug_active),
    );
    entries.push(Entry::Separator);
    entries.push(
        Entry::with_shortcut("Resume", Action::Debug(DebugAction::Resume), function_key(egui::Key::F9))
            .enabled(state.debug_paused),
    );
    entries.push(
        Entry::with_shortcut("Step Over", Action::Debug(DebugAction::StepOver), function_key(egui::Key::F8))
            .enabled(state.debug_paused),
    );
    entries.push(
        Entry::with_shortcut("Step Into", Action::Debug(DebugAction::StepInto), function_key(egui::Key::F7))
            .enabled(state.debug_paused),
    );
    entries.push(
        Entry::with_shortcut(
            "Step Out",
            Action::Debug(DebugAction::StepOut),
            Shortcut { key: egui::Key::F8, command: false, shift: true, alt: false, ctrl: false },
        )
        .enabled(state.debug_paused),
    );
    entries.push(
        Entry::with_shortcut(
            "Run to Cursor",
            Action::Debug(DebugAction::RunToCursor),
            Shortcut { key: egui::Key::F9, command: false, shift: false, alt: true, ctrl: false },
        )
        .enabled(state.debug_paused),
    );
    entries.push(Entry::Separator);
    entries.push(
        Entry::with_shortcut(
            "Toggle Breakpoint",
            Action::Debug(DebugAction::ToggleBreakpoint),
            Shortcut::control(egui::Key::F8),
        ),
    );
    entries.push(
        Entry::with_shortcut(
            "Show Value",
            Action::Debug(DebugAction::ShowValue),
            Shortcut { key: egui::Key::F8, command: true, shift: false, alt: true, ctrl: false },
        )
        .enabled(state.debug_paused),
    );
    entries.push(
        Entry::with_shortcut(
            "Evaluate Expression...",
            Action::Debug(DebugAction::EvaluateExpression),
            Shortcut { key: egui::Key::F8, command: false, shift: false, alt: true, ctrl: false },
        )
        .enabled(state.debug_paused),
    );
    entries
}

/// The Git menu, which holds what the reference capture in `tasks/unluminate-ide-tdd.md` holds.
///
/// The same entries, aimed at one path, are the `Git` submenu on a row in the explorer, built by
/// [`git_submenu`], so the two cannot drift apart.
///
/// `Continue` and `Abort` are there only while a merge or a rebase has stopped on a conflict. An
/// editor that hides a half-finished merge is an editor you cannot finish one in.
pub fn git_menu(state: &MenuState) -> Menu {
    let here = state.in_repository;
    let file = here && state.has_file;
    let mut entries = vec![
        Entry::with_shortcut("Commit...", Action::Git(GitAction::Commit), Shortcut::command(egui::Key::K))
            .enabled(here),
        Entry::with_shortcut(
            "Add",
            Action::Git(GitAction::Add(None)),
            Shortcut { alt: true, ..Shortcut::command(egui::Key::A) },
        )
        .enabled(file),
        Entry::item("Exclude from Version Control", Action::Git(GitAction::Exclude)).enabled(here),
        Entry::Separator,
        Entry::with_shortcut("Show Diff", Action::Git(GitAction::ShowDiff(None)), Shortcut::command(egui::Key::D))
            .enabled(file),
        Entry::item("Compare with Revision...", Action::Git(GitAction::CompareWithRevision(None)))
            .enabled(file),
        Entry::item("Show History", Action::Git(GitAction::ShowHistory(None))).enabled(here),
        Entry::item("Show Current Revision", Action::Git(GitAction::ShowCurrentRevision)).enabled(here),
        Entry::item(
            if state.annotated { "Close Annotations" } else { "Annotate with Git Blame" },
            Action::Git(GitAction::Annotate),
        )
        .enabled(file)
        .checked(state.annotated),
        Entry::Separator,
        Entry::with_shortcut(
            "Rollback...",
            Action::Git(GitAction::Rollback(None)),
            Shortcut { alt: true, ..Shortcut::command(egui::Key::Z) },
        )
        .enabled(file),
        Entry::Separator,
    ];
    if state.unfinished.is_some() {
        entries.push(Entry::item("Continue", Action::Git(GitAction::Continue)));
        entries.push(Entry::item("Abort", Action::Git(GitAction::Abort)));
        entries.push(Entry::Separator);
    }
    entries.extend([
        Entry::with_shortcut("Push...", Action::Git(GitAction::Push), Shortcut::command_shift(egui::Key::K))
            .enabled(here),
        Entry::item("Pull...", Action::Git(GitAction::Pull)).enabled(here),
        Entry::item("Fetch", Action::Git(GitAction::Fetch)).enabled(here),
        Entry::Separator,
        Entry::item("Merge...", Action::Git(GitAction::Merge)).enabled(here),
        Entry::item("Rebase...", Action::Git(GitAction::Rebase)).enabled(here),
        Entry::Separator,
        Entry::with_shortcut(
            "Branches...",
            Action::Git(GitAction::Branches),
            Shortcut::control_shift(egui::Key::Backtick),
        )
        .enabled(here),
        Entry::with_shortcut(
            "New Branch...",
            Action::Git(GitAction::NewBranch),
            Shortcut { alt: true, ..Shortcut::command(egui::Key::N) },
        )
        .enabled(here),
        Entry::item("New Tag...", Action::Git(GitAction::NewTag)).enabled(here),
        Entry::item("Reset HEAD...", Action::Git(GitAction::ResetHead)).enabled(here),
        Entry::Separator,
        Entry::item("Stash Changes...", Action::Git(GitAction::Stash)).enabled(here),
        Entry::item("Unstash Changes...", Action::Git(GitAction::Unstash)).enabled(here),
        Entry::Separator,
        Entry::item("Manage Remotes...", Action::Git(GitAction::Remotes)).enabled(here),
        Entry::item("Clone...", Action::Git(GitAction::Clone)),
    ]);
    Menu { name: "Git".to_owned(), entries }
}

/// The `Git` submenu on a row in the explorer: the same entries, aimed at that row.
pub fn git_submenu(state: &MenuState, path: &std::path::Path) -> Vec<Entry> {
    let here = state.in_repository;
    vec![
        Entry::item("Add", Action::Git(GitAction::Add(Some(path.to_path_buf())))).enabled(here),
        Entry::item("Show Diff", Action::Git(GitAction::ShowDiff(Some(path.to_path_buf())))).enabled(here),
        Entry::item(
            "Compare with Revision...",
            Action::Git(GitAction::CompareWithRevision(Some(path.to_path_buf()))),
        )
        .enabled(here),
        Entry::item("Show History", Action::Git(GitAction::ShowHistory(Some(path.to_path_buf()))))
            .enabled(here),
        Entry::item("Rollback...", Action::Git(GitAction::Rollback(Some(path.to_path_buf()))))
            .enabled(here),
        Entry::Separator,
        Entry::item("Commit...", Action::Git(GitAction::Commit)).enabled(here),
    ]
}

fn unluminate_menu() -> Menu {
    Menu {
        name: "Unluminate".to_owned(),
        // Settings is in two menus on purpose. `tasks/improvements.md` names both `Unluminate -> Settings` and
        // `Edit -> Settings`, and both are where a person looks: the application menu is where macOS keeps a
        // program's own settings, and the Edit menu is where Windows does. The shortcut is on the Edit entry
        // only, because two menu items claiming one key equivalent is a fault on macOS.
        entries: vec![
            Entry::item("About Unluminate", Action::About),
            // **A person asking**, which is the only thing that makes Unluminate send anything on its
            // own behalf. `update.check` is off until somebody turns it on, and this entry works
            // either way. See `services::update`. `task-1804` §6.
            Entry::item("Check for Updates", Action::CheckForUpdates),
            Entry::Separator,
            Entry::item("Settings", Action::Settings),
            Entry::Separator,
            Entry::with_shortcut("Quit Unluminate", Action::Quit, Shortcut::command(egui::Key::Q)),
        ],
    }
}

fn file_menu(state: &MenuState) -> Menu {
    let mut entries = vec![
        Entry::with_shortcut(
            "New Window",
            Action::NewWindow,
            Shortcut::command_shift(egui::Key::N),
        ),
        Entry::Separator,
        Entry::with_shortcut("Open File", Action::OpenFile, Shortcut::command(egui::Key::O)),
        Entry::item("Open Web Address...", Action::OpenWebAddress),
        // Searching the project rather than the disk, which is what `task-1659` asks for and what
        // The reference editor puts on this key. It took the shortcut `Open Folder` used to have, because two
        // menu items claiming one key equivalent is a fault on macOS and there is a test for it;
        // `Open Folder` moved one modifier along, to the key nothing else was using.
        Entry::with_shortcut(
            "Go to File...",
            Action::GoToFile,
            Shortcut::command_shift(egui::Key::O),
        ),
        // A project of its own in a window of its own, which is how a second project is opened without
        // giving up the one that is open.
        Entry::with_shortcut(
            "Open Folder",
            Action::OpenFolder,
            Shortcut { alt: true, ..Shortcut::command(egui::Key::O) },
        ),
        Entry::Submenu { name: "Recent Projects".to_owned(), entries: recent_entries(state) },
        Entry::Separator,
        Entry::with_shortcut("Save", Action::Save, Shortcut::command(egui::Key::S)),
        Entry::with_shortcut("Save As", Action::SaveAs, Shortcut::command_shift(egui::Key::S)),
        Entry::Separator,
        Entry::with_shortcut(
            "Close Window",
            Action::CloseWindow,
            Shortcut::command(egui::Key::W),
        ),
    ];
    entries.retain(|entry| !matches!(entry, Entry::Submenu { entries, .. } if entries.is_empty()));
    Menu { name: "File".to_owned(), entries }
}

/// The recent projects, newest first, with a way to forget them.
///
/// A project the window already has open is still listed, dimmed, because a list that changes length
/// depending on where you are is harder to use than one that does not.
fn recent_entries(state: &MenuState) -> Vec<Entry> {
    if state.recent.is_empty() {
        return Vec::new();
    }
    let mut entries: Vec<Entry> = state
        .recent
        .iter()
        .map(|path| {
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().to_string())
                .unwrap_or_else(|| path.display().to_string());
            Entry::item(&name, Action::OpenRecent(path.clone()))
        })
        .collect();
    entries.push(Entry::Separator);
    entries.push(Entry::item("Forget Recent Projects", Action::ForgetRecent));
    entries
}

/// The three symbol entries, in the order the `Find` menu holds them.
///
/// Built here rather than written into the menu twice, because `components::text_menu` — the
/// editing area's own right click menu — holds the same three above its highlight section, and two
/// lists that agree today are two lists that will not agree next month.
///
/// They are **absent** when the file's language cannot answer them, which is Unluminate's rule for a
/// control that can never apply: the `F` button is not drawn for a `.rs` file either. Dimming means
/// something different and is still right where it was, for a control that could be used in a
/// moment.
pub fn symbol_entries(state: &MenuState) -> Vec<Entry> {
    let mut entries = Vec::new();
    if state.definitions_apply {
        // The reference editor's key. The command key and B is bold everywhere else in Unluminate, and the two can
        // never collide: `services::file_kind` says formatting is for prose and a definition needs
        // a language that has said what one looks like, and no file is both.
        entries.push(Entry::with_shortcut(
            "Go to Definition",
            Action::GoToDefinition,
            Shortcut::command(egui::Key::B),
        ));
    }
    if state.symbols_apply {
        entries.push(Entry::with_shortcut(
            "Find References",
            Action::FindReferences,
            Shortcut { key: egui::Key::F7, command: false, shift: false, alt: true, ctrl: false },
        ));
        entries.push(Entry::with_shortcut(
            "Rename Symbol...",
            Action::RenameSymbol,
            Shortcut { key: egui::Key::F6, command: false, shift: true, alt: false, ctrl: false },
        ));
    }
    entries
}

/// `Complete Word`, when the file's language can offer one.
///
/// **On the Edit menu rather than with the three above**, and `task-1804` is where the two parted
/// company: those three ask *where a name is* and went to the new `Find` menu with the rest of the
/// searching; this one puts a word into the document, which is editing. They were one list while
/// they were all on one menu.
///
/// Absent rather than dimmed when no plugin claims the file, which is `symbol_entries`' rule and the
/// window's: a control that can never apply is not drawn.
pub fn completion_entries(state: &MenuState) -> Vec<Entry> {
    if !state.completion_applies {
        return Vec::new();
    }
    // The reference editor's own binding, and the real control key on both platforms rather than the Apple
    // key on one of them: `Cmd+Space` is Spotlight. macOS may have claimed `Ctrl+Space` for
    // switching input sources, in which case the menu entry is how a person reaches it there —
    // which is a note for the menu test rather than a reason to bind something else here.
    vec![Entry::with_shortcut(
        "Complete Word",
        Action::CompleteWord,
        Shortcut::control(egui::Key::Space),
    )]
}

/// The two entries that walk the places a jump has been from.
///
/// Always there, dimmed when there is nowhere to go: they are about the window's own history rather
/// than about the file, so they do not change shape depending on what is open.
pub fn navigation_entries(state: &MenuState) -> Vec<Entry> {
    vec![
        Entry::with_shortcut(
            "Navigate Back",
            Action::NavigateBack,
            Shortcut { alt: true, ..Shortcut::command(egui::Key::ArrowLeft) },
        )
        .enabled(state.can_go_back),
        Entry::with_shortcut(
            "Navigate Forward",
            Action::NavigateForward,
            Shortcut { alt: true, ..Shortcut::command(egui::Key::ArrowRight) },
        )
        .enabled(state.can_go_forward),
    ]
}

fn edit_menu(state: &MenuState) -> Menu {
    let mut entries = vec![
        Entry::with_shortcut("Undo", Action::Undo, Shortcut::command(egui::Key::Z))
            .enabled(state.can_undo),
        Entry::with_shortcut("Redo", Action::Redo, Shortcut::command_shift(egui::Key::Z))
            .enabled(state.can_redo),
        Entry::Separator,
        Entry::with_shortcut("Cut", Action::Cut, Shortcut::command(egui::Key::X))
            .enabled(state.has_selection)
            .not_from_the_keyboard(),
        Entry::with_shortcut("Copy", Action::Copy, Shortcut::command(egui::Key::C))
            .enabled(state.has_selection)
            .not_from_the_keyboard(),
        Entry::with_shortcut("Paste", Action::Paste, Shortcut::command(egui::Key::V))
            .not_from_the_keyboard(),
        Entry::with_shortcut("Select All", Action::SelectAll, Shortcut::command(egui::Key::A)),
        Entry::Separator,
        Entry::Submenu { name: "Highlight".to_owned(), entries: highlight_menu(state) },
    ];
    let completion = completion_entries(state);
    if !completion.is_empty() {
        entries.push(Entry::Separator);
        entries.extend(completion);
    }
    entries.push(Entry::Separator);
    entries.extend(navigation_entries(state));
    entries.push(Entry::Separator);
    entries.push(Entry::with_shortcut(
        "Settings",
        Action::Settings,
        Shortcut::command(egui::Key::Comma),
    ));
    Menu { name: "Edit".to_owned(), entries }
}

/// `Find`: every way of asking where something is, and the two of changing it.
///
/// **A menu of its own, and it was not one before.** The comment this replaces read *"the reference
/// editor keeps this under `Edit -> Find`, one level further down. Unluminate's Edit menu is eight
/// entries long and a submenu holding one thing is a step for nothing"* -- which was right while
/// `Find in Files` was the only thing in it.
///
/// `task-1804` added Find, Replace, Find Next and Find Previous, and the Edit menu then **ran off
/// the bottom of the window**: `Settings` could not be reached at all in a 740 point tall window, by
/// a person or by a test, and nothing said so. A submenu did not help, because `controls::menu_rows`
/// draws one as a heading with its entries in the same list rather than as a flyout -- which is the
/// right decision for a menu this shape, and is part of why the Edit menu was already at the limit.
/// `menu_rows` does scroll a menu that will not fit, so nothing was ever *lost*; a menu somebody has
/// to scroll is simply not an answer.
///
/// So the group has its own word in the bar, which is what it is: seven ways of finding something,
/// in a product whose largest single gap was that `Ctrl+F` did nothing. The order is the order they
/// are reached in -- what is in front of you first, then the project, then the symbol entries, which
/// are two more ways of asking where something is and one of changing it everywhere it was found.
fn find_menu(state: &MenuState) -> Menu {
    let mut entries = vec![
        Entry::with_shortcut("Find...", Action::Find, Shortcut::command(egui::Key::F)),
        Entry::with_shortcut("Replace...", Action::Replace, Shortcut::command(egui::Key::H)),
        Entry::with_shortcut("Find Next", Action::FindNext, Shortcut::command(egui::Key::G))
            .enabled(state.finding),
        Entry::with_shortcut(
            "Find Previous",
            Action::FindPrevious,
            Shortcut::command_shift(egui::Key::G),
        )
        .enabled(state.finding),
        Entry::Separator,
        Entry::with_shortcut(
            "Find in Files...",
            Action::FindInFiles,
            Shortcut::command_shift(egui::Key::F),
        ),
    ];
    let symbols = symbol_entries(state);
    if !symbols.is_empty() {
        entries.push(Entry::Separator);
        entries.extend(symbols);
    }
    Menu { name: "Find".to_owned(), entries }
}

fn view_menu(state: &MenuState) -> Menu {
    let explorer = if state.explorer_visible { "Hide Explorer" } else { "Show Explorer" };
    // The three entries are named after what the open file's preview actually is, so a `.mmd` file
    // reads `Raw Mermaid` and `Mermaid Diagram` rather than being offered a Markdown preview it has
    // not got. The buttons in the title bar take the same answer from the same function.
    let diagram = state.preview_kind == crate::services::file_kind::PreviewKind::Mermaid;
    let (raw, rendered) = if diagram {
        ("Raw Mermaid", "Mermaid Diagram")
    } else {
        ("Raw Markdown", "Markdown Preview")
    };
    Menu {
        name: "View".to_owned(),
        entries: vec![
            Entry::with_shortcut(
                raw,
                Action::SetViewMode(ViewMode::Raw),
                Shortcut::command(egui::Key::Num1),
            )
            .checked(state.view_mode == ViewMode::Raw)
            .enabled(state.can_preview),
            Entry::with_shortcut(
                "Side by Side",
                Action::SetViewMode(ViewMode::SideBySide),
                Shortcut::command(egui::Key::Num2),
            )
            .checked(state.view_mode == ViewMode::SideBySide)
            .enabled(state.can_preview),
            Entry::with_shortcut(
                rendered,
                Action::SetViewMode(ViewMode::Preview),
                Shortcut::command(egui::Key::Num3),
            )
            .checked(state.view_mode == ViewMode::Preview)
            .enabled(state.can_preview),
            Entry::Separator,
            Entry::with_shortcut(
                explorer,
                Action::ToggleExplorer,
                Shortcut::command(egui::Key::Num0),
            ),
            // No chord. Command and zero belongs to the explorer, and `Reset Font Size` already goes without one
            // for the same reason: the obvious key is taken.
            Entry::item(
                if state.editor_visible { "Hide Editor" } else { "Show Editor" },
                Action::ToggleEditor,
            ),
            Entry::item(
                if state.line_numbers { "Hide Line Numbers" } else { "Show Line Numbers" },
                Action::ToggleLineNumbers,
            ),
            // Two presses at the top of a pane do the same thing, which is what `task-1771` asks for; this
            // is the entry that gives it a name, a key and - because `actions::menus` is walked to build
            // the command line - an agent. Escape is what puts it back, and is not a shortcut here because
            // Escape already means several things and a menu equivalent would claim it from all of them.
            Entry::with_shortcut(
                if state.maximised { "Restore Pane" } else { "Maximise Pane" },
                Action::ToggleMaximisedPane,
                Shortcut::command_shift(egui::Key::M),
            ),
            Entry::Separator,
            // The editor's font size, on the keyboard as it is in every other editor. They are menu
            // entries rather than keys watched for in the editing area for the reason the whole menu
            // exists: on macOS a shortcut on a menu item is a key equivalent and AppKit hands the
            // press to the menu before the window sees it, so a key read in `editor_view` would work
            // on Windows and be dead on macOS. There is no shortcut on `Reset Font Size`, because
            // the obvious one is command and zero and `Show Explorer` already has it.
            Entry::with_shortcut(
                "Increase Font Size",
                Action::ChangeFontSize { larger: true },
                Shortcut::command(egui::Key::Plus),
            ),
            Entry::with_shortcut(
                "Decrease Font Size",
                Action::ChangeFontSize { larger: false },
                Shortcut::command(egui::Key::Minus),
            ),
            Entry::item("Reset Font Size", Action::ResetFontSize),
            Entry::Separator,
            // Ctrl+F4 rather than the Apple key and W, which `Close Window` already claims. Two menu
            // items claiming one key equivalent is a fault on macOS, and there is a test for it.
            Entry::with_shortcut("Close Tab", Action::CloseTab, Shortcut::command(egui::Key::F4))
                .enabled(state.open_files > 1),
            Entry::with_shortcut("Next Tab", Action::NextTab, Shortcut::control(egui::Key::Tab))
                .enabled(state.tabs_in_pane > 1),
            Entry::with_shortcut(
                "Previous Tab",
                Action::PreviousTab,
                Shortcut::control_shift(egui::Key::Tab),
            )
            .enabled(state.tabs_in_pane > 1),
            Entry::Separator,
            Entry::item("Select Opened File", Action::SelectOpenFile),
            // The splits are a menu inside the menu, as Recent Projects is. Seven more rows in View
            // itself would have made it twenty two rows long and taller than a small window; and a
            // tab's own right click menu is where a person reaches for them anyway. They are on a
            // real menu at all so that `unluminate-cli action list` finds them, because that list is built
            // by walking the menus and a context menu is not one of them.
            Entry::Submenu { name: "Split".to_owned(), entries: split_menu(state) },
            Entry::Submenu { name: "Folding".to_owned(), entries: folding_menu(state) },
            Entry::Separator,
            Entry::with_shortcut(
                "Terminal",
                Action::ToggleTerminal,
                Shortcut::control(egui::Key::Backtick),
            )
            .checked(state.terminal_visible),
            Entry::item(
                if state.run_tile_visible { "Hide Run Tile" } else { "Run Tile" },
                Action::ToggleRunTile,
            )
            .checked(state.run_tile_visible),
            Entry::item(
                if state.debug_tile_visible { "Hide Debug Tile" } else { "Debug Tile" },
                Action::ToggleDebugTile,
            )
            .checked(state.debug_tile_visible),
            // The one row of `task-1697` that is worth a place in the bar. Moving a panel is a drag,
            // or its own right click menu, or `unluminate-cli panel dock`; putting them all back is the
            // thing somebody looks for in a menu, because by then they have lost one.
            Entry::item("Reset Panel Layout", Action::ResetPanelLayout),
            Entry::item("New Terminal Tab", Action::NewTerminalTab),
            Entry::item("Close Terminal Tab", Action::CloseTerminalTab)
                .enabled(state.terminal_tabs > 0),
            Entry::item("Rename Terminal Tab...", Action::RenameTerminalTab)
                .enabled(state.terminal_tabs > 0),
        ],
    }
}

/// The entries that split the editing area into panes, which are the same wherever they are shown:
/// the `Split` menu inside `View`, and the lower half of a tab's own right click menu.
fn split_menu(state: &MenuState) -> Vec<Entry> {
    vec![
        Entry::item("Split Right", Action::SplitRight),
        Entry::item("Move Right", Action::MoveTabRight).enabled(state.pane + 1 < state.panes),
        Entry::item("Move Left", Action::MoveTabLeft).enabled(state.pane > 0),
        Entry::item("Next Pane", Action::NextPane).enabled(state.panes > 1),
        Entry::item("Previous Pane", Action::PreviousPane).enabled(state.panes > 1),
        Entry::Separator,
        Entry::item("Unsplit", Action::Unsplit).enabled(state.panes > 1),
        Entry::item("Unsplit All", Action::UnsplitAll).enabled(state.panes > 1),
    ]
}

/// What a tab's right click menu holds.
///
/// Every entry is about **the tab that is showing**, which is what makes them parameterless actions
/// the View menu, the keyboard and `unluminate-cli action run` can all ask for without inventing a way to
/// name a tab. Right clicking a tab therefore shows it first — the editing area's own menu already
/// sets that precedent, putting the caret where the pointer is before opening.
///
/// The same entries are on the View menu, so a person who does not think to right click a tab can
/// still find them, and so `unluminate-cli action list` — which is built by walking the real menus — lists
/// them.
pub fn tab_menu(state: &MenuState) -> Vec<Entry> {
    let mut entries = vec![
        Entry::item("Close", Action::CloseTab).enabled(state.open_files > 1),
        Entry::item("Select Opened File", Action::SelectOpenFile),
        Entry::Separator,
    ];
    entries.extend(split_menu(state));
    entries
}

/// What a terminal tab's right click menu holds.
///
/// Every entry is about **the terminal tab that is showing**, exactly as a file tab's menu is, which
/// is what makes them parameterless actions the View menu, the keyboard and `unluminate-cli action run`
/// can all ask for without inventing a way to name a tab. Right clicking a tab therefore shows it
/// first. It takes no [`MenuState`] because it can only be opened by right clicking a tab, so there
/// is always one to rename and one to close.
///
/// The same three entries are on the View menu, so a person who does not think to right click a tab
/// can still find them, and so `unluminate-cli action list` — which is built by walking the real menus —
/// lists them.
pub fn terminal_tab_menu() -> Vec<Entry> {
    vec![
        Entry::item("Rename...", Action::RenameTerminalTab),
        Entry::item("Close", Action::CloseTerminalTab),
        Entry::Separator,
        Entry::item("New Terminal Tab", Action::NewTerminalTab),
    ]
}

/// What a panel's own right click menu holds — `task-1697`.
///
/// Opened from the panel's header, which is also the handle it is dragged by, and from its button in
/// the rail. Every entry names the panel it was opened on, so this takes one rather than acting on
/// "the panel that is showing": all four can be showing at once.
///
/// The four `Move to` rows are **not** put on the `View` menu, and that is a decision rather than an
/// omission. A submenu here is drawn *inline*, so four panels' four sides would be twenty rows added
/// to a menu that already has thirty-odd and already scrolls — which is the exact fault `task-1686`
/// records for the Edit menu, where three more rows pushed `Settings` off the bottom of the window.
/// What does go on `View` is the one row worth a menu of its own, `Reset Panel Layout`, so there is
/// always a way back that does not need the panel you have lost to be found first. The rest is
/// `unluminate-cli panel`, which is a whole area of the catalogue and is what an agent reads.
pub fn panel_menu(state: &MenuState, panel: crate::app::dock::Panel) -> Vec<Entry> {
    use crate::app::dock::Side;
    let mut entries: Vec<Entry> = Side::ALL
        .into_iter()
        .map(|side| {
            Entry::item(&format!("Move to {}", side.label()), Action::Dock { panel, side })
                .checked(state.dock.side_of(panel) == side)
        })
        .collect();
    entries.push(Entry::Separator);
    entries.push(Entry::item("Reset Panel Layout", Action::ResetPanelLayout));
    entries
}

/// Whether the explorer's menu was opened over a row or over the empty space below the rows.
///
/// `task-1693` asks that a right click anywhere in the panel open the same menu, "but options that
/// don't apply, like rename, etc should be greyed out". So there is one menu and one function, and
/// this is the one thing that changes between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Aim {
    /// The pointer was over a file or a folder, or over the project's own name.
    AtARow,
    /// The pointer was over the empty space below the rows, which is the project folder.
    AtEmptySpace,
}

/// What the explorer's right click menu holds, for the row that was clicked.
///
/// `directory` says whether the row is a folder, because a new file goes *in* a folder and *beside*
/// a file, and there is nothing to reload from disk about a file that is not open.
///
/// `Delete` is here because `task-1681` asked for it, one row under `Rename...` and with the key
/// beside it. It is marked as not coming from the keyboard: the watcher behind the menu bar must
/// never fire it, because `Delete` in the editing area means "take away the letter in front of the
/// caret". The explorer reads the key itself, and only while it has the keyboard.
pub fn explorer_menu(
    path: &std::path::Path,
    directory: bool,
    can_paste: bool,
    aimed: Aim,
) -> Vec<Entry> {
    let folder = if directory {
        path.to_path_buf()
    } else {
        path.parent().map(std::path::Path::to_path_buf).unwrap_or_else(|| path.to_path_buf())
    };
    // A right click in the empty space below the rows is about the project folder, so everything
    // that is about the *folder* is live and everything that is about a particular file is dimmed —
    // which is what `task-1693` asks for in as many words. Dimmed rather than absent, deliberately:
    // absent is Unluminate's rule for a control that can never apply, and every one of these is live the
    // instant the pointer is over a row.
    let on_a_row = aimed == Aim::AtARow;
    let mut entries = vec![
        Entry::Submenu {
            name: "New".to_owned(),
            entries: vec![
                Entry::item("File", Action::NewFile(folder.clone())),
                Entry::item("Folder", Action::NewFolder(folder.clone())),
            ],
        },
        Entry::Separator,
        Entry::with_shortcut("Cut", Action::CutPath(path.to_path_buf()), Shortcut::command(egui::Key::X))
            .enabled(on_a_row)
            .not_from_the_keyboard(),
        Entry::with_shortcut("Copy", Action::CopyPath(path.to_path_buf()), Shortcut::command(egui::Key::C))
            .enabled(on_a_row)
            .not_from_the_keyboard(),
        Entry::item("Copy Path", Action::CopyPathReference(path.to_path_buf())).enabled(on_a_row),
        Entry::with_shortcut("Paste", Action::PasteInto(folder), Shortcut::command(egui::Key::V))
            .enabled(can_paste)
            .not_from_the_keyboard(),
        Entry::Separator,
        Entry::item("Rename...", Action::RenamePath(path.to_path_buf())).enabled(on_a_row),
        Entry::with_shortcut(
            "Delete",
            Action::DeletePath(path.to_path_buf()),
            Shortcut::plain(egui::Key::Delete),
        )
        .enabled(on_a_row)
        .not_from_the_keyboard(),
        Entry::Separator,
        Entry::item(crate::services::launcher::file_manager_name(), Action::RevealPath(path.to_path_buf())),
        Entry::item("Reload from Disk", Action::ReloadPath(path.to_path_buf())),
    ];
    if on_a_row && !directory && crate::services::file_kind::is_html(path) {
        entries.insert(1, Entry::Submenu {
            name: "Open in Browser".to_owned(),
            entries: vec![Entry::item("Tab", Action::OpenInBrowser(path.to_path_buf()))],
        });
    }
    entries
}

/// The explorer's menu with the `Git` submenu on the end, which is what is actually shown.
///
/// Split from [`explorer_menu`] so the entries that have nothing to do with git can be tested
/// without a repository behind them.
pub fn explorer_menu_with_git(
    state: &MenuState,
    path: &std::path::Path,
    directory: bool,
    can_paste: bool,
    aimed: Aim,
) -> Vec<Entry> {
    let mut entries = explorer_menu(path, directory, can_paste, aimed);
    entries.push(Entry::Separator);
    let mut git = git_submenu(state, path);
    if aimed == Aim::AtEmptySpace {
        // Every entry in it is about the file that was clicked, and nothing was.
        git = git.into_iter().map(|entry| entry.enabled(false)).collect();
    }
    entries.push(Entry::Submenu { name: "Git".to_owned(), entries: git });
    entries
}

/// What `Edit -> Highlight` holds, and what the colour rows of the editor's own right click menu
/// are built from.
///
/// It is on a menu at all so that every one of these has an [`Action`] with a name, which is what
/// puts it on the command line the day it exists — `unluminate-cli action list` is built by walking the
/// real menus. The four blocks drawn in the right click menu are these four entries wearing a
/// different shape.
pub fn highlight_menu(state: &MenuState) -> Vec<Entry> {
    let mut entries: Vec<Entry> = HighlightColor::ALL
        .iter()
        .map(|colour| {
            Entry::item(colour.label(), Action::Highlight(*colour)).enabled(state.has_selection)
        })
        .collect();
    entries.push(Entry::Separator);
    entries.extend(clear_highlight_menu(state));
    entries
}

/// The `Folding` submenu of `View`, and the entries the gutter's menu takes from it.
///
/// Absent altogether for a file that cannot fold, which is Unluminate's rule for a control that can never
/// apply — the `F` button is not drawn for a `.rs` file either. Dimmed rather than absent when there
/// is simply nothing to collapse just now, which is the other half of the same rule.
pub fn folding_menu(state: &MenuState) -> Vec<Entry> {
    if !state.folding_applies {
        return Vec::new();
    }
    FoldAction::ALL
        .iter()
        .map(|what| {
            let enabled = match what {
                FoldAction::Toggle
                | FoldAction::All
                | FoldAction::Others
                | FoldAction::CollapseRecursively => state.foldable > 0,
                FoldAction::None_ | FoldAction::ExpandRecursively => state.folded > 0,
            };
            Entry::with_shortcut(what.label(), Action::Fold(*what), what.shortcut()).enabled(enabled)
        })
        .collect()
}

/// The two entries the editing area's own right click menu holds: the ticket's own words.
///
/// `Collapse All But Highlighted` and `Expand All` — the pair somebody who has just marked four
/// passages is reaching for. The other two are on the View menu and on the gutter's menu, which is
/// where a person who has just right clicked the arrows is asking about folding.
///
/// **Not added to `clear_highlight_menu`**, tempting as that was: the Edit menu's `Highlight`
/// submenu is drawn *inline* rather than as a flyout — `controls::menu_rows` puts a heading and
/// then the rows, indented — so three more rows there make the whole Edit menu taller, and a menu
/// past the height of the window turns into a scrolling one whose last entry is `Settings`. Two
/// rows on a right click menu must not be able to move `Settings` out of reach.
pub fn folding_here_menu(state: &MenuState) -> Vec<Entry> {
    if !state.folding_applies {
        return Vec::new();
    }
    vec![
        Entry::with_shortcut(
            FoldAction::Others.label(),
            Action::Fold(FoldAction::Others),
            FoldAction::Others.shortcut(),
        )
        .enabled(state.foldable > 0),
        Entry::with_shortcut(
            FoldAction::None_.label(),
            Action::Fold(FoldAction::None_),
            FoldAction::None_.shortcut(),
        )
        .enabled(state.folded > 0),
    ]
}

/// The two ways of taking a mark away, which the Edit menu and the editing area's own menu both
/// hold. Split out because the right click menu draws the four colours as blocks rather than as
/// rows, so it needs these two on their own.
pub fn clear_highlight_menu(state: &MenuState) -> Vec<Entry> {
    vec![
        Entry::item("Clear Highlight", Action::ClearHighlight).enabled(state.on_a_highlight),
        Entry::item("Clear All Highlights", Action::ClearHighlights).enabled(state.highlights > 0),
    ]
}

/// The rows above the colours in the editing area's own right click menu.
///
/// The clipboard and Select All: what every editor's right click menu holds, and what a person
/// reaches for when they have just selected something. They are the same [`Action`]s the Edit menu
/// uses, so there is one arm in `run_action` for each rather than two.
pub fn text_menu(state: &MenuState) -> Vec<Entry> {
    let mut entries = vec![
        Entry::with_shortcut("Cut", Action::Cut, Shortcut::command(egui::Key::X))
            .enabled(state.has_selection),
        Entry::with_shortcut("Copy", Action::Copy, Shortcut::command(egui::Key::C))
            .enabled(state.has_selection),
        Entry::with_shortcut("Paste", Action::Paste, Shortcut::command(egui::Key::V)),
        Entry::with_shortcut("Select All", Action::SelectAll, Shortcut::command(egui::Key::A)),
    ];
    // The same three the Edit menu holds, above the highlight section, from the same function — so
    // a right click and the menu bar cannot come to different answers about whether a file has a
    // definition in it. They are absent here for exactly the files they are absent there for.
    let symbols = symbol_entries(state);
    if !symbols.is_empty() {
        entries.push(Entry::Separator);
        entries.extend(symbols);
    }
    entries
}

/// What the gutter's own right click menu holds.
///
/// Built here rather than in the component for the same reason the bar's menus are: an entry is an
/// [`Action`] with one arm in `run_action`, so the gutter's `Show Line Numbers` and the View menu's
/// are the same thing rather than two things that agree today.
pub fn gutter_menu(state: &MenuState) -> Vec<Entry> {
    let mut entries = vec![
        Entry::item(
            if state.annotated { "Close Annotations" } else { "Annotate with Git Blame" },
            Action::Git(GitAction::Annotate),
        )
        .enabled(state.in_repository && state.has_file)
        .checked(state.annotated),
        Entry::Separator,
        Entry::item(
            if state.line_numbers { "Hide Line Numbers" } else { "Show Line Numbers" },
            Action::ToggleLineNumbers,
        ),
    ];
    // The dot is drawn in this strip, so this is where somebody asking about it right clicks. The
    // entries are about the **row under the pointer** rather than about the caret, which is the rule
    // the text menu and the terminal tab menu already follow; `UnluminateApp::gutter_menu_line` is what
    // remembers which row that was.
    if state.debug_applies {
        entries.push(Entry::Separator);
        match state.on_a_breakpoint {
            true => {
                entries.push(Entry::item(
                    "Edit Breakpoint...",
                    Action::Debug(DebugAction::EditBreakpoint),
                ));
                entries.push(Entry::item(
                    match state.breakpoint_enabled {
                        true => "Disable Breakpoint",
                        false => "Enable Breakpoint",
                    },
                    Action::Debug(DebugAction::ToggleBreakpointEnabled),
                ));
                entries.push(Entry::item(
                    "Remove Breakpoint",
                    Action::Debug(DebugAction::ToggleBreakpoint),
                ));
            }
            false => {
                entries.push(Entry::item(
                    "Set Breakpoint",
                    Action::Debug(DebugAction::ToggleBreakpoint),
                ));
                entries.push(Entry::item(
                    "Add Conditional Breakpoint...",
                    Action::Debug(DebugAction::EditBreakpoint),
                ));
            }
        }
    }
    // The arrows are drawn in this strip too, so the same is true of them.
    let folding = folding_menu(state);
    if !folding.is_empty() {
        entries.push(Entry::Separator);
        entries.extend(folding);
    }
    entries
}

/// The action a key press asks for, if any menu entry claims it.
///
/// Entries marked [`Entry::not_from_the_keyboard`] are skipped, because something else delivers those.
pub fn action_for_key(
    state: &MenuState,
    key: egui::Key,
    modifiers: &egui::Modifiers,
) -> Option<Action> {
    fn search(entries: &[Entry], key: egui::Key, modifiers: &egui::Modifiers) -> Option<Action> {
        for entry in entries {
            match entry {
                Entry::Item { action, shortcut: Some(shortcut), enabled, keyboard: true, .. }
                    if *enabled && shortcut.matches(key, modifiers) =>
                {
                    return Some(action.clone());
                }
                Entry::Submenu { entries, .. } => {
                    if let Some(found) = search(entries, key, modifiers) {
                        return Some(found);
                    }
                }
                _ => {}
            }
        }
        None
    }
    menus(state).iter().find_map(|menu| search(&menu.entries, key, modifiers))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(entries: &[Entry]) -> Vec<String> {
        entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item { name, .. } => Some(name.clone()),
                Entry::Submenu { name, .. } => Some(name.clone()),
                Entry::Separator => None,
            })
            .collect()
    }

    fn find(menus: &[Menu], menu: &str) -> Menu {
        menus
            .iter()
            .find(|found| found.name == menu)
            .cloned()
            .unwrap_or_else(|| panic!("there should be a {menu} menu"))
    }

    #[test]
    fn unluminate_comes_first_and_then_file_edit_find_view_and_git() {
        let bar = menus(&MenuState::default());
        let order: Vec<&str> = bar.iter().map(|menu| menu.name.as_str()).collect();
        // `Find` sits after `Edit`, which is where every editor puts it and where a hand looking
        // for it goes. `task-1804` -- see `find_menu` for why it is a menu rather than a submenu.
        assert_eq!(order, vec!["Unluminate", "File", "Edit", "Find", "View", "Run", "Git"]);
    }

    #[test]
    fn every_git_entry_is_dimmed_outside_a_repository_rather_than_missing() {
        // A menu that changes shape depending on where you are is harder to use than one that does
        // not, so the entries stay and are dimmed. `Clone...` is the exception: it is how you come
        // to have a repository at all.
        let outside = git_menu(&MenuState::default());
        let usable: Vec<String> = outside
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item { name, enabled: true, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(usable, vec!["Clone...".to_owned()]);
        assert!(
            names(&outside.entries).len() > 15,
            "the entries are still there, just dimmed: {:?}",
            names(&outside.entries)
        );
    }

    #[test]
    fn continue_and_abort_are_there_only_while_something_is_half_finished() {
        let settled = MenuState { in_repository: true, ..MenuState::default() };
        assert!(!names(&git_menu(&settled).entries).contains(&"Abort".to_owned()));
        let stuck = MenuState { unfinished: Some("Merging"), ..settled.clone() };
        let entries = names(&git_menu(&stuck).entries);
        assert!(entries.contains(&"Continue".to_owned()));
        assert!(entries.contains(&"Abort".to_owned()));
    }

    #[test]
    fn the_git_entries_on_a_row_in_the_explorer_are_aimed_at_that_row() {
        let state = MenuState { in_repository: true, ..MenuState::default() };
        let path = PathBuf::from("/project/notes.md");
        let entries = explorer_menu_with_git(&state, &path, false, false, Aim::AtARow);
        let git = entries
            .iter()
            .find_map(|entry| match entry {
                Entry::Submenu { name, entries } if name == "Git" => Some(entries.clone()),
                _ => None,
            })
            .expect("a Git submenu");
        let aimed = git.iter().any(|entry| {
            matches!(entry, Entry::Item { action: Action::Git(GitAction::ShowDiff(Some(at))), .. } if *at == path)
        });
        assert!(aimed, "Show Diff on a row is about that row's file");
    }

    /// `task-1693`: one menu, opened from a row or from the empty space below the rows, with the
    /// entries that are about a particular file dimmed in the second case rather than taken away.
    #[test]
    fn the_menu_from_the_empty_space_dims_what_is_about_a_file() {
        let root = std::path::PathBuf::from("/project");
        let live = |entries: &[Entry], name: &str| {
            entries.iter().any(|entry| {
                matches!(entry, Entry::Item { name: found, enabled, .. } if found == name && *enabled)
            })
        };
        let on_a_row = explorer_menu(&root.join("readme.md"), false, true, Aim::AtARow);
        let empty = explorer_menu(&root, true, true, Aim::AtEmptySpace);
        assert_eq!(
            on_a_row.len(),
            empty.len(),
            "it is the same menu, so a person finds the same rows in the same places"
        );
        for entry in ["Cut", "Copy", "Copy Path", "Rename...", "Delete"] {
            assert!(live(&on_a_row, entry), "{entry} is live over a row");
            assert!(!live(&empty, entry), "{entry} is dimmed over the empty space");
        }
        for entry in ["Paste", "Reload from Disk"] {
            assert!(live(&empty, entry), "{entry} is about the folder, so it stays live");
        }
        // And the one thing somebody who right clicked the empty space came for.
        let new = empty.iter().find_map(|entry| match entry {
            Entry::Submenu { name, entries } if name == "New" => Some(entries.clone()),
            _ => None,
        });
        let new = new.expect("a New submenu");
        assert!(live(&new, "File"));
        assert!(live(&new, "Folder"));
    }

    /// The `New` submenu makes both kinds of thing, which `task-1693` asked for: there was no way to
    /// make a folder from the explorer at all.
    #[test]
    fn the_new_submenu_makes_a_folder_as_well_as_a_file() {
        let folder = std::path::PathBuf::from("/project/chapters");
        let entries = explorer_menu(&folder, true, false, Aim::AtARow);
        let new = entries
            .iter()
            .find_map(|entry| match entry {
                Entry::Submenu { name, entries } if name == "New" => Some(entries.clone()),
                _ => None,
            })
            .expect("a New submenu");
        let makes = |action: &Action| matches!(action, Action::NewFolder(at) if *at == folder);
        assert!(
            new.iter().any(|entry| matches!(entry, Entry::Item { action, .. } if makes(action))),
            "Folder makes one inside the folder that was clicked"
        );
    }

    /// `task-1756`: only HTML rows offer the rendered-tab action, and it keeps the selected path.
    #[test]
    fn html_rows_offer_open_in_browser_as_a_tab() {
        let html = PathBuf::from("/project/site/index.html");
        let entries = explorer_menu(&html, false, false, Aim::AtARow);
        let browser = entries.iter().find_map(|entry| match entry {
            Entry::Submenu { name, entries } if name == "Open in Browser" => Some(entries),
            _ => None,
        }).expect("an Open in Browser submenu");
        assert!(browser.iter().any(|entry| {
            matches!(entry, Entry::Item { name, action: Action::OpenInBrowser(path), .. } if name == "Tab" && path == &html)
        }));
        let markdown = explorer_menu(std::path::Path::new("/project/readme.md"), false, false, Aim::AtARow);
        assert!(!markdown.iter().any(|entry| matches!(entry, Entry::Submenu { name, .. } if name == "Open in Browser")));
    }

    #[test]
    fn the_run_menu_names_the_configuration_that_is_chosen() {
        // Live in the entry, as the Git menu's entries already name the branch: `Run Dev server`
        // says what pressing it will do rather than leaving it to be read off the widget at the
        // other end of the bar.
        let state = MenuState {
            run_selected: Some("Dev server".to_owned()),
            run_names: vec!["Dev server".to_owned(), "cargo run".to_owned()],
            ..MenuState::default()
        };
        let run = names(&run_menu(&state).entries);
        assert_eq!(run[0], "Run Dev server");
        assert_eq!(run[1], "Stop Dev server");
        assert_eq!(run[2], "Rerun");
        assert!(run.contains(&"cargo run".to_owned()), "and each configuration is an entry: {run:?}");
        assert_eq!(run.last(), Some(&"Edit Configurations...".to_owned()));
    }

    /// The debug half is **absent** for a language that names no debugger, which is Unluminate's rule for
    /// a control that can never apply — the same answer `Go to Definition` gets for a picture.
    #[test]
    fn a_file_whose_language_names_no_debugger_gets_no_debug_entries_at_all() {
        let state = MenuState {
            run_selected: Some("Dev server".to_owned()),
            debug_applies: false,
            ..MenuState::default()
        };
        let run = names(&run_menu(&state).entries);
        assert!(
            !run.iter().any(|name| name.contains("Debug") || name.contains("Step")),
            "a stylesheet has nothing to step through: {run:?}"
        );
        assert_eq!(run.last(), Some(&"Edit Configurations...".to_owned()));
    }

    /// And within it the entries are **dimmed** rather than absent while they cannot apply this
    /// instant, because "in a moment" is exactly what dimming means.
    #[test]
    fn the_stepping_entries_are_dimmed_until_the_program_is_stopped() {
        let running = MenuState {
            run_selected: Some("Dev server".to_owned()),
            debug_applies: true,
            debug_active: true,
            debug_paused: false,
            ..MenuState::default()
        };
        let enabled = |state: &MenuState, wanted: &str| {
            run_menu(state)
                .entries
                .iter()
                .find_map(|entry| match entry {
                    Entry::Item { name, enabled, .. } if name == wanted => Some(*enabled),
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{wanted} should be on the menu"))
        };
        for entry in ["Resume", "Step Over", "Step Into", "Step Out", "Run to Cursor"] {
            assert!(!enabled(&running, entry), "{entry} cannot apply while the program runs");
        }
        assert!(enabled(&running, "Stop Debugging"), "stopping always can");
        // Toggling a breakpoint needs no session at all: a breakpoint set now is one the next
        // session will be told about.
        assert!(enabled(&running, "Toggle Breakpoint"));

        let paused = MenuState { debug_paused: true, ..running.clone() };
        for entry in ["Resume", "Step Over", "Step Into", "Step Out", "Run to Cursor"] {
            assert!(enabled(&paused, entry), "{entry} applies once it has stopped");
        }
        assert!(enabled(&paused, "Evaluate Expression..."));
    }

    /// The reference editor's keys, kept exactly, because the ticket says to mimic it and these are the keys
    /// a person's hands already know.
    #[test]
    fn the_stepping_keys_are_the_reference_editors_own() {
        let state = MenuState {
            run_selected: Some("Dev server".to_owned()),
            debug_applies: true,
            debug_paused: true,
            ..MenuState::default()
        };
        let shortcut = |wanted: &str| {
            run_menu(&state)
                .entries
                .iter()
                .find_map(|entry| match entry {
                    Entry::Item { name, shortcut, .. } if name == wanted => *shortcut,
                    _ => None,
                })
                .unwrap_or_else(|| panic!("{wanted} should have a key"))
        };
        assert_eq!(shortcut("Resume").key, egui::Key::F9);
        assert!(!shortcut("Resume").shift);
        assert_eq!(shortcut("Step Over").key, egui::Key::F8);
        assert!(!shortcut("Step Over").shift);
        assert_eq!(shortcut("Step Into").key, egui::Key::F7);
        assert_eq!(shortcut("Step Out").key, egui::Key::F8);
        assert!(shortcut("Step Out").shift, "which is what keeps it apart from Step Over");
        assert!(shortcut("Run to Cursor").alt);
        assert_eq!(shortcut("Run to Cursor").key, egui::Key::F9);
        assert_eq!(shortcut("Toggle Breakpoint").key, egui::Key::F8);
        assert!(shortcut("Evaluate Expression...").alt);
    }

    /// The gutter's own menu is about the row under the pointer, and it offers the opposite thing
    /// depending on what is there — which is what makes one entry serve both.
    #[test]
    fn the_gutters_menu_offers_to_set_a_breakpoint_or_to_remove_one() {
        let empty = MenuState { debug_applies: true, ..MenuState::default() };
        let rows = names(&gutter_menu(&empty));
        assert!(rows.contains(&"Set Breakpoint".to_owned()), "{rows:?}");
        assert!(rows.contains(&"Add Conditional Breakpoint...".to_owned()));

        let on_one =
            MenuState { on_a_breakpoint: true, breakpoint_enabled: true, ..empty.clone() };
        let rows = names(&gutter_menu(&on_one));
        assert!(rows.contains(&"Remove Breakpoint".to_owned()), "{rows:?}");
        assert!(rows.contains(&"Disable Breakpoint".to_owned()));
        assert!(rows.contains(&"Edit Breakpoint...".to_owned()));

        let off = MenuState { breakpoint_enabled: false, ..on_one };
        assert!(names(&gutter_menu(&off)).contains(&"Enable Breakpoint".to_owned()));

        // And nothing at all for a file that cannot be debugged, which is the same rule again.
        let css = MenuState { debug_applies: false, ..MenuState::default() };
        let rows = names(&gutter_menu(&css));
        assert!(!rows.iter().any(|name| name.contains("Breakpoint")), "{rows:?}");
    }

    #[test]
    fn with_nothing_chosen_the_run_entries_are_dimmed_rather_than_missing() {
        // A menu that changes shape depending on what has been chosen is harder to use than one
        // that does not, which is the rule the Git menu already keeps outside a repository.
        let menu = run_menu(&MenuState::default());
        let usable: Vec<String> = menu
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item { name, enabled: true, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(usable, vec!["Edit Configurations...".to_owned()]);
        assert_eq!(names(&menu.entries)[0], "Run", "with no name after it, because none is chosen");
    }

    #[test]
    fn stop_is_dimmed_until_something_is_running() {
        let chosen =
            MenuState { run_selected: Some("Dev server".to_owned()), ..MenuState::default() };
        let stop = |state: &MenuState| {
            run_menu(state)
                .entries
                .iter()
                .find_map(|entry| match entry {
                    Entry::Item { name, enabled, .. } if name.starts_with("Stop") => Some(*enabled),
                    _ => None,
                })
                .expect("a Stop entry")
        };
        assert!(!stop(&chosen));
        assert!(stop(&MenuState { run_active: true, ..chosen }));
    }

    #[test]
    fn run_current_file_is_absent_for_a_file_whose_language_names_no_command() {
        // The rule the three code-navigation entries already follow. A `.rs` file will never have
        // one, because running one file of a Cargo project is not a thing cargo does.
        assert!(!names(&run_menu(&MenuState::default()).entries)
            .contains(&"Run Current File".to_owned()));
        let state = MenuState { run_file_applies: true, ..MenuState::default() };
        assert!(names(&run_menu(&state).entries).contains(&"Run Current File".to_owned()));
    }

    #[test]
    fn the_run_tile_is_toggled_from_the_view_menu_beside_the_terminal() {
        let view = names(&find(&menus(&MenuState::default()), "View").entries);
        let terminal = view.iter().position(|name| name == "Terminal").expect("Terminal");
        let tile = view.iter().position(|name| name == "Run Tile").expect("Run Tile");
        assert!(tile > terminal, "beside it, and after it: {view:?}");
        let showing = MenuState { run_tile_visible: true, ..MenuState::default() };
        assert!(names(&find(&menus(&showing), "View").entries).contains(&"Hide Run Tile".to_owned()));
    }

    #[test]
    fn the_run_and_stop_shortcuts_are_the_ones_the_platform_uses() {
        // The reference editor's own, which is what somebody who has used one will try.
        let state = MenuState { run_selected: Some("Dev server".to_owned()), run_active: true, ..MenuState::default() };
        let run = if cfg!(target_os = "macos") {
            action_for_key(&state, egui::Key::R, &pressing_control())
        } else {
            action_for_key(
                &state,
                egui::Key::F10,
                &egui::Modifiers { shift: true, ..Default::default() },
            )
        };
        assert_eq!(run, Some(Action::Run(RunAction::Start(None))));
        let stop = if cfg!(target_os = "macos") {
            action_for_key(&state, egui::Key::F2, &pressing_command())
        } else {
            action_for_key(&state, egui::Key::F2, &pressing_control())
        };
        assert_eq!(stop, Some(Action::Run(RunAction::Stop(None))));
    }

    #[test]
    fn the_gutter_menu_offers_blame_and_the_line_numbers() {
        let state = MenuState {
            in_repository: true,
            has_file: true,
            line_numbers: true,
            ..MenuState::default()
        };
        assert_eq!(
            names(&gutter_menu(&state)),
            vec!["Annotate with Git Blame", "Hide Line Numbers"]
        );
        // Once it is annotated the same entry closes the annotations, so there is one entry rather
        // than two that have to be kept in step.
        let annotated = MenuState { annotated: true, ..state };
        assert_eq!(names(&gutter_menu(&annotated))[0], "Close Annotations");
    }

    #[test]
    fn settings_can_be_reached_from_the_application_menu_and_from_the_edit_menu() {
        let bar = menus(&MenuState::default());
        assert!(names(&find(&bar, "Unluminate").entries).contains(&"Settings".to_owned()));
        assert!(names(&find(&bar, "Edit").entries).contains(&"Settings".to_owned()));
        // Only one of them carries the shortcut, because two menu items claiming one key equivalent is a
        // fault on macOS.
        let mut with_shortcut = 0;
        for menu in menus(&MenuState::default()) {
            for entry in &menu.entries {
                if let Entry::Item { name, shortcut: Some(_), .. } = entry {
                    if name == "Settings" {
                        with_shortcut += 1;
                    }
                }
            }
        }
        assert_eq!(with_shortcut, 1);
    }

    #[test]
    fn the_file_menu_holds_the_things_it_used_to_and_the_new_window() {
        let bar = menus(&MenuState::default());
        let file = names(&find(&bar, "File").entries);
        for expected in ["New Window", "Open File", "Open Folder", "Save", "Save As"] {
            assert!(file.contains(&expected.to_owned()), "File should hold {expected}, it has {file:?}");
        }
    }

    #[test]
    fn opening_a_folder_is_one_entry_and_it_opens_a_window_of_its_own() {
        // There used to be two, and they differed only in which window the project landed in.
        // `task-1658` asks for a project to be a window, so there is one entry.
        let file = names(&find(&menus(&MenuState::default()), "File").entries);
        let opens: Vec<&String> =
            file.iter().filter(|name| name.starts_with("Open Folder")).collect();
        assert_eq!(opens, vec![&"Open Folder".to_owned()]);
    }

    #[test]
    fn recent_projects_is_left_out_until_there_is_something_in_it() {
        let bar = menus(&MenuState::default());
        assert!(
            !names(&find(&bar, "File").entries).contains(&"Recent Projects".to_owned()),
            "an empty Recent Projects is not shown at all"
        );

        let state = MenuState { recent: vec![PathBuf::from("/tmp/one")], ..MenuState::default() };
        let bar = menus(&state);
        let file = find(&bar, "File");
        let submenu = file
            .entries
            .iter()
            .find_map(|entry| match entry {
                Entry::Submenu { name, entries } if name == "Recent Projects" => Some(entries.clone()),
                _ => None,
            })
            .expect("Recent Projects should be there once a project has been opened");
        assert_eq!(names(&submenu), vec!["one", "Forget Recent Projects"]);
    }

    #[test]
    fn undo_is_dimmed_until_there_is_something_to_undo() {
        let bar = menus(&MenuState::default());
        let edit = find(&bar, "Edit");
        let undo = edit
            .entries
            .iter()
            .find(|entry| matches!(entry, Entry::Item { name, .. } if name == "Undo"))
            .expect("Edit should hold Undo");
        assert!(matches!(undo, Entry::Item { enabled: false, .. }));

        let state = MenuState { can_undo: true, ..MenuState::default() };
        let bar = menus(&state);
        let edit = find(&bar, "Edit");
        let undo = edit
            .entries
            .iter()
            .find(|entry| matches!(entry, Entry::Item { name, .. } if name == "Undo"))
            .expect("Edit should hold Undo");
        assert!(matches!(undo, Entry::Item { enabled: true, .. }));
    }

    #[test]
    fn the_view_menu_marks_the_mode_that_is_showing() {
        let state = MenuState { view_mode: ViewMode::Preview, ..MenuState::default() };
        let view = find(&menus(&state), "View");
        let checked: Vec<String> = view
            .entries
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item { name, checked: true, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(checked, vec!["Markdown Preview"]);
    }

    /// The modifier set the platform really sends when the command key is held: on macOS the Apple
    /// key, which egui reports as `command` and `mac_cmd`; everywhere else the control key, which it
    /// reports as `command` and `ctrl` both.
    fn pressing_command() -> egui::Modifiers {
        if cfg!(target_os = "macos") {
            egui::Modifiers { command: true, mac_cmd: true, ..Default::default() }
        } else {
            egui::Modifiers { command: true, ctrl: true, ..Default::default() }
        }
    }

    /// The control key itself, which on macOS is not the command key and everywhere else is.
    fn pressing_control() -> egui::Modifiers {
        if cfg!(target_os = "macos") {
            egui::Modifiers { ctrl: true, ..Default::default() }
        } else {
            egui::Modifiers { command: true, ctrl: true, ..Default::default() }
        }
    }

    #[test]
    fn eguis_own_command_modifier_is_matched_as_well_as_the_platforms() {
        // A test presses `egui::Modifiers::COMMAND`, which sets `command` and leaves `ctrl` clear —
        // a combination no platform sends. Both have to work, or the shortcuts pass their tests and
        // do nothing in the real window, which is exactly what happened before this was fixed.
        let state = MenuState::default();
        assert_eq!(
            action_for_key(&state, egui::Key::S, &egui::Modifiers::COMMAND),
            Some(Action::Save)
        );
    }

    #[test]
    fn a_shortcut_the_platform_really_sends_is_matched() {
        // This is the case that used to fail on Windows: `Ctrl+S` arrives with `command` and `ctrl`
        // both set, and `Save` asks for the command key and not the control key.
        let state = MenuState::default();
        assert_eq!(action_for_key(&state, egui::Key::S, &pressing_command()), Some(Action::Save));
        assert_eq!(
            action_for_key(&state, egui::Key::Backtick, &pressing_control()),
            Some(Action::ToggleTerminal),
            "control and backtick opens the terminal on both platforms"
        );
    }

    #[test]
    fn the_font_size_can_be_changed_from_the_keyboard_however_plus_is_typed() {
        // `+` and `=` are one key, and `+` is the shifted one, so all three of these are a person
        // pressing "control and plus": the unshifted key, the shifted key, and the numeric keypad.
        let state = MenuState::default();
        let larger = Some(Action::ChangeFontSize { larger: true });
        assert_eq!(action_for_key(&state, egui::Key::Equals, &pressing_command()), larger);
        assert_eq!(action_for_key(&state, egui::Key::Plus, &pressing_command()), larger);
        let shifted = egui::Modifiers { shift: true, ..pressing_command() };
        assert_eq!(action_for_key(&state, egui::Key::Plus, &shifted), larger);
        assert_eq!(
            action_for_key(&state, egui::Key::Minus, &pressing_command()),
            Some(Action::ChangeFontSize { larger: false })
        );
    }

    #[test]
    fn the_plus_rule_does_not_loosen_any_other_shortcut() {
        // Only plus accepts an unasked-for shift. `Cmd+S` and `Cmd+Shift+S` are two different
        // entries and have to stay that way.
        let state = MenuState::default();
        let shifted = egui::Modifiers { shift: true, ..pressing_command() };
        assert_eq!(action_for_key(&state, egui::Key::S, &shifted), Some(Action::SaveAs));
        assert_eq!(action_for_key(&state, egui::Key::S, &pressing_command()), Some(Action::Save));
    }

    #[test]
    fn resetting_the_font_size_is_on_the_view_menu_without_a_shortcut() {
        // Command and zero is the obvious one and `Show Explorer` already claims it. Two entries on
        // one key equivalent is a fault on macOS, and there is a test above that would catch it.
        let view = find(&menus(&MenuState::default()), "View");
        let reset = view
            .entries
            .iter()
            .find(|entry| matches!(entry, Entry::Item { name, .. } if name == "Reset Font Size"))
            .expect("Reset Font Size is on the View menu");
        assert!(matches!(reset, Entry::Item { shortcut: None, action: Action::ResetFontSize, .. }));
    }

    #[test]
    fn only_undo_redo_and_select_all_belong_to_a_focused_text_box() {
        // `task-1656`. While one of the window's text boxes has the keyboard these three mean that
        // box, so the keyboard watcher lets them go. The rest of the menu is untouched, which is the
        // half that stops the guard from being too broad.
        for action in [Action::Undo, Action::Redo, Action::SelectAll] {
            assert!(action.belongs_to_a_focused_text_box(), "{action:?} belongs to the box");
        }
        for action in [
            Action::Save,
            Action::SaveAs,
            Action::ToggleExplorer,
            Action::ToggleTerminal,
            Action::Settings,
            Action::SetViewMode(ViewMode::Preview),
        ] {
            assert!(
                !action.belongs_to_a_focused_text_box(),
                "{action:?} keeps working while a box has the keyboard"
            );
        }
    }

    #[test]
    fn a_shortcut_is_spelled_out_in_words() {
        assert_eq!(Shortcut::command(egui::Key::S).label(), if cfg!(target_os = "macos") { "Cmd+S" } else { "Ctrl+S" });
        assert_eq!(
            Shortcut::command_shift(egui::Key::O).label(),
            if cfg!(target_os = "macos") { "Cmd+Shift+O" } else { "Ctrl+Shift+O" }
        );
        assert_eq!(Shortcut::command(egui::Key::Comma).label().ends_with(','), true);
        assert_eq!(Shortcut::control(egui::Key::Backtick).label(), "Ctrl+`");
    }

    #[test]
    fn a_key_press_finds_the_action_whose_shortcut_it_is() {
        let state = MenuState { can_undo: true, can_redo: true, ..MenuState::default() };
        let command = pressing_command();
        assert_eq!(action_for_key(&state, egui::Key::S, &command), Some(Action::Save));
        assert_eq!(action_for_key(&state, egui::Key::Z, &command), Some(Action::Undo));
        let with_shift = egui::Modifiers { shift: true, ..command };
        assert_eq!(action_for_key(&state, egui::Key::Z, &with_shift), Some(Action::Redo));
        assert_eq!(action_for_key(&state, egui::Key::S, &with_shift), Some(Action::SaveAs));
    }

    #[test]
    fn a_shortcut_with_more_modifiers_is_not_mistaken_for_one_with_fewer() {
        let state = MenuState::default();
        let command = pressing_command();
        assert_eq!(action_for_key(&state, egui::Key::N, &command), None, "Cmd+N is not New Window");
        let with_shift = egui::Modifiers { shift: true, ..command };
        assert_eq!(action_for_key(&state, egui::Key::N, &with_shift), Some(Action::NewWindow));
    }

    #[test]
    fn undo_that_cannot_be_done_is_not_taken_from_the_keyboard_either() {
        let state = MenuState::default();
        let command = pressing_command();
        assert_eq!(action_for_key(&state, egui::Key::Z, &command), None);
    }

    #[test]
    fn the_clipboard_shortcuts_are_left_to_the_platform() {
        // Cut, copy and paste reach the window as egui clipboard events. If they were watched for here as
        // well, one press would do the work twice.
        let state = MenuState { has_selection: true, ..MenuState::default() };
        let command = pressing_command();
        assert_eq!(action_for_key(&state, egui::Key::C, &command), None);
        assert_eq!(action_for_key(&state, egui::Key::V, &command), None);
        assert_eq!(action_for_key(&state, egui::Key::X, &command), None);
        // They are still in the menu, with their shortcuts shown.
        let edit = find(&menus(&state), "Edit");
        assert!(names(&edit.entries).contains(&"Paste".to_owned()));
    }

    #[test]
    fn every_shortcut_in_the_bar_is_claimed_by_one_entry_only() {
        let state = MenuState {
            can_undo: true,
            can_redo: true,
            has_selection: true,
            recent: vec![PathBuf::from("/tmp/one")],
            terminal_tabs: 1,
            // The run entries are dimmed with nothing chosen, and a dimmed entry's shortcut is
            // still a shortcut the bar claims, so this walks them switched on.
            run_selected: Some("Dev server".to_owned()),
            run_names: vec!["Dev server".to_owned()],
            run_active: true,
            run_file_applies: true,
            ..MenuState::default()
        };
        let mut seen: Vec<(Shortcut, String)> = Vec::new();
        fn walk(entries: &[Entry], seen: &mut Vec<(Shortcut, String)>) {
            for entry in entries {
                match entry {
                    Entry::Item { name, shortcut: Some(shortcut), .. } => {
                        if let Some((_, other)) = seen.iter().find(|(seen, _)| seen == shortcut) {
                            panic!("{} and {other} both claim {}", name, shortcut.label());
                        }
                        seen.push((*shortcut, name.clone()));
                    }
                    Entry::Submenu { entries, .. } => walk(entries, seen),
                    _ => {}
                }
            }
        }
        for menu in menus(&state) {
            walk(&menu.entries, &mut seen);
        }
        assert!(seen.len() > 10, "there should be a shortcut on most entries, found {}", seen.len());
    }
    #[test]
    fn every_folding_entry_is_reachable_from_the_keyboard() {
        // A shortcut that passes its test and does nothing in the real window is the fault
        // `a_shortcut_the_platform_really_sends_is_matched` records, so these are asked with the
        // modifiers a platform really sends.
        let state = MenuState { folding_applies: true, foldable: 3, folded: 1, ..MenuState::default() };
        assert_eq!(
            action_for_key(&state, egui::Key::Period, &pressing_command()),
            Some(Action::Fold(FoldAction::Toggle))
        );
        assert_eq!(
            action_for_key(&state, egui::Key::Period, &egui::Modifiers { shift: true, ..pressing_command() }),
            Some(Action::Fold(FoldAction::All))
        );
        assert_eq!(
            action_for_key(&state, egui::Key::Comma, &egui::Modifiers { shift: true, ..pressing_command() }),
            Some(Action::Fold(FoldAction::None_))
        );
        assert_eq!(
            action_for_key(&state, egui::Key::Comma, &pressing_command()),
            Some(Action::Settings),
            "the comma on its own still opens Settings"
        );
        assert_eq!(
            action_for_key(&state, egui::Key::Period, &egui::Modifiers { alt: true, ..pressing_command() }),
            Some(Action::Fold(FoldAction::Others))
        );
        assert_eq!(
            action_for_key(
                &state,
                egui::Key::Period,
                &egui::Modifiers { alt: true, shift: true, ..pressing_command() }
            ),
            Some(Action::Fold(FoldAction::CollapseRecursively)),
            "the recursive pair is the same two keys with the one remaining modifier added"
        );
        assert_eq!(
            action_for_key(
                &state,
                egui::Key::Comma,
                &egui::Modifiers { alt: true, shift: true, ..pressing_command() }
            ),
            Some(Action::Fold(FoldAction::ExpandRecursively))
        );
    }

    #[test]
    fn a_file_that_cannot_fold_has_no_folding_entries_and_no_folding_keys() {
        // Unluminate's rule for a control that can never apply: absent, not dimmed.
        let state = MenuState { folding_applies: false, ..MenuState::default() };
        assert!(folding_menu(&state).is_empty());
        assert!(folding_here_menu(&state).is_empty());
        assert_eq!(action_for_key(&state, egui::Key::Period, &pressing_command()), None);
        assert!(!names(&gutter_menu(&state)).contains(&"Collapse All".to_owned()));
    }

    #[test]
    fn a_file_with_nothing_to_fold_dims_the_folding_entries_rather_than_hiding_them() {
        // The other half of the rule: a control that could be used in a moment is dimmed.
        let state = MenuState { folding_applies: true, foldable: 0, folded: 0, ..MenuState::default() };
        let entries = folding_menu(&state);
        assert_eq!(entries.len(), 6);
        assert!(entries.iter().all(|entry| matches!(entry, Entry::Item { enabled: false, .. })));
    }

    #[test]
    fn the_two_expand_entries_are_the_only_ones_that_need_something_collapsed() {
        let state = MenuState { folding_applies: true, foldable: 3, folded: 0, ..MenuState::default() };
        let usable: Vec<String> = folding_menu(&state)
            .iter()
            .filter_map(|entry| match entry {
                Entry::Item { name, enabled: true, .. } => Some(name.clone()),
                _ => None,
            })
            .collect();
        assert_eq!(
            usable,
            vec![
                "Collapse or Expand Block",
                "Collapse All",
                "Collapse All But Highlighted",
                "Collapse Recursively",
            ]
        );
    }

}
