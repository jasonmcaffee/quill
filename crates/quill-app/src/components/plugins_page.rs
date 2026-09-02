//! `Settings -> Plugins`: the marketplace, and what is installed.
//!
//! Laid out like the capture in `tasks/quill-ide-tdd.md`, and built out of the Settings window's own
//! parts, because a second window that looks nearly the same is worse than one that looks the same:
//! a search box, `Marketplace` and `Installed` tabs with a count on the second, the list at the
//! left, and the chosen plugin's own page at the right with its vendor, a button, and what it does
//! and does not do.
//!
//! **The catalogue is the bundled set, and there is no network call.** Fetching a plugin over the
//! network means signature checking, a trust decision and a story about downloaded code, none of
//! which a format that executes nothing has earned yet. The page says so plainly rather than looking
//! like a shop that has run out of stock.

use egui::{CornerRadius, Pos2, Rect, Sense, Stroke, Vec2};

use crate::components::modal;
use crate::services::plugins::{Plugin, Plugins};
use crate::theme::{color, icon, size};

/// How wide the list at the left is.
const LIST: f32 = 300.0;

/// Which of the two tabs is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Marketplace,
    Installed,
}

/// What the page is showing.
#[derive(Debug, Default, Clone)]
pub struct PluginsState {
    pub tab: Tab,
    pub search: String,
    /// The plugin whose page is showing, by id.
    pub chosen: Option<String>,
}

/// What the page asked for.
#[derive(Debug, Default, PartialEq)]
pub struct PluginsOutcome {
    /// Write this plugin out to the settings folder and load it back from there.
    pub install: Option<String>,
    /// Switch a plugin on or off.
    pub set_enabled: Option<(String, bool)>,
}

/// Draw the page into `area`.
pub fn show(
    ui: &mut egui::Ui,
    area: Rect,
    state: &mut PluginsState,
    plugins: &Plugins,
    installed_on_disk: &dyn Fn(&str) -> bool,
    icon_for: &dyn Fn(&str) -> Option<egui::TextureHandle>,
) -> PluginsOutcome {
    let mut outcome = PluginsOutcome::default();
    let header = header(ui, area, state, plugins);
    let list = Rect::from_min_max(
        Pos2::new(area.left() + 20.0, header),
        Pos2::new(area.left() + 20.0 + LIST, area.bottom() - 16.0),
    );
    let page = Rect::from_min_max(
        Pos2::new(list.right() + 20.0, header),
        Pos2::new(area.right() - 20.0, area.bottom() - 16.0),
    );
    let showing = list_of_plugins(ui, list, state, plugins, icon_for);
    if state.chosen.is_none() {
        state.chosen = showing.first().cloned();
    }
    if let Some(id) = state.chosen.clone() {
        if let Some(plugin) = plugins.get(&id) {
            detail(ui, page, plugin, installed_on_disk(&id), icon_for, &mut outcome);
        }
    }
    outcome
}

