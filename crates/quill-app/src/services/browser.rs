//! Rendered web pages, their constrained local origin, and the native views that show them.
//!
//! The tab is ordinary application state and the native browser is not. `BrowserTab` can therefore
//! be tested with no window, while `BrowserHost` owns WebView2 or WKWebView and creates either only
//! when a rendered tab is visible. Local pages are served through a custom origin rooted at the
//! project, so linked assets work without giving page JavaScript a filesystem handle.

use std::collections::{HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use std::time::{Duration, Instant};

use egui::Rect;
use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, CONTROLS};
use url::Url;

use crate::services::file_kind;

/// The browser engines Quill embeds on the platforms it ships.
pub const SUPPORTED: bool = cfg!(any(windows, target_os = "macos"));

/// Where a browser tab was asked to go.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserLocation {
    Local { path: PathBuf, root: PathBuf },
    Remote { url: String },
}

impl BrowserLocation {
    /// Resolve a web address or HTML path against the project and validate it before a tab exists.
    pub fn parse(value: &str, project: &Path) -> Result<Self, String> {
        let value = value.trim();
        if value.is_empty() {
            return Err("Say which HTML file or web address to open.".to_owned());
        }
        let path = Path::new(value);
        if path.is_absolute() || project.join(path).is_file() {
            return Self::local(value, project);
        }
        if let Ok(url) = Url::parse(value) {
            return match url.scheme() {
                "http" | "https" => Ok(Self::Remote { url: url.to_string() }),
                scheme => Err(format!("Quill opens HTTP and HTTPS addresses, not {scheme} addresses.")),
            };
        }
        // `example.com` and `example.com/page` are addresses a person and a model both write with no
        // scheme, and neither is a file in the project. Reading them as a missing file would answer
        // the wrong question; an HTML name that happens to have a dot in it is still read as a file.
        if let Some(url) = implied_address(value) {
            return Ok(Self::Remote { url });
        }
        Self::local(value, project)
    }

    /// Resolve one local HTML file and choose the folder its root-relative resources belong to.
    fn local(value: &str, project: &Path) -> Result<Self, String> {
        let given = PathBuf::from(value);
        let candidate = if given.is_absolute() { given } else { project.join(given) };
        let path = candidate
            .canonicalize()
            .map_err(|problem| format!("Quill could not open {}: {problem}", candidate.display()))?;
        if !path.is_file() || !file_kind::is_html(&path) {
            return Err(format!("{} is not an HTML file.", path.display()));
        }
        let project = project.canonicalize().unwrap_or_else(|_| project.to_path_buf());
        let root = if path.starts_with(&project) {
            project
        } else {
            path.parent().map(Path::to_path_buf).unwrap_or_else(|| path.clone())
        };
        Ok(Self::Local { path, root })
    }

    /// The URL handed to the native browser for this tab.
    pub fn initial_url(&self, id: u64) -> String {
        match self {
            Self::Remote { url } => url.clone(),
            Self::Local { path, root } => local_url(id, path.strip_prefix(root).unwrap_or(path)),
        }
    }

    /// The local file represented by this tab, when it has one.
    pub fn source_path(&self) -> Option<&Path> {
        match self {
            Self::Local { path, .. } => Some(path),
            Self::Remote { .. } => None,
        }
    }
}

/// State that belongs to a rendered tab rather than to its native child view.
///
/// Every rendered tab in a window shares one native view (see [`BrowserHost`]), so where a tab has
/// been is remembered here rather than read back out of the engine: two tabs sharing one view would
/// otherwise share one history, and `Back` on the second would land on the first one's page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BrowserTab {
    pub id: u64,
    pub location: BrowserLocation,
    pub title: String,
    /// Every address this tab has been at, oldest first, with `position` saying which one it is on.
    history: Vec<String>,
    position: usize,
    /// The address this tab has asked the shared view for, until that page arrives.
    awaiting: Option<Awaited>,
    pub loading: bool,
    pub problem: Option<String>,
}

/// An address a tab has asked the shared view for, and where arriving there leaves it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Awaited {
    url: String,
    /// Where in this tab's history the page belongs, when a step asked for it.
    position: Option<usize>,
}

impl BrowserTab {
    /// Create the testable state for a browser tab before a native view is needed.
    pub fn new(id: u64, location: BrowserLocation) -> Self {
        let first = location.initial_url(id);
        Self {
            id,
            title: String::new(),
            location,
            history: vec![first],
            position: 0,
            awaiting: None,
            loading: true,
            problem: None,
        }
    }

    /// The address this tab is showing.
    pub fn current_url(&self) -> &str {
        self.history.get(self.position).map(String::as_str).unwrap_or_default()
    }

    /// Whether this tab has somewhere of its own to go back to.
    pub fn can_go_back(&self) -> bool {
        self.position > 0
    }

    /// Whether this tab has been forward of here.
    pub fn can_go_forward(&self) -> bool {
        self.position + 1 < self.history.len()
    }

    /// The address one step in the given direction, and the position it would leave the tab at.
    ///
    /// `Back` and `Forward` are answered from this tab's own list rather than from the shared view's,
    /// which is what keeps one tab's history out of another's.
    pub fn step(&self, back: bool) -> Option<(usize, String)> {
        let to = match back {
            true => self.position.checked_sub(1)?,
            false => (self.position + 1 < self.history.len()).then_some(self.position + 1)?,
        };
        Some((to, self.history[to].clone()))
    }

