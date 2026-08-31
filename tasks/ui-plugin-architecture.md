# ui-plugin-architecture — plugins that draw

> We want to update our plugin architecture to allow for UI rendering in rust.
>
> - should be able to add buttons/icons to the left, top, etc.
> - should be able to define panes. e.g. I may want a new icon button on the left to open a pane on
>   the right for chatting with an agent.
> - should be able to render as a tab in the text editor pane. e.g. file.txt has a tab, Agent-tasks
>   should have a tab.
> - should be able to define menu items. e.g we have Quill, File, etc so I should be able to add
>   Agent-Tasks with sub items and nested sub items
> - add to settings menu in Quill settings
> - understand current settings like font and background transparency, so the plugin can match the
>   settings of the app.
>
> We want strong capabilities for customization, flexibility that can be injected when a plugin is
> installed (prefer not to restart, but fine if we have to)

Five contributions, one manifest, no restart. This is the design. `services/plugins.rs` grows the
manifest keys and the registry, `services/plugin_ui.rs` is the registry's code half, and
`components/plugin_pane.rs` draws a contributed pane.

## 1. What Quill has today, and why it cannot answer the ask as it stands

A plugin in Quill is data. A folder holds `plugin.conf`, an icon and the words that make up a
language, and loading one is reading a file. Nothing in a plugin is executed. `plugin.kind` is read
and checked, and a manifest saying anything but `language` is refused with a message rather than
loaded halfway.