/// The tabs and the search box. Returns the y everything else starts at.
fn header(ui: &mut egui::Ui, area: Rect, state: &mut PluginsState, plugins: &Plugins) -> f32 {
    let top = area.top() + 16.0;
    let heading = ui.painter().layout_no_wrap(
        "Plugins".to_owned(),
        egui::FontId::proportional(13.5),
        color::text_strong(),
    );
    ui.painter().galley(Pos2::new(area.left() + 20.0, top), heading, color::text_strong());

    let mut pen = area.left() + 120.0;
    for (tab, name) in [(Tab::Marketplace, "Marketplace"), (Tab::Installed, "Installed")] {
        let count = if tab == Tab::Installed { Some(plugins.enabled_count()) } else { None };
        let label = match count {
            Some(count) => format!("{name}  {count}"),
            None => name.to_owned(),
        };
        let width = label.chars().count() as f32 * 7.0 + 26.0;
        let rect = Rect::from_min_size(Pos2::new(pen, top - 4.0), Vec2::new(width, 24.0));
        let response = ui.interact(rect, ui.id().with(("plugins-tab", name)), Sense::click());
        let chosen = state.tab == tab;
        if chosen {
            ui.painter().rect(
                rect,
                CornerRadius::same(size::CONTROL_CORNER),
                color::selected_row(),
                Stroke::new(1.0, color::accent()),
                egui::StrokeKind::Inside,
            );
        } else if response.hovered() {
            ui.painter().rect_filled(rect, CornerRadius::same(size::CONTROL_CORNER), color::control());
        }
        let tint = if chosen { color::text_strong() } else { color::text_control() };
        modal::label(ui.painter(), rect, rect.left() + 12.0, &label, tint, 12.0);
        response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::Button, true, chosen, name));
        if response.clicked() {
            state.tab = tab;
            state.chosen = None;
        }
        pen += width + 8.0;
    }

    let search = Rect::from_min_size(Pos2::new(area.left() + 20.0, top + 30.0), Vec2::new(LIST, 26.0));
    ui.painter().rect(
        search,
        CornerRadius::same(size::CONTROL_CORNER),
        color::field(),
        Stroke::new(1.0, color::control_border()),
        egui::StrokeKind::Inside,
    );
    icon::magnifier(ui.painter(), Pos2::new(search.left() + 13.0, search.center().y), color::text_faint());
    let text_rect = crate::components::controls::field_text_rect(ui, search, 26.0);
    let mut field = ui.new_child(egui::UiBuilder::new().max_rect(text_rect));
    let response = field.add(
        egui::TextEdit::singleline(&mut state.search)
            .hint_text(egui::RichText::new("Search plugins").color(color::text_faint()))
            .frame(egui::Frame::NONE)
            .desired_width(text_rect.width())
            .text_color(color::text_control()),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "Search plugins")
    });
    search.bottom() + 12.0
}

/// The list at the left. Returns the ids it showed, in order.
fn list_of_plugins(
    ui: &mut egui::Ui,
    area: Rect,
    state: &mut PluginsState,
    plugins: &Plugins,
    icon_for: &dyn Fn(&str) -> Option<egui::TextureHandle>,
) -> Vec<String> {
    ui.painter().rect(
        area,
        CornerRadius::same(size::CONTROL_CORNER),
        color::explorer_footer(),
        Stroke::new(1.0, color::divider()),
        egui::StrokeKind::Inside,
    );
    let needle = state.search.trim().to_lowercase();
    let showing: Vec<&Plugin> = plugins
        .all()
        .iter()
        .filter(|plugin| state.tab == Tab::Marketplace || plugin.enabled)
        .filter(|plugin| {
            needle.is_empty()
                || plugin.name.to_lowercase().contains(&needle)
                || plugin.description.to_lowercase().contains(&needle)
        })
        .collect();
    let ids: Vec<String> = showing.iter().map(|plugin| plugin.id.clone()).collect();

    let inner = area.shrink(4.0);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.set_clip_rect(inner);
    let mut chosen = None;
    egui::ScrollArea::vertical().id_salt("plugin-list").show(&mut child, |ui| {
        if showing.is_empty() {
            ui.add_space(8.0);
            ui.label(egui::RichText::new("  Nothing matches.").size(11.5).color(color::text_faint()));
        }
        for plugin in &showing {
            let picked = state.chosen.as_deref() == Some(plugin.id.as_str());
            let name = plugin.name.clone();
            let under = format!("{}  {}", plugin.version, plugin.vendor);
            let enabled = plugin.enabled;
            let picture = icon_for(&plugin.id);
            let label = name.clone();
            let response = row(ui, &plugin.id, &label, picked, move |painter, rect| {
                if let Some(picture) = &picture {
                    crate::services::icons::draw(painter, Pos2::new(rect.left() + 22.0, rect.center().y), picture);
                }
                let tint = if picked { color::text_strong() } else { color::text_control() };
                modal::label(painter, rect, rect.left() + 40.0, &name, tint, 12.5);
                let lower = Rect::from_min_size(
                    Pos2::new(rect.left(), rect.center().y + 8.0),
                    Vec2::new(rect.width(), 14.0),
                );
                modal::label(painter, lower, rect.left() + 40.0, &under, color::text_faint(), 10.5);
                if enabled {
                    let box_rect =
                        Rect::from_center_size(Pos2::new(rect.right() - 20.0, rect.center().y), Vec2::splat(15.0));
                    painter.rect_filled(box_rect, CornerRadius::same(3), color::accent());
                    icon::tick(painter, box_rect.center(), color::text_strong());
                }
            });
            if response.clicked() {
                chosen = Some(plugin.id.clone());
            }
        }
    });
    if let Some(id) = chosen {
        state.chosen = Some(id);
    }
    ids
}