    /// Take a step this tab asked for, so the page that arrives is not read as a new address.
    pub fn heading_for(&mut self, position: usize) {
        let url = self.history[position].clone();
        self.awaiting = Some(Awaited { url, position: Some(position) });
        self.loading = true;
    }

    /// Note that the shared view has just been pointed back at this tab's own address.
    ///
    /// Switching tabs sends one view to another page, and the engine reports the page it is leaving
    /// on the way. Without this, the tab being switched *to* would record the tab being switched
    /// *from* as somewhere it had been, and offer a `Back` to a page it had never shown.
    pub fn pointed_at(&mut self) {
        let url = self.current_url().to_owned();
        self.awaiting = Some(Awaited { url, position: None });
        self.loading = true;
    }

    /// Record where a finished page load left this tab.
    ///
    /// While the tab is waiting for an address it asked for, every other page the view reports is the
    /// one it is leaving, and is ignored. Otherwise the address is somewhere new — unless it is
    /// exactly the entry behind or ahead of this one, which is what the engine's own back gesture
    /// inside the page looks like from here.
    pub fn arrived_at(&mut self, url: String) {
        let url = canonical(&url);
        if let Some(awaited) = &self.awaiting {
            if awaited.url != url {
                return;
            }
            let position = awaited.position;
            self.awaiting = None;
            self.loading = false;
            if let Some(position) = position {
                self.position = position;
            }
            return;
        }
        self.loading = false;
        if self.current_url() == url {
            return;
        }
        if self.position > 0 && self.history[self.position - 1] == url {
            self.position -= 1;
            return;
        }
        if self.history.get(self.position + 1).is_some_and(|next| next == &url) {
            self.position += 1;
            return;
        }
        self.history.truncate(self.position + 1);
        self.history.push(url);
        self.position = self.history.len() - 1;
    }

    /// The concise label shown in the file tab strip.
    pub fn name(&self) -> String {
        if !self.title.trim().is_empty() {
            return self.title.clone();
        }
        if let Some(path) = self.location.source_path() {
            return path.file_name().map(|name| name.to_string_lossy().to_string()).unwrap_or_else(|| "Web page".to_owned());
        }
        Url::parse(self.current_url())
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned))
            .unwrap_or_else(|| "Web page".to_owned())
    }
}

/// A browser view that should be visible in this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrowserPlacement {
    pub id: u64,
    pub area: Rect,
    pub focused: bool,
}

/// A command shared by the browser toolbar and `quill-cli browser`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserCommand {
    Back,
    Forward,
    Reload,
}

/// Something the embedded engine reported back to the application.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserEvent {
    LoadStarted { id: u64, url: String },
    LoadFinished { id: u64, url: String },
    Title { id: u64, title: String },
    OpenRequested { source: u64, url: String },
}

/// The one native browser view a window owns, and the local roots its tabs read under.
///
/// **A window has at most one native view, however many rendered tabs are open**, and the view is
/// pointed at whichever tab is showing. That is a platform limit found by measurement on
/// `task-1756`, not a simplification: creating a second WebView2 controller while another view lives
/// on the same thread blocks inside a nested Windows message pump on a completion that never
/// arrives, and the window never draws again — no crash, no error, no way back. It is also the
/// cheaper answer. A tab costs its remembered state and nothing else, and the whole feature costs
/// one browser: measured at 197 MB with no rendered tab open, 521 MB with one, and **521 MB with
/// three**.
pub struct BrowserHost {
    next_id: u64,
    profile: Option<PathBuf>,
    resources: LocalResourceStore,
    /// The tab the one native view is pointed at, shared with the engine's own callbacks.
    showing: Arc<AtomicU64>,
    sender: std::sync::mpsc::Sender<BrowserEvent>,
    receiver: std::sync::mpsc::Receiver<BrowserEvent>,
    native: native::NativeHost,
    last_resource_check: Instant,
}

impl BrowserHost {
    /// A lazy host. Constructing an ordinary Quill window starts no browser process.
    pub fn new() -> Self {
        let (sender, receiver) = std::sync::mpsc::channel();
        Self {
            next_id: 1,
            profile: None,
            resources: LocalResourceStore::new(),
            showing: Arc::new(AtomicU64::new(0)),
            sender,
            receiver,
            native: native::NativeHost::new(),
            last_resource_check: Instant::now(),
        }
    }

    /// Keep browser cookies and caches in Quill's per-user settings folder.
    pub fn set_profile(&mut self, folder: PathBuf) {
        if !self.has_views() {
            self.profile = Some(folder);
        }
    }

    /// Whether the native view exists, which is what decides if there is anything to settle.
    pub fn has_views(&self) -> bool {
        self.native.has_view()
    }

    /// Remember the window a child view is created inside, which is the one thing reconciling needs
    /// from the frame and the one thing it cannot be handed outside the egui pass.
    pub fn remember_window(&mut self, frame: &eframe::Frame) {
        self.native.remember_window(frame);
    }

    /// The tab the native view is pointed at, which is the only one that can be driven.
    pub fn showing(&self) -> Option<u64> {
        match self.showing.load(Ordering::Relaxed) {
            0 => None,
            id => Some(id),
        }
    }

    /// Allocate a stable id and register any local root before the page can request an asset.
    pub fn open_tab(&mut self, location: BrowserLocation) -> BrowserTab {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        if let BrowserLocation::Local { root, .. } = &location {
            self.resources.register(id, root.clone());
        }
        BrowserTab::new(id, location)
    }

    /// Release everything local that belonged to a closed tab, and the view once none are left.
    pub fn close_tab(&mut self, id: u64) {
        self.resources.unregister(id);
        if self.showing() == Some(id) {
            self.showing.store(0, Ordering::Relaxed);
        }
    }