**Source:** [`quill`, `crates/quill-app/src/services/plugins.rs` lines 96 to 101](https://github.com/jasonmcaffee/quill/blob/4fc49f1ce68258bcaec1ca9e4eeb16bc0aec2307/crates/quill-app/src/services/plugins.rs#L96-L101)

```rust
/// The kind of plugin. One today, and the field exists so that a second one can be refused rather
/// than half-loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A description of a language: extensions, a grammar, an icon and a colour scheme.
    Language,
}
```

**Source:** [`quill`, `crates/quill-app/src/services/plugins.rs` lines 465 to 478](https://github.com/jasonmcaffee/quill/blob/4fc49f1ce68258bcaec1ca9e4eeb16bc0aec2307/crates/quill-app/src/services/plugins.rs#L465-L478)

```rust
pub fn parse(values: &Values, bundled: bool) -> Result<Plugin, String> {
    let id = values.text("plugin.id").ok_or("plugin.id is missing")?.to_owned();
    let kind = match values.text("plugin.kind").unwrap_or("language") {
        "language" => Kind::Language,
        other => return Err(format!("plugin.kind is `{other}`, and this version of Quill only runs `language` plugins")),
    };
    let extensions: Vec<String> = list(values, "language.extensions")
        .into_iter()
        .map(|extension| extension.trim_start_matches('.').to_lowercase())
        .collect();
    if extensions.is_empty() {
        return Err("language.extensions is empty, so nothing would ever use this plugin".to_owned());
    }
```

That refusal is where this design starts. Six further things in the window are wired to the idea that
the only plugin is a language, and each of them has to change.

**A plugin with no file extensions is refused.** The check on `language.extensions` above rejects a
manifest that claims no file type, because a language claiming none would never be used. Agent-Tasks
claims no file type at all, so the check has to become a check per kind.

**A panel is an enum, deliberately.** The four panels that can be docked are variants, and the
comment on them says a fifth is a variant rather than a registry entry.

**Source:** [`quill`, `crates/quill-app/src/app/dock.rs` lines 32 to 43](https://github.com/jasonmcaffee/quill/blob/4fc49f1ce68258bcaec1ca9e4eeb16bc0aec2307/crates/quill-app/src/app/dock.rs#L32-L43)

```rust
/// The panels that can be moved.
///
/// Four, because four is what the window has. Not a trait and not a registry: a fifth is a variant,
/// and the compiler then names every place that has to answer for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Panel {
    Explorer,
    Terminal,
    Run,
    Debug,
}
```

`Panel::index` returns 0 to 3 and is what indexes the arrays in `dock::Layout` and `settings::Panes`.
A pane that arrives when a manifest is read has no compile time index, so those arrays cannot stay
arrays of four.

**A tab in the editing area is a document.** `OpenFile` holds a `Document`, and a picture is a
document with `picture: Some(..)` beside it. There is no third kind of tab.

**An action is an enum.** `app::actions::Action` has one variant per thing the window can be asked to
do, and `QuillApp::run_action` is the single place a variant becomes a change. A plugin's command is
not a variant known at compile time.

**A command line command is `'static`.** The catalogue is a `&'static [Command]` whose every field is
a `&'static str`, and the MCP tools are generated from it.

**Source:** [`quill`, `quill-cli/src/catalogue.rs` lines 60 to 72](https://github.com/jasonmcaffee/quill/blob/4fc49f1ce68258bcaec1ca9e4eeb16bc0aec2307/quill-cli/src/catalogue.rs#L60-L72)

```rust
/// One command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Command {
    /// The grouping it is typed under, or `""` when it is typed on its own.
    pub area: &'static str,
    pub verb: &'static str,
    pub summary: &'static str,
    pub arguments: &'static [Argument],
    pub flags: &'static [Flag],
    pub examples: &'static [&'static str],
    /// True when the client answers it without a running Quill.
    pub local: bool,
}
```

**A settings page is an enum too.** `components::settings_dialog` has five pages, the window is one
size for every page, and a page does not scroll.

So the work is not "let a plugin draw". It is to make five closed lists into lists that a manifest can
add to, without giving up the property that makes every one of them closed today: the compiler names
every place that has to answer for a new member.

## 2. What was weighed

Seven systems were read. The two closest comparisons are the two Rust editors, and they agree with
each other.

### 2.1 The seven systems

| | How a plugin contributes to the window | What it costs |
|---|---|---|
| **IntelliJ Platform** | `com.intellij.toolWindow` in `plugin.xml` gives an id, an icon, an anchor of left, right or bottom, a primary or secondary group flag, and a factory class. `createToolWindowContent()` runs the first time somebody clicks the button. Settings pages are `applicationConfigurable` and `projectConfigurable`. | More than 1700 extension points. Loading and unloading without a restart is possible but fenced: every extension point used has to be declared dynamic, action groups need ids, load and unload happen on the event thread under a write action, cached values are dropped, and a plugin that cannot meet the rules declares `require-restart="true"`. |
| **VS Code** | About 46 contribution points in `package.json`, all data. `viewsContainers` puts an icon on the activity bar, `views` fills it, `menus` names about 40 fixed anchors with a `when` expression and a `group` for ordering, `submenus` nests them. | A view is filled either by a tree data provider, where VS Code draws and the extension supplies rows, or by a webview, where the extension renders HTML. Microsoft's own guidance on the second: webviews "are resource heavy and run in a separate context", "should be used sparingly and only when VS Code's native API is inadequate", and "a poorly designed webview can also easily feel out of place". A webview reaches the theme only through CSS variables. |
| **Zed** | Nothing. `extension.toml` provides languages, debuggers, themes, icon themes, snippets and MCP servers. There is no way for an extension to put a button or a pane in the window. | Procedural parts compile to `wasm32-wasip2`. The closest comparison to Quill, a Rust editor with its own renderer, and it has not opened this door at all. |
| **Lapce** | Nothing. WASI plugins speak JSON-RPC through `psp-types`, and they run in `lapce-proxy` rather than in the window, which is also what makes remote development work. | Plugins mostly exist to wire up language servers. The second Rust editor, and the second one with no way to draw. |
| **Eclipse** | OSGi bundles and an extension registry, with lazy activation through proxies so a bundle nobody uses is never started. | The ancestor of the whole model. Its declarative manifest plus lazy activation is the part that survived into IntelliJ and VS Code. |
| **Obsidian** | A plugin is TypeScript running in the application's own process with access to its DOM, so it can draw anything. | Nothing separates a plugin from the application. A plugin can break the editor, and Obsidian's answer is a review process rather than a boundary. |
| **Sublime Text** | An embedded Python interpreter. Panels are limited to what the API offers. | The same trade as Obsidian without the DOM. |

### 2.2 The four ways a plugin's code could arrive

| | What it is | Verdict |
|---|---|---|
| **Compiled into the binary, named by the manifest** | A Rust module in `quill-app` that the manifest names by a key checked against a list. The manifest is data saying what is contributed and where; the code shipped with Quill. | **Chosen for the first UI plugin.** It is the pattern `language.renders`, `run.project` and `debug.adapter` already follow three times, and no fourth mechanism has to be invented before anything can be drawn. |
| **A dynamic library** | A `cdylib` loaded with `libloading`, exporting a function Quill calls with an `egui::Ui`. | Refused. A Rust type passed over that boundary is undefined behaviour unless both sides were built by the same compiler with the same flags, so every plugin would need rebuilding for every release, and a plugin that crashes takes the window with it. `hot-lib-reloader` is the honest state of this: a development tool, dylib only, no generic functions, global state breaks across a reload, and the library needs codesigning on macOS. `egui::Ui` is generic and closure heavy, which is the exact shape that cannot cross the boundary. |
| **WebAssembly** | A component in a `wasmtime` sandbox, called through a WIT interface. | Right for computation, wrong for drawing. The component model has no pointers and cannot pass object graphs or recursive structures, which is what an immediate mode UI call graph is. VS Code's own account of the component model says arguments are copied by value and the limits are JSON's limits. Drawing through it means designing a widget protocol first, which §13 keeps as its own work. |
| **A separate process over the control channel** | The plugin is a program. It declares its contributions in the manifest and answers questions over the socket Quill already listens on. | **The named route for third party plugins, and left empty for now.** It is what Quill already does for git, for debug adapters, for terminals and for the command line, and a plugin that crashes is a plugin that has exited. It needs the same widget protocol WebAssembly needs, so it waits for the same work. |

### 2.3 What a contributed pane may draw with

This is the design's central question, and both precedents in the repository answer it the same way.

A Mermaid diagram is laid out in `quill-core` and drawn in `quill-app`, and the seam between them is a
`Scene` of five kinds of item: rectangles, circles, polygons, lines and text at absolute positions.
`components::diagram_view` has no diagram knowledge at all, which is why a twenty first diagram type
needs no change there. A syntax theme colours the tokens and not the background, and Dracula's own
`#282A36` is deliberately unused, because a plugin does not get to repaint the editing area.

VS Code arrived at the same split from the other direction. Its tree views hand over rows and VS Code
draws them; its webviews hand over HTML and look foreign. It needed the second one because its
extensions are JavaScript and cannot call into its renderer.

Quill's first UI plugin is Rust in the same binary, so it does not need a protocol to draw through.
The decision is therefore split in two, and saying so plainly is what keeps the design buildable:

- **A provider compiled into the binary draws with `egui`,** through the same
  `components::controls`, `components::modal` and `theme` every other part of the window uses. It is
  Quill's own code, held to `design/style-guide.md`, and covered by Quill's own screenshot tests.
- **A provider that is not in the binary draws through a widget tree,** which is the Mermaid `Scene`
  decision applied to controls rather than diagrams. That tree is not designed here. §13 says why,
  and nothing in this design has to change when it arrives, because the contributions are already
  data and the pane already asks a provider for its contents rather than reaching into it.

### 2.4 What was rejected

**A trait object per contribution with no manifest.** Registering a pane in Rust code alone would
work for a plugin in the binary and would give a third party plugin nothing, and it would put the
arrangement of the window inside the plugin rather than in a file a person can read and correct. The
manifest is what makes the same keys serve both.

**A `when` expression language.** VS Code's context keys are the most copied part of its model and the
hardest to test. Quill has a smaller answer already in use: a control that cannot apply is absent, and
the question is a function. `file_kind::definitions_apply` decides whether Go to Definition is on the
Edit menu. A plugin's contribution gets the same treatment through one optional key, `pane.applies`,
whose value names a condition Quill knows. Two conditions exist to start with, `always` and
`in_project`, and the value is checked against the list.

**Per plugin colours.** A plugin naming its own palette would undo the reason the palette is closed.
Every colour a plugin draws with comes from `theme::color`, and there is no manifest key that sets
one. This is the rule the syntax themes already keep and the rule a Mermaid diagram's own `style`
directive already meets.

## 3. The model

Four values, and the last one is the only new idea.

```
Contribution   what a manifest adds to the window: a rail button, a pane, a tab, a menu, a settings page
UiProvider     code in the binary that fills a pane, a tab or a settings page, and answers a command
Registry       the list of provider names this version of Quill has, checked when a manifest is read
Surfaces       everything every enabled plugin contributes, worked out once per load and read by the window
```

`plugin.kind` gains one value.

```rust
pub enum Kind {
    /// A description of a language: extensions, a grammar, an icon and a colour scheme.
    Language,
    /// A plugin that draws: it contributes buttons, panes, tabs, menu entries and a settings page.
    Ui,
}
```

A `Ui` plugin needs no `language.extensions`, and a `Language` plugin still does. The check moves
inside the match on the kind, so the refusal a language gets is unchanged and a UI plugin is not
refused for claiming no file type.

`ui.provider` names the code, and it is checked against a registry exactly as `language.renders`,
`run.project` and `debug.adapter` are checked. This is the fourth registry of that shape and it is
built the same way.

```rust
/// The UI providers built into this version of Quill that a plugin's `ui.provider` may name.
///
/// The fourth registry of this shape, checked the same way and for the same reason: a manifest naming
/// a provider Quill does not have should say so plainly rather than load as a plugin whose pane is
/// permanently empty.
pub const UI_PROVIDERS: &[&str] = &["agent-tasks"];
```

A manifest naming a provider this version does not have is refused with the house message, which
names what was asked for and what is available.

## 4. The manifest

One example, complete, and then the keys one at a time. This is `agent-tasks/plugin.conf`, the file
the Agent-Tasks plugin ships.

```
plugin.id          = agent-tasks
plugin.name        = Agent-Tasks
plugin.version     = 1.0.0
plugin.vendor      = Quill
plugin.kind        = ui
plugin.description = A task board whose tickets are worked by an agent in a terminal Quill owns.
plugin.limitations = The board is local to this machine and its database is a single file.

ui.provider = agent-tasks

# The button in the rail down the far left, and the pane it opens.
pane.id      = board
pane.label   = Agent-Tasks
pane.icon    = board
pane.group   = top
pane.side    = right
pane.width   = 420
pane.height  = 280
pane.applies = always

# A tab in the editing area, opened from the menu rather than by opening a file.
tab.id    = board
tab.label = Agent-Tasks

# The menu, its entries, and one nested submenu.
menu.name              = Agent-Tasks
menu.entries           = board=Open Board, terminal=Open Ticket Terminal, -, sync=Sync JIRA
menu.submenu.new       = New
menu.submenu.new.entries = task=Task, epic=Epic, sprint=Sprint

# The page in Settings.
settings.page  = Agent-Tasks
settings.icon  = board
```

### 4.1 The rail button and the pane it opens

Six keys, and five of them have a default so that a manifest asking for a pane writes two lines.

| Key | What it says | Default |
|---|---|---|
| `pane.id` | The name the command line and the settings file call this pane. Lower case, one word. | Required when any `pane.` key is present. |
| `pane.label` | What a person reads in the rail's tooltip and on the pane's header. | `plugin.name`. |
| `pane.icon` | Which drawn icon goes in the rail, checked against `theme::icon`. | The plugin's own `icon.png`. |
| `pane.group` | `top` or `bottom`. The rail's two groups say what a panel is: the top group holds lists and the bottom holds tiles with a character grid in them. | `top`. |
| `pane.side` | `left`, `right`, `top` or `bottom`, the side the pane is docked to the first time it is shown. | `right`. |
| `pane.width` and `pane.height` | The two measurements a panel carries, because one number cannot be both. A width for when it is a column at the side and a height for when it is in a strip. | 320 and 260, which are the explorer's width and the terminal's height. |
| `pane.applies` | `always` or `in_project`, checked against the conditions Quill has. | `always`. |

The icon is checked rather than taken on trust. A manifest naming an icon Quill cannot draw is refused
with the list of icons, which is the same refusal `language.renders` gives.

### 4.2 A tab in the editing area

```
tab.id    = board
tab.label = Agent-Tasks
```

Two keys. A contributed tab has no path on disk, is never modified, and cannot be saved. It appears in
the tab strip beside file tabs, is closed the way a file tab is closed, and is remembered in the
project's `.quill` folder so that reopening the project reopens it, which is what a file tab already
does.

### 4.3 Menu entries, including nested ones

```
menu.name                = Agent-Tasks
menu.entries             = board=Open Board, terminal=Open Ticket Terminal, -, sync=Sync JIRA
menu.submenu.new         = New
menu.submenu.new.entries = task=Task, epic=Epic, sprint=Sprint
```

`menu.entries` is a comma list of `command=Name`. A lone `-` is a separator, which is what
`actions::Entry::Separator` already is. `menu.submenu.<id>` names a submenu and
`menu.submenu.<id>.entries` fills it with the same `command=Name` list, so a submenu inside a submenu
is `menu.submenu.new.submenu.other`. The nesting is recursive in the reader and in `Entry::Submenu`,
which already holds a `Vec<Entry>`.

A plugin's menu is added after the six Quill has, so `Quill`, `File`, `Edit`, `View`, `Run` and `Git`
never move. A plugin cannot add an entry to one of those six. VS Code allows that through about 40
named anchors and it is the largest part of its contribution model; Quill's answer for now is a menu of
the plugin's own, and §13 records what adding anchors would take.

No plugin entry carries a keyboard shortcut. Quill tests that no two menu items claim one key
equivalent, because two items claiming one chord is a real fault on macOS, and a manifest that could
claim `Cmd+S` would be able to break that test from outside the repository. A plugin's command is
reachable from its menu, from its pane and from the command line, which is three ways.

### 4.4 A page in Settings

```
settings.page = Agent-Tasks
settings.icon = board
```

The Settings window is one size for every page and a page does not scroll. It is 640 points tall
today, and the tallest page is what it has to hold. A contributed page therefore gets a fixed budget
rather than a scrolling area, and a page that wants more than 640 points is the moment to build a
scrolling page area rather than the moment to make the window taller again. The provider is handed the
page rectangle and draws its rows with `components::modal`'s own furniture, so a plugin's page is made
of the same rows, fields and tick boxes as the other five.

### 4.5 Every refusal, in one place

A manifest is either loaded or refused with a sentence. The sentences are the house style: what was
asked for, and what this version has.

| What is wrong | What is said |
|---|---|
| `plugin.kind` is not `language` or `ui` | ``plugin.kind is `wasm`, and this version of Quill runs `language` and `ui` plugins`` |
| `plugin.kind = ui` with no `ui.provider` | ``ui.provider is missing, and a ui plugin with no provider would draw nothing`` |
| `ui.provider` names something unknown | ``ui.provider is `chat`, and this version of Quill has agent-tasks`` |
| `pane.group` is not `top` or `bottom` | ``pane.group is `middle`, and the rail has top and bottom`` |
| `pane.side` is not one of the four | ``pane.side is `middle`, and a panel docks to left, right, top or bottom`` |
| `pane.icon` names an icon Quill cannot draw | ``pane.icon is `sparkle`, and this version of Quill draws board, explorer, git, terminal, run, debug`` |
| `pane.applies` names an unknown condition | ``pane.applies is `has_git`, and this version of Quill knows always, in_project`` |
| `menu.entries` holds an entry that is not `command=Name` | ``menu.entries holds `board`, which is not `command=Name``` |
| `language.extensions` is empty on a `language` plugin | unchanged: ``language.extensions is empty, so nothing would ever use this plugin`` |

A `ui` plugin that contributes nothing at all is refused as well, because a plugin that adds no
button, no pane, no tab, no menu and no settings page has no way of being reached.

## 5. The registry, and what a provider is

`services::plugin_ui` is the code half of `UI_PROVIDERS`, the way `services::debuggers` is the code
half of `DEBUGGERS`.

```rust
/// What a plugin's code is asked to do. One implementation per name in `plugins::UI_PROVIDERS`.
///
/// Every method takes `&mut self` and the window's own `Look`, and none of them can reach the window:
/// a provider draws inside the rectangle it is given and answers commands, and everything else it
/// wants is asked for by returning a `Request`. That is what keeps a plugin from moving a tab or
/// writing a file behind the editor's back.
pub trait UiProvider {
    /// The name in the manifest, which is the name in the registry.
    fn id(&self) -> &'static str;

    /// Called the first time the pane, the tab or the settings page is shown, and never before.
    fn open(&mut self, context: &Context) -> Result<(), String>;

    /// Draw the pane. Called once a frame while the pane is showing.
    fn pane(&mut self, ui: &mut egui::Ui, look: &Look) -> Vec<Request>;

    /// Draw the tab in the editing area. Called once a frame while that tab is showing.
    fn tab(&mut self, ui: &mut egui::Ui, look: &Look) -> Vec<Request>;

    /// Draw the Settings page inside the rectangle every page gets.
    fn settings(&mut self, ui: &mut egui::Ui, look: &Look) -> Vec<Request>;

    /// Answer a command from the menu, the pane or the command line. The one path a change goes down,
    /// which is what `QuillApp::run_action` is for the window.
    fn command(&mut self, command: &str, arguments: &[String]) -> Result<Answer, String>;

    /// What the command line prints and what a test reads: the pane's contents as data rather than as
    /// pixels.
    fn view(&self) -> serde_json::Value;

    /// Called when the plugin is switched off, when the project changes, or when the window closes.
    fn close(&mut self);
}
```

Five things about that trait are decisions rather than shape.

**`open` is separate from `pane`, and that is what makes a plugin lazy.** Nothing a provider owns is
built until the first time somebody presses its button. This is IntelliJ's `createToolWindowContent`,
whose documented reason is the same one: "if a user does not interact with the tool window, no plugin
code will be loaded or executed". For Agent-Tasks it is the difference between opening a SQLite file at
startup and opening it when the board is first looked at.

**Drawing returns requests instead of doing things.** A provider that wanted to open a file would have
to reach the window's `OpenFiles`, and then two things would own the tab strip. So a provider returns
`Request::OpenFile(PathBuf)`, `Request::ShowPane`, `Request::Message(String)` and the handful of others
it needs, and `QuillApp` acts on them after the pane has been drawn. This is the rule
`components::activity_bar` already keeps: nothing there changes anything, and each button reports the
`Action` it stands for.

**`view` is not optional.** Quill's rule is that everything a person can do in the window an agent can
do too, through the same code, and both are covered by tests. A pane drawn with `egui` is invisible to
both a test and an agent unless it can also be read as data. `view` is what `quill-cli plugin view`
prints and what a unit test asserts against, so a plugin's contents are testable with no window.

**`command` is the only mutating path.** The menu entry, the button inside the pane and the command
line all call it with the same string, so a thing done by hand and the same thing done by an agent are
the same thing. This is what `QuillApp::run_cli` and `run_action` already guarantee for Quill's own
commands.

**`Look` is how the plugin matches the window.** It is a value, built once a frame from the settings
and the theme, and it is the only way a provider learns a colour or a size.

```rust
/// Everything a provider needs to look like the rest of the window.
///
/// It is handed over rather than reached for, so a plugin cannot read a setting it was not given and
/// cannot name a colour of its own. `opacity` is the setting that lets the desktop show through, and a
/// pane that ignores it would be the one opaque rectangle in a transparent window.
pub struct Look {
    pub font_family: String,
    pub font_size: f32,
    pub monospace_size: f32,
    pub opacity: f32,
    pub palette: Palette,
    pub row_height: f32,
    pub menu_row_height: f32,
    pub corner_radius: f32,
}
```

`Palette` is `theme::color` passed through, so a plugin's rows, fields, buttons and selected row are
the ones every list in Quill draws. `row_height` is 28 and `menu_row_height` is 24, which is what
`design/style-guide.md` says a list row and a menu row are. The provider is given the numbers rather
than the file, so a plugin cannot disagree with the style guide by accident.

## 6. Where each contribution goes in the window

### 6.1 The rail

`components::activity_bar` gains a third group: the plugin buttons, drawn in the group each manifest
asks for, after Quill's own. `RailState` grows one field, a slice of `(pane_id, on)`, and `RailOutcome`
already carries an `Action`, so a plugin's button reports `Action::PluginPane { plugin, pane }` and
goes down `QuillApp::run_action` with every other button.

The rail is 36 points wide and a button is 24 with a 30 point step, so eight buttons fit in a 300 point
tall rail and about twenty in a tall window. A plugin contributing one pane is the common case. With
more buttons than fit, the group scrolls, which is the same answer the tab strip already gives.

### 6.2 The dock

`Panel` gains a variant.

```rust
pub enum Panel {
    Explorer,
    Terminal,
    Run,
    Debug,
    /// A pane a plugin contributed, identified by `<plugin id>/<pane id>` rather than by an index,
    /// because the set is decided when the manifests are read rather than at compile time.
    Plugin(PaneId),
}
```

`Panel::ALL` becomes `Panel::all(&Surfaces)`, and `Panel::index` goes away. The two places that used
the index are `dock::Layout` and `settings::Panes`, and both become maps keyed by
`Panel::name()`, which is the string the settings file and the command line already use. That is a
change to how the layout is stored rather than to what it means, and the file it is written to keeps
the same names, so a `workspace.conf` written by the previous version still reads.

Everything else in the dock is unchanged. The three rules that settle the awkward cases already speak
about panels in general rather than about four particular ones: order is screen order along x, the
strips are taken first and the columns come out of what is left, and a panel carries two measurements
with the side deciding which is read. A contributed pane in the bottom strip is a tile, so
`Panel::is_a_tile` answers from `pane.group` and the rule that the bottom of the window holds one tile
at a time covers it without a new rule.

### 6.3 A tab in the editing area

The picture precedent is followed exactly. `OpenFile` holds a `Document` and a picture is a document
with a `Picture` beside it, so a plugin tab is a document with a `PluginTab` beside it.

```rust
pub struct OpenFile {
    pub document: Document,
    // ... the existing fields ...
    /// The picture, when this tab holds one rather than text.
    pub picture: Option<Picture>,
    /// The plugin, when this tab is a plugin's own rather than a file.
    pub plugin: Option<PluginTab>,
}
```

The document behind a plugin tab is empty and has no path. Four questions the window asks a tab have
to answer for it, and all four have an obvious answer: it is never modified, it cannot be saved, it has
no preview modes, and the gutter, the blame column and the folding arrows are absent. Those are the
same four answers a picture tab gives, so the code that already asks them is where the answer goes.

`components::editor_view` chooses what fills the editing area, and it already chooses between text, a
picture and a diagram. A plugin tab is a fourth arm that calls `provider.tab`.

### 6.4 The menus

`actions::menus` returns a `Vec<Menu>` of owned `String`s, so the plugin menus are appended and nothing
in the six built in menus changes. `MenuState` grows one field holding the contributed menus, worked
out from `Surfaces` when the plugins are loaded rather than once a frame.

`Action` gains one variant for a plugin command and one for a pane.

```rust
pub enum Action {
    // ... the existing variants ...
    /// Run a command a plugin declared. The strings are the plugin's id and the command's name, which
    /// is what the menu entry, the pane's own button and `quill-cli plugin run` all carry.
    PluginCommand { plugin: String, command: String },
    /// Show or hide a pane a plugin contributed.
    PluginPane { plugin: String, pane: String },
}
```

`app/action_names.rs` fails when a menu entry has no name, and it walks `actions::menus` to build
`quill-cli action list`. Both keep working: a plugin's entry has a name because `menu.entries` gives
it one, and the walk finds it because the menu is in the list the walk reads. So a plugin's menu entry
is agent reachable the day the manifest is written, with no further work, which is the property the
machinery exists to give.

### 6.5 The settings page

`settings_dialog::Page` gains `Page::Plugin(PluginId)`, and `Page::ALL` becomes a function of
`Surfaces` the way `Panel::ALL` does. The page list down the left of the Settings window grows a row
per contributing plugin, after the five Quill has. The page's body is `provider.settings(ui, look)`
inside the same rectangle every page gets.

The Plugins page itself is unchanged in shape and gains one thing: a `ui` plugin's row says what it
contributes, so somebody reading the list can see that Agent-Tasks adds a pane, a tab, a menu and a
page rather than a file type.

## 7. Loading, switching off, and not restarting

`Plugins::load` reads the bundled manifests and then anything on disk, where a plugin on disk shadows a
bundled one of the same id. That is unchanged. What is new is that after loading, the surfaces are
worked out once.

```rust
/// Everything every enabled plugin contributes, worked out once when the plugins are loaded.
///
/// One value rather than a question asked of each plugin every frame: the rail, the dock, the menus,
/// the tab strip and the Settings window all read it, and none of them can disagree with the others
/// about what is contributed.
pub struct Surfaces {
    pub panes: Vec<PaneSurface>,
    pub tabs: Vec<TabSurface>,
    pub menus: Vec<Menu>,
    pub pages: Vec<PageSurface>,
}
```

**Switching a plugin off withdraws every contribution in the same frame.** This is what
`Plugins::renders` already does for Mermaid: the window asks before it draws a diagram anywhere, so
`.mmd` files stop being drawn the moment the tick box is cleared. Here, `set_enabled` rebuilds
`Surfaces`, the rail loses the button, the dock loses the panel, the menu goes, the Settings page goes,
an open plugin tab closes, and `provider.close()` is called so the provider drops what it holds. The
reverse is one frame too.

**No restart is needed for anything in this design, and that is a property of the mechanism rather
than a feature that was added.** A provider is already in the binary, so switching a plugin on is
reading a file and calling `open`. A manifest edited by hand is re-read by `Plugins::load`, and
`Settings -> Plugins` gains a `Reload plugins` button that calls it, so a person writing a manifest
sees the result without leaving the window.

That is worth setting against IntelliJ, whose dynamic plugin support is a list of restrictions:
extension points declared dynamic, no components, ids on every action group, load and unload on the
event thread under a write action, cached values dropped, no stored PSI, and `require-restart="true"`
for a plugin that cannot comply. Quill avoids all of it by not loading code at run time. The cost is
stated rather than hidden: **a third party cannot ship a UI plugin in this version.** Its manifest
would name a provider that does not exist and would be refused with a message saying so. §13 is where
that door is named.

## 8. One command, three ways in, and the command line

The catalogue is `&'static`, so a plugin's commands cannot be rows in it. The answer is a fixed area
whose verbs are static and whose arguments name the plugin, which keeps the catalogue, the generated
MCP tools and the documentation test working untouched.

| Command | What it does |
|---|---|
| `plugin list` | Every plugin, its kind, whether it is on, and what it contributes. |
| `plugin show <id>` | One plugin: its manifest values, its provider, its refusals if it was refused. |
| `plugin enable <id>` and `plugin disable <id>` | The tick box in `Settings -> Plugins`. |
| `plugin reload` | Re-read every manifest from disk, which is the button in the Plugins page. |
| `plugin pane <id> <pane>` with `--show`, `--hide`, `--side <side>` | The rail button and the dock's `Move to`. |
| `plugin tab <id> <tab>` with `--open`, `--close` | A tab in the editing area. |
| `plugin run <id> <command> [arguments...]` | `provider.command`, which is the same path the menu entry takes. |
| `plugin view <id>` | `provider.view` as JSON: what the pane is showing, as data. |

`plugin view` is the one that matters for the rule about agents. A pane full of cards is unreadable to
a test and to an agent unless there is a way to ask what is in it, and a screenshot is not an answer to
"how many tickets are in progress". So the same call that a test asserts against is the one an agent
reads, and there is no second path that could drift from it.

Eight verbs are added to `quill-cli/src/catalogue.rs` and eight arms to `app/cli.rs`. The MCP tools are
generated from the catalogue, so all eight are offered to an agent with no further work, and
`every_command_is_offered_as_a_tool_in_both_shapes` keeps passing. Each verb gets a section in
`quill-cli/docs/commands.md`, because `quill-cli/src/documentation.rs` fails while a command has none.

## 9. Tests

The bar is the repository's own: a feature is the control, the way an agent asks for the same thing
through the same code, and tests over both.

**The manifest, with no window.** Every refusal in §4.5 has a test asserting the message names what
was asked for and what is available. A `ui` manifest with no `language.extensions` loads, and a
`language` manifest with none is still refused. The bundled language plugins are asserted to ask for
none of the new keys, which is the test `the_older_plugins_ask_for_none_of_what_the_imports_added`
already makes for a previous round of keys, and it is what proves a change to the reader has not moved
Rust, CSS, HTML, JavaScript, TypeScript or Mermaid.

**The surfaces, with no window.** A manifest contributing all five things gives five surfaces. Two
plugins contributing panes give two rail buttons in the group each asked for. Switching one off leaves
the other's contributions untouched. A plugin contributing nothing is refused.

**The dock, with no window.** `dock::regions` of a layout holding a contributed pane on the right
returns the same rectangles the four panel version returned for the other four, and the pane gets what
is left. A contributed pane in the bottom strip is a tile, so showing it puts the terminal away. A
`workspace.conf` written before this change still reads, and its four panel sizes land on the same four
panels.

**Laziness.** A window built with a plugin enabled and its pane never shown records no call to `open`.
Pressing the rail button records exactly one. Pressing it twice records one. Switching the plugin off
records a `close`.

**The look.** A provider is handed a `Look` whose palette is `theme::color`, whose font size is the
setting's, and whose opacity is the setting's. A test changes the opacity setting and asserts the value
the provider was handed changed with it. There is no manifest key that sets a colour, which is asserted
by reading every key the parser knows.

**The window, through the real widget tree.** `egui_kittest` drives a real window: press the rail
button and the pane appears on the right; drag its header to the bottom and it becomes a tile; open the
tab from the menu and the editing area shows it; open `Settings` and the plugin's page is in the list.
Each is asked for the way a person does it rather than by setting a field.

**Screenshots**, accepted after somebody has opened the image and looked at it: the contributed pane
docked right, the same pane as a tile along the bottom, the plugin's tab filling the editing area, the
plugin's menu open with its submenu open inside it, and the plugin's Settings page.

**The command line.** Each of the eight verbs is driven against a real window and its answer checked
against the window's own state read back through `quill-cli status`, rather than against what the
command said it did. `plugin view` is asserted to answer the same numbers the pane is drawing.

## 10. Deliberately not here

**Code that is not in the binary.** A third party UI plugin needs a widget tree and a transport, and
both are their own design. The transport is named: a separate process over the socket
`services::control` already listens on, because that is what Quill does for git, for debug adapters and
for terminals, and because a plugin that crashes should be a plugin that exited. WebAssembly is the
alternative and its limits are recorded in §2.2.

**A widget tree.** VS Code needed one because its extensions cannot call its renderer; Quill's first UI
plugin is Rust in the same binary and does not. Designing a closed list of widgets before anything has
been drawn with it would be designing against a guess. The place it plugs in is already decided: a
provider that is not in the binary returns a tree where a provider in the binary draws, which is the
`Scene` decision `quill-core::mermaid` already made for diagrams.

**Menu anchors inside Quill's own six menus.** VS Code's roughly 40 named anchors with `when`
expressions are the largest part of its contribution model and the hardest part to keep tested. A
plugin gets a menu of its own here. Adding anchors later is a list of names and a condition per anchor,
and it does not change anything in this design.

**Keyboard shortcuts from a manifest.** Two menu items claiming one key equivalent is a real fault on
macOS and there is a test for it. A manifest that could claim a chord could break that test from
outside the repository. A plugin's commands are reachable three other ways.

**A `when` expression language.** §2.4. Two named conditions instead, checked against a list.

**Floating a contributed pane into a window of its own.** A Quill window is a project, and a floating
panel would be a second operating system window with no project behind it. That is true of the four
panels Quill already has and is refused there for the same reason.

**Downloading a plugin.** Nothing in Quill is ever fetched. A plugin is a folder in the settings
folder, and installing one is copying it there.

**Per plugin colour schemes.** §2.4.

**A plugin reading another plugin's contributions.** IntelliJ allows a plugin to extend another
plugin's extension point, and it is where the dependency rules in its dynamic loading restrictions come
from. There is one UI plugin.