/// A row that is two lines tall, because a plugin shows its name and its version and vendor.
fn row(
    ui: &mut egui::Ui,
    id: &str,
    name: &str,
    chosen: bool,
    draw: impl FnOnce(&egui::Painter, Rect),
) -> egui::Response {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(Vec2::new(width, 44.0), Sense::hover());
    let response = ui.interact(rect, ui.id().with(("plugin-row", id)), Sense::click());
    let pill = rect.shrink2(Vec2::new(6.0, 2.0));
    if chosen {
        ui.painter().rect_filled(pill, CornerRadius::same(5), color::selected_row());
    } else if response.hovered() {
        ui.painter().rect_filled(pill, CornerRadius::same(5), color::control());
    }
    let upper = Rect::from_min_size(rect.min, Vec2::new(rect.width(), 26.0));
    draw(ui.painter(), upper);
    response.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, ui.is_enabled(), chosen, name)
    });
    response
}

/// The chosen plugin's own page.
fn detail(
    ui: &mut egui::Ui,
    area: Rect,
    plugin: &Plugin,
    on_disk: bool,
    icon_for: &dyn Fn(&str) -> Option<egui::TextureHandle>,
    outcome: &mut PluginsOutcome,
) {
    let mut pen = area.top();
    if let Some(picture) = icon_for(&plugin.id) {
        crate::services::icons::draw(ui.painter(), Pos2::new(area.left() + 10.0, pen + 10.0), &picture);
    }
    let title = ui.painter().layout_no_wrap(
        plugin.name.clone(),
        egui::FontId::proportional(16.0),
        color::text_strong(),
    );
    ui.painter().galley(Pos2::new(area.left() + 26.0, pen), title.clone(), color::text_strong());
    pen += title.size().y + 6.0;

    let vendor = format!("{}  \u{00B7}  version {}", plugin.vendor, plugin.version);
    pen = modal::note(ui, area, pen, &vendor);

    // The button. `Install` writes the plugin out to disk and loads it back from there, which is
    // what proves the loader works on real files rather than only on what was baked into Quill.
    let button = Rect::from_min_size(Pos2::new(area.left(), pen + 4.0), Vec2::new(120.0, 28.0));
    if on_disk {
        let word = if plugin.enabled { "DISABLE" } else { "ENABLE" };
        if modal::button(ui, button, word, true, !plugin.enabled) {
            outcome.set_enabled = Some((plugin.id.clone(), !plugin.enabled));
        }
    } else if modal::button(ui, button, "INSTALL", true, true) {
        outcome.install = Some(plugin.id.clone());
    }
    pen = button.bottom() + 10.0;
    let where_from = if on_disk {
        "Installed. Its folder is under the settings folder, and it can be edited by hand."
    } else {
        "Bundled with Quill and already working. Installing writes its folder out where it can be edited."
    };
    pen = modal::note(ui, area, pen, where_from);

    // Everything below the button scrolls, because a plugin that has a lot to say about what it does
    // not do would otherwise have the last of it cut off by the bottom of the window.
    let rest = Rect::from_min_max(Pos2::new(area.left(), pen + 6.0), area.max);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rest));
    child.set_clip_rect(rest);
    egui::ScrollArea::vertical().id_salt("plugin-detail").show(&mut child, |ui| {
        let heading = |ui: &mut egui::Ui, text: &str| {
            ui.add_space(6.0);
            ui.label(egui::RichText::new(text).size(12.5).color(color::text_strong()));
        };
        let note = |ui: &mut egui::Ui, text: String| {
            ui.label(egui::RichText::new(text).size(11.5).color(color::text_faint()));
        };
        heading(ui, "Overview");
        note(ui, plugin.description.clone());
        // What a plugin claims depends on which kind it is: a language claims file types and brings a
        // colour scheme, and a plugin that draws claims neither and adds to the window instead. Saying
        // `Colour scheme: Dracula` about a plugin that names no colours would be a small wrongness a
        // reader notices at once, and it would also be a claim no plugin is allowed to make.
        match plugin.kind {
            crate::services::plugins::Kind::Language => {
                let extensions: Vec<String> =
                    plugin.extensions.iter().map(|extension| format!(".{extension}")).collect();
                note(ui, format!("Claims {}", extensions.join(", ")));
                // The scheme that is actually colouring this language's files, which since `task-1776`
                // is the theme's when the theme names the nine tokens. Naming the plugin's own here
                // while a theme was overriding it would be a small wrongness a reader notices at once,
                // and it is the same rule the `match` above this one keeps about a plugin's kind.
                let scheme = crate::services::plugins::scheme_of(plugin);
                let from = match crate::theme::syntax().is_some() {
                    true => " \u{2014} from the theme, which colours every language at once",
                    false => "",
                };
                note(
                    ui,
                    format!(
                        "Colour scheme: {}{from}. A scheme colours the tokens and not the editing area, so the window still lets the desktop through.",
                        scheme.name
                    ),
                );
            }
            crate::services::plugins::Kind::Ui => {
                note(ui, format!("Adds {}", contributions_of(plugin)));
                note(
                    ui,
                    "It draws with Quill's own controls, so it takes the font and the transparency from Appearance and cannot name a colour of its own.".to_owned(),
                );
            }
            crate::services::plugins::Kind::Theme => {
                let names: Vec<&str> =
                    plugin.themes.iter().map(|theme| theme.name.as_str()).collect();
                note(ui, format!("Carries {}", names.join(", ")));
                note(
                    ui,
                    "A theme says what every name in Quill's own palette means, so the list of colours stays closed while what they are changes. Choose one in Appearance & Behavior -> Theme.".to_owned(),
                );
            }
        }
        if !plugin.limitations.is_empty() {
            heading(ui, "What it does not do");
            note(ui, plugin.limitations.clone());
        }
        heading(ui, "Where plugins come from");
        note(
            ui,
            "These ship with Quill. There is no network marketplace: a plugin is data rather than a program, nothing in one is executed, and fetching code over the network would need a trust decision a format like this has not earned. Writing a folder into the settings folder by hand installs one.".to_owned(),
        );
    });
}

/// What a plugin that draws adds to the window, as a sentence for its own row in the Plugins page.
///
/// Read from the manifest rather than written down here, so a plugin that later contributes a second
/// thing says so with no change to this page.
fn contributions_of(plugin: &crate::services::plugins::Plugin) -> String {
    let mut added: Vec<String> = Vec::new();
    if let Some(pane) = &plugin.contributions.pane {
        added.push(format!("a pane called {} with a button in the rail", pane.label));
    }
    if let Some(tab) = &plugin.contributions.tab {
        added.push(format!("the {} tab in the editing area", tab.label));
    }
    if let Some(menu) = &plugin.contributions.menu {
        added.push(format!("the {} menu", menu.name));
    }
    if plugin.contributions.page.is_some() {
        added.push("a page in these settings".to_owned());
    }
    match added.is_empty() {
        true => "nothing to the window".to_owned(),
        false => format!("{}.", added.join(", ")),
    }
}