    /// Create, point, place and hide the native view. Called before the egui pass, never inside it.
    pub fn reconcile(&mut self, tabs: &[BrowserTab], placements: &[BrowserPlacement], occluded: bool, repaint: egui::Context) -> Settled {
        let live: HashSet<u64> = tabs.iter().map(|tab| tab.id).collect();
        self.resources.retain(&live);
        if tabs.is_empty() {
            self.native.forget();
            self.showing.store(0, Ordering::Relaxed);
            return Settled::default();
        }
        let chosen = choose(placements, occluded);
        let Some(placement) = chosen else {
            self.native.hide();
            return Settled::default();
        };
        let Some(tab) = tabs.iter().find(|tab| tab.id == placement.id) else { return Settled::default() };
        let was = self.showing();
        let problems = self.native.settle(native::Settle {
            tab,
            placement,
            showing: &self.showing,
            profile: self.profile.clone(),
            resources: self.resources.clone(),
            sender: self.sender.clone(),
            repaint,
        });
        let pointed_at = (was != self.showing()).then(|| self.showing()).flatten();
        Settled { problems, pointed_at }
    }

    /// Send the view to an address on behalf of the tab that is showing.
    pub fn navigate(&self, id: u64, url: &str) -> Result<(), String> {
        self.for_the_showing_tab(id)?;
        self.native.navigate(url)
    }

    /// Reload the page the showing tab is on.
    pub fn reload(&self, id: u64) -> Result<(), String> {
        self.for_the_showing_tab(id)?;
        self.native.reload()
    }

    /// Refuse to drive a tab that the one native view is not pointed at.
    fn for_the_showing_tab(&self, id: u64) -> Result<(), String> {
        match self.showing() == Some(id) {
            true => Ok(()),
            false => Err("That rendered tab is not the one showing.".to_owned()),
        }
    }

    /// Drain browser callbacks at the top of a frame.
    pub fn take_events(&self) -> Vec<BrowserEvent> {
        self.receiver.try_iter().collect()
    }

    /// Reload the local tabs whose previously requested resources changed on disk.
    pub fn reload_changed_local_tabs(&mut self) -> Vec<u64> {
        if self.last_resource_check.elapsed() < Duration::from_millis(500) {
            return Vec::new();
        }
        self.last_resource_check = Instant::now();
        self.resources.changed_tabs()
    }
}

/// What settling the one native view did this frame.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Settled {
    /// Anything the platform could not do, against the tab that asked for it.
    pub problems: Vec<(u64, String)>,
    /// The tab the view was pointed at, when that changed this frame.
    pub pointed_at: Option<u64>,
}

/// The one placement the native view goes to: the pane with the keyboard, else the first drawn.
///
/// A second rendered tab beside the first in a split pane cannot have a view of its own — see
/// [`BrowserHost`] — so it is drawn as a pane that says where its page is.
fn choose(placements: &[BrowserPlacement], occluded: bool) -> Option<&BrowserPlacement> {
    if occluded {
        return None;
    }
    placements.iter().find(|placement| placement.focused).or_else(|| placements.first())
}

impl Default for BrowserHost {
    /// Construct the same lazy host as [`BrowserHost::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// Enough file metadata to notice a changed resource without reading it again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResourceStamp {
    modified: Option<SystemTime>,
    len: u64,
}

impl ResourceStamp {
    /// Measure one existing file.
    fn of(path: &Path) -> Option<Self> {
        let metadata = std::fs::metadata(path).ok()?;
        Some(Self { modified: metadata.modified().ok(), len: metadata.len() })
    }
}

#[derive(Debug, Default)]
struct LocalRoot {
    root: PathBuf,
    resources: HashMap<PathBuf, ResourceStamp>,
}

/// Static resources available to local browser tabs, shared with Wry's protocol callbacks.
#[derive(Debug, Clone)]
struct LocalResourceStore(Arc<Mutex<HashMap<u64, LocalRoot>>>);

impl LocalResourceStore {
    /// An empty registry that opens no file and starts no thread.
    fn new() -> Self {
        Self(Arc::new(Mutex::new(HashMap::new())))
    }

    /// Register the one canonical root a local tab may read under.
    fn register(&self, id: u64, root: PathBuf) {
        let root = root.canonicalize().unwrap_or(root);
        self.0.lock().expect("browser resource registry").insert(id, LocalRoot { root, resources: HashMap::new() });
    }

    /// Forget a tab's root and the bounded list of resources it loaded.
    fn unregister(&self, id: u64) {
        self.0.lock().expect("browser resource registry").remove(&id);
    }

    /// Forget roots whose tabs disappeared through a whole-window state change.
    fn retain(&self, live: &HashSet<u64>) {
        self.0.lock().expect("browser resource registry").retain(|id, _| live.contains(id));
    }

    /// Resolve one custom-origin request without exposing paths outside the registered root.
    fn resolve(&self, id: u64, method: &str, uri: &str) -> ResourceReply {
        if method != "GET" && method != "HEAD" {
            return ResourceReply::empty(405);
        }
        let Some(path) = self.safe_path(id, uri) else {
            return ResourceReply::empty(404);
        };
        let Ok(bytes) = std::fs::read(&path) else {
            return ResourceReply::empty(404);
        };
        self.record(id, &path);
        let mime = mime_guess::from_path(&path).first_or_octet_stream().essence_str().to_owned();
        ResourceReply { status: 200, mime, bytes: if method == "HEAD" { Vec::new() } else { bytes } }
    }

    /// Resolve and canonicalize the URL path, returning nothing for every escape and miss.
    fn safe_path(&self, id: u64, uri: &str) -> Option<PathBuf> {
        let root = self.0.lock().ok()?.get(&id)?.root.clone();
        let url = Url::parse(uri).ok()?;
        let mut relative = PathBuf::new();
        for encoded in url.path().split('/').filter(|part| !part.is_empty()) {
            let part = percent_decode_str(encoded).decode_utf8().ok()?;
            let mut components = Path::new(part.as_ref()).components();
            match (components.next(), components.next()) {
                (Some(Component::Normal(name)), None) => relative.push(name),
                _ => return None,
            }
        }
        let candidate = root.join(relative);
        let candidate = if candidate.is_dir() { candidate.join("index.html") } else { candidate };
        let canonical = candidate.canonicalize().ok()?;
        canonical.starts_with(&root).then_some(canonical)
    }

    /// Remember a served resource so change detection polls only what the page used.
    fn record(&self, id: u64, path: &Path) {
        let Some(stamp) = ResourceStamp::of(path) else { return };
        let Ok(mut roots) = self.0.lock() else { return };
        if let Some(root) = roots.get_mut(&id) {
            root.resources.insert(path.to_path_buf(), stamp);
        }
    }

    /// Return each tab whose loaded resource set changed, updating its stamps once.
    fn changed_tabs(&self) -> Vec<u64> {
        let Ok(mut roots) = self.0.lock() else { return Vec::new() };
        let mut changed = Vec::new();
        for (id, root) in roots.iter_mut() {
            let moved = root.resources.iter_mut().any(|(path, before)| {
                let now = ResourceStamp::of(path);
                let differs = now != Some(*before);
                if let Some(now) = now { *before = now; }
                differs
            });
            if moved { changed.push(*id); }
        }
        changed
    }
}

/// A protocol response independent of Wry, so its security can be tested with no browser runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResourceReply {
    status: u16,
    mime: String,
    bytes: Vec<u8>,
}

impl ResourceReply {
    /// A response with no body, used for misses and refused methods.
    fn empty(status: u16) -> Self {
        Self { status, mime: "text/plain; charset=utf-8".to_owned(), bytes: Vec::new() }
    }
}

/// The address a scheme-less value stands for, when it names a host rather than a file.
fn implied_address(value: &str) -> Option<String> {
    let host = value.split(['/', '?', '#']).next().unwrap_or_default();
    let looks_like_a_host = host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && !host.contains(' ')
        && !host.contains('\\')
        && !file_kind::is_html(Path::new(host));
    let url = looks_like_a_host.then(|| format!("https://{value}"))?;
    Url::parse(&url).ok().filter(|url| url.host_str().is_some_and(|host| host.contains('.'))).map(|url| url.to_string())
}

/// The bytes a path segment cannot carry literally. A dot, a dash and a space-free name are left
/// alone, because the address bar shows this URL and `index%2Ehtml` is a worse answer than
/// `index.html` for the same file.
const SEGMENT: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'/')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'\\')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

/// The one form of an address, whatever the engine calls it.
///
/// WebView2 has no way to serve a scheme of its own, so wry maps `quill://tab-1/page.html` to
/// `http://quill.tab-1/page.html` and reports that back. WKWebView reports the `quill://` form. A tab
/// comparing the two as different addresses would count arriving at its own first page as having
/// gone somewhere new, and would then offer a `Back` to the page it is already on.
fn canonical(url: &str) -> String {
    let Ok(parsed) = Url::parse(url) else { return url.to_owned() };
    let Some(host) = parsed.host_str() else { return url.to_owned() };
    match (parsed.scheme(), host.strip_prefix("quill.")) {
        ("http" | "https", Some(origin)) => format!("quill://{origin}{}", parsed.path()),
        _ => url.to_owned(),
    }
}

/// The name for an address that the engine will actually navigate to.
///
/// The inverse of [`canonical`], and needed for the same reason: WebView2 cannot navigate a scheme of
/// its own, so wry serves `quill://` under `http://quill.<origin>/`. It rewrites the address a view
/// is *built* with, but `WebView::load_url` hands its string straight to `Navigate`, where an unknown
/// scheme is refused in silence — the pane keeps showing the page it was already on while the toolbar
/// says it is loading the new one, which is what pointing the shared view at another tab looked like
/// until this existed.
fn engine_url(url: &str) -> String {
    if !cfg!(windows) {
        return url.to_owned();
    }
    match url.strip_prefix("quill://") {
        Some(rest) => format!("http://quill.{rest}"),
        None => url.to_owned(),
    }
}

/// Build a custom-origin URL whose path retains normal browser-relative semantics.
fn local_url(id: u64, relative: &Path) -> String {
    let path = relative
        .components()
        .filter_map(|component| match component { Component::Normal(name) => Some(name), _ => None })
        .map(|name| utf8_percent_encode(&name.to_string_lossy(), SEGMENT).to_string())
        .collect::<Vec<_>>()
        .join("/");
    format!("quill://tab-{id}/{path}")
}

#[cfg(any(windows, target_os = "macos"))]
mod native {
    use std::borrow::Cow;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    use wry::raw_window_handle::{HandleError, HasWindowHandle, RawWindowHandle, WindowHandle};
    use wry::{NewWindowResponse, PageLoadEvent, PermissionResponse, WebContext, WebView, WebViewBuilder};

    use super::{BrowserEvent, BrowserPlacement, BrowserTab, LocalResourceStore};

    /// Everything needed to settle the native view on one tab for one frame.
    pub struct Settle<'a> {
        pub tab: &'a BrowserTab,
        pub placement: &'a BrowserPlacement,
        pub showing: &'a Arc<AtomicU64>,
        pub profile: Option<PathBuf>,
        pub resources: LocalResourceStore,
        pub sender: std::sync::mpsc::Sender<BrowserEvent>,
        pub repaint: egui::Context,
    }

    /// The window a child view is created inside, remembered by its handle.
    ///
    /// Held rather than borrowed because the view is created before the egui pass, where there is no
    /// `eframe::Frame` to ask. It is the handle eframe itself hands out, and it is used only while
    /// the window that owns this host is alive.
    struct Parent(RawWindowHandle);

    impl HasWindowHandle for Parent {
        /// Borrow the remembered handle for as long as this parent is.
        fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
            // Safety: the handle belongs to the window this host lives in, which outlives the borrow.
            unsafe { Ok(WindowHandle::borrow_raw(self.0)) }
        }
    }

    struct NativeView {
        webview: WebView,
        bounds: Option<wry::Rect>,
        visible: bool,
    }

    /// The platform objects, kept behind this module so the rest of Quill stays portable and testable.
    pub struct NativeHost {
        context: Option<WebContext>,
        view: Option<NativeView>,
        parent: Option<Parent>,
    }

    impl NativeHost {
        /// A host with no browser environment until a tab is first shown.
        pub fn new() -> Self {
            Self { context: None, view: None, parent: None }
        }

        /// Whether the native view exists.
        pub fn has_view(&self) -> bool {
            self.view.is_some()
        }

        /// Take the window's handle while the frame that has one is in scope.
        pub fn remember_window(&mut self, frame: &eframe::Frame) {
            if self.parent.is_none() {
                self.parent = frame.window_handle().ok().map(|handle| Parent(handle.as_raw()));
            }
        }

        /// Drop the view and everything it holds, which is what closing the last rendered tab means.
        pub fn forget(&mut self) {
            self.view = None;
        }

        /// Take the view off the screen and lower its memory target, keeping it ready.
        pub fn hide(&mut self) {
            if let Some(view) = &mut self.view {
                set_visible(view, false);
            }
        }

        /// Create the view if there is none, point it at this tab, and place it in the pane.
        pub fn settle(&mut self, request: Settle<'_>) -> Vec<(u64, String)> {
            let id = request.tab.id;
            if self.view.is_none() {
                if let Err(problem) = self.create(&request) {
                    return vec![(id, problem)];
                }
                request.showing.store(id, Ordering::Relaxed);
            } else if request.showing.load(Ordering::Relaxed) != id {
                // The tab that is showing changed, so the one view follows it to that tab's address.
                request.showing.store(id, Ordering::Relaxed);
                if let Some(view) = &self.view {
                    let _ = view.webview.load_url(&super::engine_url(request.tab.current_url()));
                }
            }
            if let Some(view) = &mut self.view {
                place(view, request.placement);
            }
            Vec::new()
        }

        /// Build the one native view, with no host bridge and callbacks that report browser state.
        ///
        /// The callbacks read which tab the view is pointed at rather than closing over one, because
        /// the view outlives any single tab.
        fn create(&mut self, request: &Settle<'_>) -> Result<(), String> {
            let Some(parent) = &self.parent else {
                return Err("Quill has not finished opening its window yet.".to_owned());
            };
            let context = self.context.get_or_insert_with(|| WebContext::new(request.profile.clone()));
            let repaint = request.repaint.clone();
            let title_sender = request.sender.clone();
            let title_showing = request.showing.clone();
            let load_sender = request.sender.clone();
            let load_showing = request.showing.clone();
            let load_repaint = request.repaint.clone();
            let popup_sender = request.sender.clone();
            let popup_showing = request.showing.clone();
            let resources = request.resources.clone();
            let protocol_showing = request.showing.clone();
            let webview = WebViewBuilder::new_with_web_context(context)
                .with_id("quill-browser")
                .with_url(request.tab.current_url())
                .with_visible(false)
                .with_clipboard(true)
                .with_background_throttling(wry::BackgroundThrottlingPolicy::Throttle)
                .with_permission_handler(|_| PermissionResponse::Deny)
                .with_download_started_handler(|_, _| false)
                .with_navigation_handler(allowed_navigation)
                .with_new_window_req_handler(move |url, _| {
                    let source = popup_showing.load(Ordering::Relaxed);
                    let _ = popup_sender.send(BrowserEvent::OpenRequested { source, url });
                    NewWindowResponse::Deny
                })
                .with_document_title_changed_handler(move |title| {
                    let id = title_showing.load(Ordering::Relaxed);
                    let _ = title_sender.send(BrowserEvent::Title { id, title });
                    repaint.request_repaint();
                })
                .with_on_page_load_handler(move |event, url| {
                    let id = load_showing.load(Ordering::Relaxed);
                    let event = match event {
                        PageLoadEvent::Started => BrowserEvent::LoadStarted { id, url },
                        PageLoadEvent::Finished => BrowserEvent::LoadFinished { id, url },
                    };
                    let _ = load_sender.send(event);
                    load_repaint.request_repaint();
                })
                .with_custom_protocol("quill".to_owned(), move |_, request| {
                    protocol_response(&resources, protocol_showing.load(Ordering::Relaxed), request)
                })
                .build_as_child(parent)
                .map_err(|problem| format!("Quill could not start the browser: {problem}"))?;
            self.view = Some(NativeView { webview, bounds: None, visible: false });
            Ok(())
        }

        /// Send the view to an address, which is what this tab's own history asked for.
        pub fn navigate(&self, url: &str) -> Result<(), String> {
            let view = self.view.as_ref().ok_or_else(|| "The browser tab is not ready yet.".to_owned())?;
            view.webview
                .load_url(&super::engine_url(url))
                .map_err(|problem| format!("The browser could not navigate: {problem}"))
        }

        /// Reload the page the view is on.
        pub fn reload(&self) -> Result<(), String> {
            let view = self.view.as_ref().ok_or_else(|| "The browser tab is not ready yet.".to_owned())?;
            view.webview.reload().map_err(|problem| format!("The browser could not reload: {problem}"))
        }
    }

    /// Allow ordinary web navigation and Quill's one local resource origin.
    fn allowed_navigation(url: String) -> bool {
        Url::parse(&url).ok().is_some_and(|url| matches!(url.scheme(), "http" | "https" | "quill"))
    }

    /// Convert a constrained resource reply into the response Wry expects.
    fn protocol_response(resources: &LocalResourceStore, showing: u64, request: wry::http::Request<Vec<u8>>) -> wry::http::Response<Cow<'static, [u8]>> {
        let reply = resources.resolve(showing, request.method().as_str(), &request.uri().to_string());
        wry::http::Response::builder()
            .status(reply.status)
            .header("Content-Type", reply.mime)
            .header("Cache-Control", "no-store")
            .body(Cow::Owned(reply.bytes))
            .expect("resource response")
    }

    /// Keep the view inside its pane and change native state only when the answer moved.
    fn place(view: &mut NativeView, placement: &BrowserPlacement) {
        let bounds = browser_rect(placement.area);
        if view.bounds != Some(bounds) && view.webview.set_bounds(bounds).is_ok() {
            view.bounds = Some(bounds);
        }
        set_visible(view, true);
        if placement.focused {
            let _ = view.webview.focus();
        }
    }

    /// Show or hide the native child and lower an inactive Windows renderer's memory target.
    fn set_visible(view: &mut NativeView, visible: bool) {
        if view.visible == visible {
            return;
        }
        let _ = view.webview.set_visible(visible);
        #[cfg(windows)]
        {
            use wry::{MemoryUsageLevel, WebViewExtWindows as _};
            let level = if visible { MemoryUsageLevel::Normal } else { MemoryUsageLevel::Low };
            let _ = view.webview.set_memory_usage_level(level);
        }
        view.visible = visible;
    }

    /// Convert egui's logical points to Wry's logical child-window rectangle.
    fn browser_rect(area: egui::Rect) -> wry::Rect {
        wry::Rect {
            position: wry::dpi::LogicalPosition::new(area.left() as f64, area.top() as f64).into(),
            size: wry::dpi::LogicalSize::new(area.width().max(1.0) as f64, area.height().max(1.0) as f64).into(),
        }
    }

    use url::Url;
}

#[cfg(not(any(windows, target_os = "macos")))]
mod native {
    use std::sync::atomic::AtomicU64;
    use std::sync::Arc;

    use super::{BrowserEvent, BrowserPlacement, BrowserTab, LocalResourceStore};

    /// The same input on a platform without an embedded engine.
    pub struct Settle<'a> {
        pub tab: &'a BrowserTab,
        pub placement: &'a BrowserPlacement,
        pub showing: &'a Arc<AtomicU64>,
        pub profile: Option<std::path::PathBuf>,
        pub resources: LocalResourceStore,
        pub sender: std::sync::mpsc::Sender<BrowserEvent>,
        pub repaint: egui::Context,
    }

    /// A portable stub that keeps the workspace compiling and returns one clear refusal.
    pub struct NativeHost;

    const UNSUPPORTED: &str = "Browser tabs are available on Windows and macOS.";

    impl NativeHost {
        /// Construct the no-engine placeholder used on unsupported platforms.
        pub fn new() -> Self { Self }
        /// A placeholder never owns a native view.
        pub fn has_view(&self) -> bool { false }
        /// There is no child window to create, so the window's handle is not wanted.
        pub fn remember_window(&mut self, _frame: &eframe::Frame) {}
        /// There is nothing to drop.
        pub fn forget(&mut self) {}
        /// There is nothing to hide.
        pub fn hide(&mut self) {}
        /// Report one clear platform refusal for the tab that would have been shown.
        pub fn settle(&mut self, request: Settle<'_>) -> Vec<(u64, String)> {
            let _ = (request.placement, request.showing, request.profile, request.resources, request.sender, request.repaint);
            vec![(request.tab.id, UNSUPPORTED.to_owned())]
        }
        /// Refuse navigation where there is no embedded engine.
        pub fn navigate(&self, _url: &str) -> Result<(), String> { Err(UNSUPPORTED.to_owned()) }
        /// Refuse reloading for the same reason.
        pub fn reload(&self) -> Result<(), String> { Err(UNSUPPORTED.to_owned()) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unique folder under the process temp directory for one resource test.
    fn fixture(name: &str) -> PathBuf {
        let folder = std::env::temp_dir().join(format!("quill-browser-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).expect("make browser fixture");
        folder
    }

    #[test]
    fn local_pages_load_linked_resource_types_and_head() {
        let root = fixture("assets");
        std::fs::write(root.join("index.html"), "<link href='site.css'><script src='app.js'></script>").unwrap();
        std::fs::write(root.join("site.css"), "body { color: red; }").unwrap();
        std::fs::write(root.join("app.js"), "document.title = 'ready';").unwrap();
        let store = LocalResourceStore::new();
        store.register(7, root);
        let html = store.resolve(7, "GET", "quill://tab-7/index.html");
        let css = store.resolve(7, "GET", "quill://tab-7/site.css");
        let script = store.resolve(7, "HEAD", "quill://tab-7/app.js");
        assert_eq!((html.status, html.mime.as_str()), (200, "text/html"));
        assert_eq!((css.status, css.mime.as_str()), (200, "text/css"));
        assert_eq!((script.status, script.mime.as_str(), script.bytes.len()), (200, "text/javascript", 0));
    }

    #[test]
    fn local_origin_refuses_traversal_missing_files_and_writes() {
        let root = fixture("security");
        std::fs::write(root.join("index.html"), "ok").unwrap();
        let store = LocalResourceStore::new();
        store.register(3, root);
        assert_eq!(store.resolve(3, "GET", "quill://tab-3/../secret.txt").status, 404);
        assert_eq!(store.resolve(3, "GET", "quill://tab-3/%2E%2E/secret.txt").status, 404);
        assert_eq!(store.resolve(3, "GET", "quill://tab-3/missing.css").status, 404);
        assert_eq!(store.resolve(3, "POST", "quill://tab-3/index.html").status, 405);
    }

    #[test]
    fn local_resource_changes_are_reported_once() {
        let root = fixture("changes");
        let css = root.join("site.css");
        std::fs::write(&css, "a").unwrap();
        let store = LocalResourceStore::new();
        store.register(9, root);
        assert_eq!(store.resolve(9, "GET", "quill://tab-9/site.css").status, 200);
        assert!(store.changed_tabs().is_empty());
        std::fs::write(css, "longer").unwrap();
        assert_eq!(store.changed_tabs(), vec![9]);
        assert!(store.changed_tabs().is_empty());
    }

    /// `task-1756`: an address with no scheme is a host, and every other refusal names its reason.
    #[test]
    fn addresses_and_files_are_told_apart_and_bad_ones_are_refused() {
        let project = fixture("addresses");
        std::fs::write(project.join("index.html"), "<p>ok</p>").unwrap();
        std::fs::write(project.join("notes.md"), "not a page").unwrap();
        let parse = |value: &str| BrowserLocation::parse(value, &project);
        assert_eq!(parse("https://example.com/a").unwrap(), BrowserLocation::Remote { url: "https://example.com/a".to_owned() });
        assert_eq!(parse("example.com/a").unwrap(), BrowserLocation::Remote { url: "https://example.com/a".to_owned() });
        assert!(matches!(parse("index.html").unwrap(), BrowserLocation::Local { .. }));
        assert!(parse("ftp://example.com").unwrap_err().contains("not ftp addresses"));
        assert!(parse("notes.md").unwrap_err().contains("is not an HTML file"));
        assert!(parse("missing.html").unwrap_err().contains("could not open"));
        assert!(parse("   ").unwrap_err().contains("Say which"));
    }

    /// `task-1756`: the address bar shows a readable path, and a name with a space still resolves.
    #[test]
    fn local_addresses_stay_readable_and_still_resolve() {
        let root = fixture("readable");
        let folder = root.join("my site");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("index.html"), "<p>ok</p>").unwrap();
        let location = BrowserLocation::parse(&folder.join("index.html").to_string_lossy(), &root).unwrap();
        let url = location.initial_url(4);
        assert_eq!(url, "quill://tab-4/my%20site/index.html");
        let store = LocalResourceStore::new();
        store.register(4, root);
        assert_eq!(store.resolve(4, "GET", &url).status, 200);
    }

    /// `task-1756`: a closed tab keeps no root, so its origin answers nothing afterwards.
    #[test]
    fn a_closed_tab_releases_its_root_and_its_remembered_resources() {
        let root = fixture("lifecycle");
        std::fs::write(root.join("index.html"), "<p>ok</p>").unwrap();
        let mut host = BrowserHost::new();
        let tab = host.open_tab(BrowserLocation::parse("index.html", &root).unwrap());
        let resources = host.resources.clone();
        assert_eq!(resources.resolve(tab.id, "GET", &format!("quill://tab-{}/index.html", tab.id)).status, 200);
        host.close_tab(tab.id);
        assert_eq!(resources.resolve(tab.id, "GET", &format!("quill://tab-{}/index.html", tab.id)).status, 404);
        assert!(resources.changed_tabs().is_empty());
    }

    /// `task-1756`: an id is never reused, and a tab dropped by a whole-window change is forgotten.
    #[test]
    fn tab_ids_are_unique_and_dropped_tabs_are_retained_away() {
        let root = fixture("retain");
        std::fs::write(root.join("index.html"), "<p>ok</p>").unwrap();
        let mut host = BrowserHost::new();
        let first = host.open_tab(BrowserLocation::parse("index.html", &root).unwrap());
        let second = host.open_tab(BrowserLocation::Remote { url: "https://example.com/".to_owned() });
        assert_ne!(first.id, second.id);
        host.resources.retain(&HashSet::from([second.id]));
        assert_eq!(host.resources.resolve(first.id, "GET", &format!("quill://tab-{}/index.html", first.id)).status, 404);
    }

    /// `task-1756`: the one view goes to the pane with the keyboard, and nowhere while occluded.
    #[test]
    fn one_placement_is_chosen_and_an_occluded_frame_chooses_none() {
        let area = Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::splat(100.0));
        let panes = [BrowserPlacement { id: 1, area, focused: false }, BrowserPlacement { id: 2, area, focused: true }];
        assert_eq!(choose(&panes, false).map(|placement| placement.id), Some(2), "the focused pane holds the view");
        assert_eq!(choose(&panes[..1], false).map(|placement| placement.id), Some(1), "with no focus, the first drawn");
        assert_eq!(choose(&panes, true), None, "an egui surface over the pane takes the view off the screen");
        assert_eq!(choose(&[], false), None);
    }

    /// `task-1756`: a tab switched to ignores the page the shared view is leaving.
    #[test]
    fn a_tab_the_view_is_pointed_back_at_ignores_the_page_it_is_leaving() {
        let mut tab = BrowserTab::new(1, BrowserLocation::Remote { url: "https://example.com/one".to_owned() });
        tab.arrived_at("https://example.com/one".to_owned());
        // The view is sent back to this tab; the engine reports the other tab's page on the way.
        tab.pointed_at();
        tab.arrived_at("https://example.org/somewhere-else".to_owned());
        assert_eq!(tab.current_url(), "https://example.com/one", "the page it is leaving is not this tab's");
        assert!(!tab.can_go_back(), "so it is offered no way back to it");
        assert!(tab.loading, "and the tab is still waiting for its own page");
        tab.arrived_at("https://example.com/one".to_owned());
        assert!(!tab.loading);
        assert!(!tab.can_go_back() && !tab.can_go_forward());
    }

    /// `task-1756`: a tab's history is its own, so `Back` never lands on another tab's page.
    #[test]
    fn each_tab_remembers_where_it_has_been_by_itself() {
        let mut tab = BrowserTab::new(1, BrowserLocation::Remote { url: "https://example.com/one".to_owned() });
        assert!(!tab.can_go_back() && !tab.can_go_forward());
        tab.arrived_at("https://example.com/one".to_owned());
        tab.arrived_at("https://example.com/two".to_owned());
        assert_eq!(tab.current_url(), "https://example.com/two");
        assert!(tab.can_go_back() && !tab.can_go_forward());

        let (position, url) = tab.step(true).expect("somewhere to go back to");
        assert_eq!(url, "https://example.com/one");
        tab.heading_for(position);
        assert!(tab.loading);
        tab.arrived_at(url);
        assert_eq!(tab.current_url(), "https://example.com/one");
        assert!(!tab.loading && !tab.can_go_back() && tab.can_go_forward());

        // Somewhere new from here forgets what was ahead, as every browser's history does.
        tab.arrived_at("https://example.com/three".to_owned());
        assert!(!tab.can_go_forward());
        assert_eq!(tab.step(true).map(|(_, url)| url), Some("https://example.com/one".to_owned()));
    }

    /// `task-1756`: the two names WebView2 and WKWebView give one local address are the same address.
    #[test]
    fn a_local_page_is_one_address_under_either_engines_name() {
        let root = fixture("canonical");
        let tab = BrowserTab::new(1, BrowserLocation::Local { path: root.join("index.html"), root });
        let asked_for = tab.current_url().to_owned();
        assert_eq!(asked_for, "quill://tab-1/index.html");
        let mut tab = tab;
        tab.arrived_at("http://quill.tab-1/index.html".to_owned());
        assert_eq!(tab.current_url(), asked_for, "the engine's own name for it is the same page");
        assert!(!tab.can_go_back(), "so the tab has nowhere behind it to go");
        assert_eq!(canonical("https://example.com/a"), "https://example.com/a");
        // And back again, which is the name the engine is given when a tab is sent to its page.
        if cfg!(windows) {
            assert_eq!(engine_url(&asked_for), "http://quill.tab-1/index.html");
            assert_eq!(canonical(&engine_url(&asked_for)), asked_for);
        }
        assert_eq!(engine_url("https://example.com/a"), "https://example.com/a");
    }

    /// `task-1756`: the engine's own back gesture inside the page is read as a step, not a new page.
    #[test]
    fn a_back_taken_inside_the_page_moves_rather_than_appends() {
        let mut tab = BrowserTab::new(1, BrowserLocation::Remote { url: "https://example.com/one".to_owned() });
        tab.arrived_at("https://example.com/one".to_owned());
        tab.arrived_at("https://example.com/two".to_owned());
        tab.arrived_at("https://example.com/one".to_owned());
        assert_eq!(tab.current_url(), "https://example.com/one");
        assert!(tab.can_go_forward(), "the page it came from is still ahead of it");
        assert!(!tab.can_go_back());
    }

    /// `task-1756`: change detection polls the resources a page asked for and nothing else.
    #[test]
    fn only_the_resources_a_page_requested_are_watched() {
        let root = fixture("watched");
        std::fs::write(root.join("index.html"), "<p>ok</p>").unwrap();
        std::fs::write(root.join("unused.css"), "a").unwrap();
        let store = LocalResourceStore::new();
        store.register(5, root.clone());
        store.resolve(5, "GET", "quill://tab-5/index.html");
        std::fs::write(root.join("unused.css"), "changed").unwrap();
        assert!(store.changed_tabs().is_empty(), "a file the page never asked for is not watched");
        assert_eq!(store.0.lock().unwrap().get(&5).map(|root| root.resources.len()), Some(1));
    }

    #[test]
    fn browser_tab_names_use_title_file_then_host() {
        let root = fixture("names");
        let local = BrowserTab::new(1, BrowserLocation::Local { path: root.join("index.html"), root });
        let remote = BrowserTab::new(2, BrowserLocation::Remote { url: "https://example.com/page".to_owned() });
        assert_eq!(local.name(), "index.html");
        assert_eq!(remote.name(), "example.com");
    }
}
