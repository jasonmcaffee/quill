# agent-tasks-vello-ui — drawing the board with Vello

> Do online research on Vello for rendering graphics in rust.
> Create a agent-tasks-vello-ui-tdd.md tdd.
> Include any plugin architecture changes we need to support this.
>
> We want our agent task plugin UI to look like the ai-service's tasks page, but be dark themed.
>
> It should look nearly identical to the image below.
>
> Have Claude Sol review the pr, visually inspect the ui to ensure it looks like the image, and
> address any issues.

The board drawn as vector graphics rather than as rectangles. `services/vello_canvas.rs` is the
renderer, `services/plugin_ui.rs` grows the seam a plugin reaches it through, and
`_agent_output/task-1765-vello-board/reference-board.png` is the picture it is measured against.

This document is written to be read in the order it is in. §2 to §4 are the research the ticket asks
for and end in one decision. §5 says what that decision buys. §6 and §7 are the design. §8 is the
board, part by part, against the picture. §9 to §12 are the cost, the tests and what is left out.

## 1. What the picture actually is

The board in the reference is the tasks page of `ai-service`, in its dark mode. It is **neumorphic**:
every surface is lit from the top left, so a raised thing carries a pale shadow above and left of it
and a dark one below and right, and a recessed thing carries the same pair *inside* it. The
stylesheet says so in one place, `ui/src/app/neumorphic-tokens.css`, and the dark mode is a pure
token override in `ui/src/app/incognito-theme.css`:

```css
--e-raised:     -6px -6px 14px var(--shadow-light), 6px 6px 14px var(--shadow-dark);
--e-pressed-sm: inset 2px 2px 4px var(--shadow-dark), inset -2px -2px 4px var(--shadow-light);
```

`_agent_output/task-1765-vello-board/reference-measurements.md` is that read back out of the picture
one pixel at a time, and the two agree: a count chip in the reference has six points of dark ramp
inside its top left edge and six points of pale ramp inside its bottom right, which is
`--e-pressed-sm` and nothing else.

**So there is not one flat rectangle with a hard edge in the whole image.** A lane is a rounded box
with 24 points of shadow falling away from it, a card is a smaller one inside it, a well is the same
shape pressed into the surface, the `Add Task` button is a diagonal blue gradient with a blue glow
under it, the agent badge is a violet gradient inside a mint ring, and each lane's dot has a
coloured halo. That list is the whole of the difference between the board Quill ships today and the
picture, and every item on it is a soft shadow, an inset shadow, a gradient or a glow.

That is what makes this a rendering ticket rather than a colour ticket.

## 2. What Vello is

Read from crates.io, docs.rs and the repository on 31 August 2026 rather than remembered.

Vello is Linebender's 2D vector renderer, the same family as `kurbo` (curves), `peniko` (paint),
`skrifa` (fonts) and `parley` (text layout). It is not one crate. It is **three renderers over one
scene model**, and choosing between them is the whole of §3 and §4.

| Crate | Version | What it is | What it depends on |
|---|---|---|---|
| `vello` | 0.10.0, 14 Aug 2026 | The GPU compute renderer. Path processing, binning, coarse rasterisation and fine rasterisation all in WGSL compute shaders. The most work moved off the CPU of the three. The README calls it "experimental". | **wgpu ^29.0.3**, `vello_encoding`, `vello_shaders`, `skrifa`, `peniko` |
| `vello_hybrid` | 0.2.0, 7 Aug 2026 | CPU does the path work and produces sparse strips; the GPU composites them. The README calls this "the primary production-ready implementation". | **wgpu ^29.0.3** *or* WebGL2 through `web-sys` |
| `vello_cpu` | 0.2.0, 7 Aug 2026 | The same sparse-strip pipeline with no GPU at all. SIMD through `fearless_simd`, optional multithreading through `rayon`. `#![forbid(unsafe_code)]`. | `vello_common`, `hashbrown`, `bytemuck`. **No wgpu.** |

All three are `Apache-2.0 OR MIT`, MSRV 1.88 (this machine is on 1.95), and edition 2024. `vello_cpu`
and `vello_common` are `no_std` with an `std` feature on by default.

**And one of them is already inside Quill.** `epaint` 0.36 — the crate egui draws through — depends on
`vello_cpu` 0.1 and rasterises the glyphs in its own font atlas with it. So this is not a question of
whether to add a renderer to the binary; the renderer is in it, and the question is whether the board
may use it. The dependency is therefore pinned to **epaint's version** rather than to the newest, so
there is one copy of it rather than two.

The underlying idea is the **sparse strip** paradigm, written up in the master's thesis the crate's
own documentation links: a path is flattened, the covered area is cut into 4-pixel-tall strips, and
only the strips a path actually touches are rasterised. Empty space costs nothing, which is why a
CPU renderer built this way is competitive at all.

### 2.1 What the API looks like

`vello_cpu` is a retained-nothing immediate context. There are two objects and one call:

```rust
let mut context = RenderContext::new(width, height);      // u16 x u16
let mut resources = Resources::new();
context.set_paint(css::MAGENTA);                          // a colour, a gradient or an image
context.fill_rect(&Rect::from_points((3., 1.), (7., 4.)));
context.flush();
context.render(&mut target, &mut resources);              // target: PixmapMut
```

The parts of it this design uses:

| Call | What it is for here |
|---|---|
| `fill_path`, `stroke_path`, `fill_rect`, `stroke_rect` | Every shape on the board. |
| `fill_blurred_rounded_rect(rect, radius, std_dev, invert)` | **The whole of the elevation.** A real Gaussian blur of a rounded rectangle, and with `invert: true` the documentation says in as many words that it "can be used to implement inset box shadows". |
| `set_paint(PaintType)` where `PaintType = peniko::Brush<Image, Gradient>` | Linear, radial and sweep gradients as a first-class paint. |
| `push_clip_layer(&BezPath)`, `push_opacity_layer`, `push_blend_layer`, `pop_layer` | Clipping a lane's contents to its own rounded corners, and fading a card that is being dragged. |
| `set_transform(Affine)` | Points to physical pixels, once, at the top. |
| `render_with(target, resources, RasterizerSettings)` | `CompositeMode::Replace`, `PixelFormat::Rgba8`. |

The output is a `Pixmap` of `PremulRgba8 { r, g, b, a }`. **That is `egui::Color32`, byte for byte**,
because egui's colour is premultiplied sRGB in exactly that order — so handing a rendered pixmap to
egui is a copy and not a conversion.

## 3. The constraint that decides everything: wgpu 29 against wgpu 30

Quill is an eframe application. `Cargo.toml` pins `egui = "0.36"` and `eframe = "0.36"`, and
`Cargo.lock` resolves that to **wgpu 30.0.1**.

`vello` 0.10 and `vello_hybrid` 0.2 both declare **`wgpu ^29.0.3`**.

Those are semver-incompatible, so cargo compiles both of them into the binary and they are two
different crates as far as the type system is concerned. `wgpu29::Device` and `wgpu30::Device` are
unrelated types; so are their `Queue`, `Texture`, `TextureView` and `CommandEncoder`. A `vello`
renderer built on the first can never write into a texture eframe can read from the second, and
there is no supported way across — the escape hatches that exist (`wgpu-hal`, external memory
handles) are per-backend, unsafe, and not exposed by wgpu's safe API on all three of the platforms
Quill runs on.

Three things follow, and each closes a door:

- **A vello GPU renderer cannot share eframe's device.** Not a matter of effort; a matter of types.
- **`vello_hybrid` does not escape it.** Its two backends are `Renderer` (wgpu 29) and
  `WebGlRenderer` (`web-sys`). It exposes no backend trait a caller could implement over its own
  wgpu, so "give it our wgpu 30 device" is not an option that exists.
- **Waiting is not a plan.** Vello has tracked wgpu closely and will move to 30; egui will then move
  to 31. Two projects on independent release trains agreeing on a wgpu major is a thing that happens
  for a while and then stops, and a design whose first requirement is that two upstreams stay in
  step is a design that breaks on somebody else's release day.

## 4. The four ways in, weighed

### 4.1 `vello` on eframe's own device — impossible today, and unattractive when it is not

The version wall above is the immediate answer. The reason not to reach for it the day the versions
line up is `task-1756`, which is written down in `CLAUDE.md` already: putting a second renderer that
runs its own compute passes on the same device and the same thread as egui, inside a window whose
transparency needed **three** separate fixes to get right on Windows (DX12 named explicitly, a
DirectComposition visual instead of a window handle, and the redirection surface filled every
frame), is a change whose failure mode has historically been a window that draws no frame at all and
says nothing.

Vello's own README calls the GPU renderer experimental, and it requires compute shader support,
which is a capability a machine can lack. A board that draws on some machines and not others is not
a board.

### 4.2 `vello` or `vello_hybrid` on a device of their own — refused

A second wgpu device could be created against a surface of its own and composited by the operating
system, the way `services/browser.rs` puts a native WebView2 child into the window. `task-1756`
measured what that costs and the rules it left behind are in `CLAUDE.md`: **a window has one native
view**, it is created outside the egui pass, and it paints above egui so it has to be hidden
whenever a modal or a popup is open.

A board is not a web page. It is a pane among panes, with modals over it, a scrollbar beside it and
a divider on its edge. Putting it on a native child would mean hiding the entire board every time a
menu opened. Refused.

### 4.3 `vello_hybrid` with its WebGL backend — not applicable

It exists for the browser. Quill is a native window.

### 4.4 `vello_cpu` into a pixmap, and the pixmap into an egui texture — **chosen**

`vello_cpu` has no GPU dependency of any kind, so there is no version to agree on and no second
device. It renders into a `Pixmap` of premultiplied RGBA8, which is uploaded once as an
`egui::TextureHandle` and painted as a single image behind the pane's widgets.

The six reasons it is the right answer here rather than the one that was left:

- **It is already in the binary.** `epaint` 0.36 depends on `vello_cpu` and rasterises its font atlas
  with it, so using it for the board adds no third-party renderer at all — which is not true of either
  of the other two, and is the fact that turns this from a compromise into the obvious answer.

- **It is deterministic.** Quill's testing rests on 345 screenshot comparisons. A GPU renderer's
  output depends on the driver; a CPU renderer's does not. §10 says what has to be pinned for that
  to be true in practice.
- **It cannot fail to be available.** No compute shaders, no adapter, no feature bits. The board
  looks the same on a machine with no usable GPU as on one with a 5090.
- **The upload is a memcpy.** `PremulRgba8` is `Color32`.
- **It is `#![forbid(unsafe_code)]`.** Quill has one `unsafe` island already, in
  `services/windows_transparency.rs`, and it is there because Windows left no alternative.
- **The cost is bounded by what is on the screen, not by what is in the file** — which is
  `task-1666`'s rule. A board's chrome is a few hundred shapes over a pane a few hundred points
  across, and §9 measures it.

The cost is stated rather than hidden. It is CPU time on the frame the board changes, and §9 is the
budget and the example that measures it again. If the board ever needs a hundred times more shapes
than it has, the answer is `vello_hybrid` the day the wgpu versions agree, and nothing above the
`Decor` seam in §6 would change.

## 5. What egui can draw of the picture, and what it cannot

Being fair to egui matters, because "egui cannot do it" is the load-bearing claim.

**It can do more than it looks.** `epaint::RectShape` in 0.36 carries `corner_radius`, `blur_width`
and an optional `brush` — a texture and uv rectangle — and `epaint::Shadow { offset, blur, spread,
color }` turns into one of those. So an outer drop shadow is available, and a gradient can be faked
by stretching a one-dimensional texture across a rectangle.

Here is the honest ledger against the reference:

| The picture needs | egui | Vello |
|---|---|---|
| A rounded rectangle | yes | yes |
| One outer drop shadow | approximately: `blur_width` is a linear ramp, not a Gaussian, and it reads harder at the edge | `fill_blurred_rounded_rect`, a real Gaussian |
| **Two** shadows on one shape, one pale up-left and one dark down-right | two shapes, two ramps | two calls |
| **An inset shadow** — every well, every count chip, the `K` button, the search box | **no.** It needs a rounded-rectangle-with-a-rounded-hole, filled even-odd; `PathShape` fills reliably only convex paths | `fill_blurred_rounded_rect(.., invert: true)`, which is what the argument is for |
| A vertical gradient on a rounded rectangle | a texture brush, one texture per gradient, uv-aligned to the axes | `set_paint(Gradient)` |
| **A diagonal gradient** — the `Add Task` button, the play button, the agent badge | no; the brush's uv rectangle is axis-aligned | any angle, and radial and sweep too |
| **A coloured glow around a circle** — the four lane dots, the blue under `Add Task` | no; `blur_width` is on a rect | a blurred rounded rect with equal sides, or a blurred circle path |
| **Clipping content to rounded corners** | no; `clip_rect` is an axis-aligned `Rect` | `push_clip_layer(path)` |
| Fading a card while it is dragged | per-shape alpha, applied by hand to every colour | `push_opacity_layer` |

Four rows in that table are "no", and they are the four that make the picture look like the picture.
The alternative to a renderer is writing the missing four in `components/` — a Gaussian blur, an
even-odd fill, an arbitrary-angle gradient and a rounded clip — which is a 2D renderer with a
different name on it.

## 6. The seam: `Decor`, then pixels

`quill-core` lays a Mermaid diagram out and `quill-app` draws it, and the seam between them is a
`Scene` of five kinds of item and nothing else. This is the same shape at a smaller size, and for
the same reason: the thing that decides *what the board looks like* should be testable without a
renderer, and the thing that turns it into pixels should know nothing about boards.

### 6.1 `Decor` — what a surface is, not what it is made of

```rust
/// One piece of decoration. Five kinds, which is what the board is built from.
pub enum Decor {
    /// A rounded rectangle, filled solid or with a gradient.
    Rect { rect: Rect, radius: f32, fill: Fill },
    /// A Gaussian shadow of a rounded rectangle, outside it or inside it.
    Shadow { rect: Rect, radius: f32, blur: f32, offset: Vec2, colour: Color32, inset: bool },
    /// A filled circle.
    Disc { centre: Pos2, radius: f32, fill: Fill },
    /// An unfilled circle.
    Ring { centre: Pos2, radius: f32, width: f32, colour: Color32 },
    /// A straight line, which is every divider on the board.
    Line { from: Pos2, to: Pos2, width: f32, colour: Color32 },
    /// Everything until `Unclip` is cut to this rounded rectangle — or, with `outside`, to everything
    /// *outside* it within `bound`, which is the band a raised surface's shadow is drawn in. §9.1.
    Clip { rect: Rect, radius: f32, outside: bool, bound: Rect },
    Unclip,
}

pub enum Fill {
    Solid(Color32),
    /// A linear gradient at any angle, given as the two ends of its axis.
    Linear { from: Pos2, to: Pos2, start: Color32, end: Color32 },
}
```

`Decor` is a plain value: `Clone`, `PartialEq`, and hashable through its bit patterns. That is not
tidiness, it is what §9's caching rests on.

### 6.2 `Chrome` — the recipes, so nothing has to remember the numbers

A component does not build `Decor` values. It says what kind of surface it is drawing, and `Chrome`
knows the elevation recipe:

```rust
impl Chrome<'_> {
    /// A surface standing above the one behind it: a lane, a card, a button.
    pub fn raised(&self, rect: Rect, radius: f32, fill: Fill, lift: Lift);
    /// A surface pressed into the one behind it: a well, a field, a count chip.
    pub fn sunken(&self, rect: Rect, radius: f32, fill: Color32, depth: Lift);
    /// A coloured halo, which is what a lane's dot and the primary button wear.
    pub fn glow(&self, rect: Rect, radius: f32, colour: Color32, spread: f32);
    /// An unfilled circle, for the ring round an attached agent's badge.
    pub fn ring(&self, centre: Pos2, radius: f32, width: f32, colour: Color32);
    /// Everything drawn until the matching `unclip` is cut to this rounded rectangle.
    pub fn clip(&self, rect: Rect, radius: f32);
    pub fn unclip(&self);
}

/// How far off the surface a thing stands. Three, which is what `--e-raised-sm`, `--e-raised` and
/// `--e-raised-lg` are.
pub enum Lift { Small, Medium, Large }
```

`raised` emits a band to draw in, two `Decor::Shadow`s, the matching `Unclip`, and then the `Rect` —
five items, and the band is §9.1's measurement rather than tidiness. `sunken` emits the `Rect`, a clip
to the shape, two inset `Shadow`s and the `Unclip`. The offsets and blurs are the stylesheet's, once,
in one file. A component that wanted a shadow of its own could not ask for one, which is the same rule
as a component that cannot name a colour.

**An inset shadow has to be clipped to its own shape, and that was found by drawing one.**
`fill_blurred_rounded_rect`'s inverted form paints the *complement* of the blur — opaque everywhere
outside the rectangle, fading to nothing inside it — so an unclipped well washes its own colour across
the whole pane. CSS clips an inset shadow to the border box for exactly this reason, and
`an_inset_shadow_darkens_the_inside_of_its_shape_and_not_the_outside` is the test that fails on the
code as it first was.

**The pale shadow is derived, not added to the palette.** The dark-mode stylesheet says it itself:
"in dark neumorphism the 'light' highlight is a lifted gray (not white)". So `Chrome` computes the
pale tone by lifting the surface it is drawn on and the dark tone as black at an alpha. There is no
new hue anywhere in the elevation, which is what keeps `design/style-guide.md`'s closed palette
closed.

### 6.3 `Canvas` — one texture a pane, redrawn only when it changed

```rust
pub struct Canvas {
    context: vello_cpu::RenderContext,
    resources: vello_cpu::Resources,
    pixmap: vello_cpu::Pixmap,
    texture: Option<egui::TextureHandle>,
    /// Everything the texture in hand was made from, kept so it can be compared.
    drawn: Option<Drawn>,
    /// The frame this canvas was last asked for, which is how the store knows it is still wanted.
    last_used: u64,
}

struct Drawn {
    items: Vec<Decor>,
    rect: Rect,
    scale: f32,
    width: u16,
    height: u16,
}
```

A frame goes:

1. The **window** paints the pane's ground — `look.palette.editor` with the opacity applied — and
   **then** reserves a slot in the painter: `let slot = ui.painter().add(egui::Shape::Noop)`. The
   chrome has to be behind the widgets and in front of the ground, and egui hands a painter's shapes
   to the tessellator in the order they were added. A ground painted *after* the slot covers the very
   thing the slot is reserved for, which is what the first version did — and it was invisible rather
   than obviously broken, because the ground carries the window's opacity and let some of the
   decoration through. A provider must not paint one of its own, and `UiProvider::pane` says so.
2. Everything draws as it does now, and each part also tells `Chrome` what surface it is.
3. After the pane, `Canvas::texture_for`:
   - take the decoration's own bounding box, intersected with the pane — §9.3, and it is `task-1666`'s
     rule that a frame costs what is on the screen rather than what is in the file;
   - compare the recorded list, that rectangle and the capped scale against what the texture in hand
     was made from, **without allocating**;
   - if they match, reuse last frame's texture;
   - otherwise replay the list into the `RenderContext`, `render` into the `Pixmap`, upload, and keep
     the list;
   - `ui.painter().set(slot, Shape::image(id, drawn, uv, WHITE))`.

**The list is kept and compared rather than hashed**, and the rectangle and the scale are part of it.
A 64-bit hash makes the comparison cheaper and makes it *wrong* on a collision, which serves stale
pixels; comparing a few hundred `Copy` values costs less than the rasterisation it is deciding to
skip. And two fractional pixel densities can round to the same pixel dimensions and still need
different transforms, while the same shapes in a pane that moved are a different picture — so both are
in the key. `a_board_that_did_not_change_is_not_rasterised_again` drives all four cases through the
real entry point and counts.

**Nothing that runs once a frame may allocate**, which is `task-1666`'s rule. Recording the list is
what *is* paid: a few hundred small `Copy` values into a `Vec`, measured at 0.005 ms. The list is
copied only on the frame it is drawn from.

**The canvas is per pane, keyed on the `egui::Id`** the pane was drawn with, because the board can
be showing in a docked pane and in a tab at once and the two are different widths — which is
`task-1664`'s rule about the layout caches moving onto the tab, made again. `services::vello_canvas::Canvases`
is the small map, owned by `QuillApp`, and a canvas whose id was not drawn this frame is dropped.

## 7. What changes in the plugin architecture

`tasks/ui-plugin-architecture.md` is the design being extended, and the two properties it is built
on are kept exactly:

- **Nothing in a plugin folder is executed.** A manifest names a provider from
  `plugins::UI_PROVIDERS` and the drawing shipped in the binary. Vello changes nothing about that: a
  third-party manifest still cannot draw a rectangle, let alone a gradient.
- **A provider cannot read a setting it was not given.** `Look` is handed over. `Chrome` is handed
  over inside it.

Five changes, and no more.

### 7.1 `Look` gains a `Chrome`

```rust
pub struct Look<'a> {
    ...
    /// Where a provider puts the decoration egui cannot draw.
    ///
    /// A borrow, because the canvas behind it outlives the frame — it is holding last frame's
    /// texture, which is what makes a board that has not changed free.
    pub chrome: &'a Chrome<'a>,
}
```

It goes on `Look` rather than on the trait's method signatures, because `Look` is already "everything
a provider needs to look like the rest of the window" and elevation is exactly that. The four drawing
methods on `UiProvider` — `pane`, `tab`, `settings`, `modal` — keep their signatures.

`Look::of` grows the argument. It is called from the window and from the tests, and a test that does
not care builds `Chrome::detached()`, which records into a list nobody rasterises. That is what lets
every unit test that asserts on `Look` go on running with no window, no graphics card and no fonts.

### 7.2 `Palette` gains the three surfaces the board has and Quill has not

The palette is closed and this does not open it, for the reason the breakpoint red and the four
highlight colours record beside themselves: what is added is named from what is already there.

| New name | Value | Why it is not a new colour |
|---|---|---|
| `board_page` | `EDITOR` | The reference's `#181D24` against Quill's `#1A1F26`. |
| `board_lane` | `EXPLORER` | `#1C222A` against `#1F232A`. |
| `board_card` | `CODE_PANEL` | `#20252E` against `#232933`, and `CODE_PANEL`'s own comment says it is "a step up from `EDITOR` … so the block reads as a panel on the page", which is what a card is. |
| `board_well` | `FIELD` | `#1B2026` against `#1D212A`. |

**The board's blue is the picture's, not Quill's, and this section said the opposite until it was
reviewed twice.** The reference's primary blue is a periwinkle around `#4C6EF5` and Quill's `ACCENT` is an
azure `#489FF8`. The first pass kept the azure, on the grounds that a plugin should look like the rest of
the window and that `nearly identical` could be read as *the same construction* rather than *the same
hue*. The reviewer named it as the most obvious mismatch left in the picture, twice, and that reading does
not survive it: the ticket asked for a board that looks nearly identical to a specific image, and this is
the largest thing in that image that did not.

So `color::BOARD_ACCENT` is the periwinkle, it reaches the `Palette` as `board_accent`, and only the
board reads it — the primary button, the play buttons, the rail's chosen entry and the `IN PROGRESS` dot.
Quill's own `ACCENT` still means *this is where the keyboard is* everywhere including the board, so the
two never say the same thing in two colours.

Three colours in the picture have no name in Quill at all and are added with an argument, exactly as
`BREAKPOINT` was:

| New colour | Value | Why |
|---|---|---|
| `AGENT` | `#9B7CF6` | The violet a card's agent badge is, and the dot on the `AGENT DONE` lane. Quill's palette has a red, an amber, a green, two blues and a pink; it has no violet, and the four lanes have to be four colours a person can tell apart at nine points across. |
| `ATTACHED` | `GIT_ADDED` | The mint ring round a badge whose terminal is running. It is the green Quill already has for "there is something here that was not here before", which is what an attached terminal is. |

### 7.2b Only a pane and a tab get a canvas

`QuillApp::chrome_for` is called from the two places a provider fills a surface of its own — its docked
pane and its tab in the editing area. A provider's **Settings page** and its **modal** are handed a
`Look` with `Chrome::off()`, so they draw flat.

That is deliberate rather than an omission. Both of those live inside furniture that is Quill's:
`components::modal` is the frame every one of the ten modals in the window shares, and the Settings
window is one size with one look for every page. A plugin's page with soft shadows on it would be the
one page in that window that had them, which is the fault `design/style-guide.md` exists to prevent.

### 7.3 A pane says whether it wants a canvas

`plugins::UI_PROVIDERS` is a list of names. A provider that draws no decoration should not pay for a
pixmap the size of its pane. So `UiProvider` gains one method with a default:

```rust
/// Whether this provider draws decoration that needs a canvas behind it.
///
/// False by default, so a provider that only draws with `egui` costs no pixmap, no rasterisation
/// and no texture. Agent-Tasks answers true.
fn draws_chrome(&self) -> bool { false }
```

### 7.4 The manifest says it too, and it is checked

`plugin.conf` gains `ui.chrome = vello`, off unless a manifest asks for it, checked against
`plugins::CHROME` the way `language.renders` is checked against `RENDERERS`, `run.project` against
`PROJECT_RUNNERS` and `debug.adapter` against `DEBUGGERS`. A manifest naming a renderer this version
does not have is refused with a message rather than loading a plugin whose pane is quietly flat.

That is the fifth registry of the same shape, and it buys the property the other four buy: switching
the plugin's chrome off in the manifest really withdraws it, in the same frame, and the board falls
back to the flat drawing — which is also the answer for a machine where the rasteriser is too slow.

### 7.5 A setting, because a person may not want it

`Settings -> Appearance` gains one tick box, `plugins.chrome`, on by default. Off, `Chrome` records
nothing and `Canvas` uploads nothing, and the board draws as it does today. The setting exists for
the same reason the opacity setting two rows above it does: it is the person's window. It is on
Appearance rather than on Plugins because the Plugins page is a list of what is installed and has no
`Settings` behind it, and because depth is an appearance choice in exactly the way the opacity is.

## 8. The board, part by part

Every number below is from `_agent_output/task-1765-vello-board/reference-measurements.md`, and
every one of them is multiplied by `Look::scale()` — the ratio of the editor's font size to the
default — because `task-1683`'s widget and the board's own cards already are, and a board whose
boxes did not grow with 48-point text put a card title through its own footer.

### 8.1 The page

The pane's ground is `look.ground(palette.board_page)` — the palette colour with the window's
opacity applied, which is what stops the board being the one opaque rectangle in a window whose
transparency is the whole character of the product.

**The canvas is transparent where nothing is drawn.** `CompositeMode::Replace` clears the pixmap to
zero, so the chrome texture composites over the pane's own ground and the desktop still shows
through wherever the board draws nothing.

### 8.2 The header

**One row**, which is what the reference has. It was two, because the four views were in it and one row
could not hold all of that at a pane's width; §8.5's rail is where they went.

**`+ Add Task` is always drawn, and the heading is what gives way.** The rail is admitted as soon as 240
points are left for the page, and at that width there is not room for all four things — so the heading is
cut short on one line with an ellipsis and the count is dropped before it. A board somebody cannot add a
ticket to is a broken board; a heading that is cut short is a heading that is cut short.
`the_board_keeps_add_task_at_the_width_the_rail_appears_at` is the test, at exactly that width.

| Part | How it is drawn |
|---|---|
| `Current Sprint` | Text, `TEXT_STRONG`, in the bold face at `font_size * 1.7`. |
| `· 27 tasks` | Text, `TEXT_DIM`, at `font_size - 1`. |
| The search box | `sunken` at a pill radius, `board_well`, 44 points tall, taking **whatever is spare** between the heading and the button up to 460 points. A fixed width left a broad empty band where the reference has its search. |
| `+ Add Task` | 127 by 44, `raised(Lift::Small)` with a `Linear` fill from the accent lightened by 14% to the accent darkened by 22% along the box's own diagonal, and a `glow` of the accent at 42% under it. |

**`Sync JIRA` is absent**, and that is Quill's rule rather than an omission: JIRA is not implemented on
this board — the plugin's own `plugin.limitations` says so — and a control that can never apply is not
drawn. It is the same decision as the `F` button not appearing for a `.rs` file.

### 8.3 A lane

`raised(Lift::Medium)`, radius 18, `board_lane`, 14 points of padding, 22 points between lanes.

**A lane is as tall as what is in it**, which is what the picture shows: an empty lane is a short box
with a well in it and a full one runs to the bottom of the pane. It is never shorter than its heading
plus one card's worth of well, because that empty space is what a card is dropped onto — a lane that
shrank to its heading would be a lane nothing could be moved to. And only the New lane has a foot,
since only it has `+ Add task` in it; the other three end a card's padding after their last card.

The lane header is 57 points tall at the default size and holds three things: a `Disc` 9 across in
the lane's colour with a `glow` of the same colour at 5 points of spread, the lane's name in
`TEXT_DIM` at `font_size - 4` with letter spacing, and a count in a `sunken` chip 40 points wide at
radius 999.

The four lane colours are `TEXT_DIM`, `CLOSE`, `ACCENT` and `AGENT` — grey, red, blue and violet,
against the reference's `#8B95A4`, `#E5484D`, `#5B83FF` and `#9B7CF6`. Only the first is an exact match:
`TEXT_DIM` is `#8B93A3`. `CLOSE` is Quill's own `#FF5F57` and `ACCENT` its own `#489FF8`, both a shade
off the reference's, and that is the palette's rule doing what it is for — see §7.2 on the accent.

An empty lane draws `Nothing here` inside a `sunken` well at radius 14, which is the reference's own
answer and is better than an empty box because it says the lane is empty rather than that the board
failed to draw.

**A lane clips its cards to its own rounded corners.** `chrome.clip(lane, 18.0)` before the cards
and `unclip` after, so a card scrolled to the bottom of a lane is cut by the lane's curve instead of
its own square corner poking out of it. That is the one thing on this list that egui's rectangular
clip cannot do at all.

### 8.4 A card

300 wide by 101 tall at the default size, radius 14, `board_card`, `raised(Lift::Small)`, 24 points
between two.

**Under the pointer the decoration does not change**, and that is a performance rule rather than a taste
one — §9. What changes is a wash of `SELECTED_ROW` painted over it by `egui`, which is the pill every
list in Quill draws for the row it is on, so a card still behaves like a row and the pointer crossing a
board costs nothing.

Inside it, in the order the reference puts them:

| Row | What |
|---|---|
| Title | Two lines at most, `TEXT_STRONG`, wrapped and clipped. |
| Priority | One chevron in `UNSAVED` for high, `TEXT_DIM` for medium, nothing for low. |
| Epic chip | The epic's own colour, which is the one colour on the board that comes from the data. |
| Footer | The key in the code font in `TEXT_DIM`, a tick and `3/7`, a speech mark and a count. |
| Buttons | Play — a `Disc` **30** across with a diagonal `Linear` from the accent and a `glow` under it; the agent badge — a `Disc` **28** across with a diagonal `Linear` in `AGENT`, inside a `Ring` at radius 17.5 and 2 points wide in `ATTACHED` while a terminal is running. Two sizes and not one, because the reference has two: the button you press is the larger of the pair. |

The reference's third round button is its JIRA link, and it is **absent** here for the reason
`Sync JIRA` is: there is no JIRA on this board, and Quill draws no control that cannot apply.

The priority chevron points **up** for high and medium and **low draws nothing at all**. It pointed the
other way, so every card wore a downward mark and the one ticket that mattered least wore the upward one;
and low is now silent, which is the rule the rest of Quill keeps — a mark is drawn to say a thing is
*unusual*, and low is what most tickets are. A downward chevron on every ordinary card says nothing while
taking the place a mark that says something would go.

The epic's colour stays a 3-point edge down the left of the card, drawn as a `Rect` with the card's
radius on its left corners and none on its right, which is what it already is.

### 8.5 The rail

52 points wide at radius 26, `raised(Lift::Medium)` in `board_lane`, as tall as the four buttons in it,
16 points in from the edge of the pane and 24 points clear of the first lane. One icon button a view —
`board`, `stack`, `tick`, `diamond`, all drawn rather than lettered — and the chosen one is a rounded
square filled with the accent gradient and carrying a `glow`, which is what the reference lights its
chosen entry with.

**How far in it sits is asked for rather than written down**, and that is not fussiness: a raised
surface's shadow reaches `Lift::reach()`, which for `Medium` is 25.5 points, and the canvas is cut to the
pane — so a rail closer to the edge than that has its own left shadow clipped and reads as a strip stuck
to the side rather than as a floating rail. It was written down as 16 and it was still clipped by a third,
which an eye did not catch and arithmetic did. The 24 points on its other side are the same measurement
for the first lane, which had the same fault.

**Everything in it scales together.** The height scaled the buttons while the placement scaled the padding
and the gaps as well, so above the default font size the last button hung out of the bottom.

**This is the board's rail and not Quill's.** `components::activity_bar` is the strip on the window's
own edge that puts panels away, and it belongs to the window: a plugin adding entries to it would be a
plugin deciding what the window's furniture is. The reference has a rail *inside* its own page, which
is a different thing — it is how the board switches between its four views — and that is what this is.

**A pane too narrow for a rail and a lane draws no rail**, which is the absent-control rule again, and
the four views stay reachable from the `Agent-Tasks` menu and from `quill-cli`.

## 9. What it costs, measured — and it misses its own budget

`crates/quill-app/examples/vello_cost.rs` records a board of a stated shape, pushes it through the same
`Canvas` the window uses, and prints what each part cost. The raw output of a run, with the machine and
the toolchain, is at `_agent_output/task-1765-vello-board/vello-cost.txt` so the numbers below can be
checked rather than taken.

It is not a test and nothing fails it, for the reason `frame_cost.rs` records — a threshold in
milliseconds is a different number on every machine. What *is* a test is the work itself: how many
`Decor` items a board produces, that the same board twice gives an identical list, and that a frame in
which nothing moved rasterises nothing.

### 9.1 The numbers, and what is not in them

| A board of | Recording, every frame | Rasterising, on a **changed** frame | Asking, on a **still** frame |
|---|---|---|---|
| 4 lanes, 24 cards, 1400 x 900 points | 0.005 ms | **20.7 ms** at 1 pixel a point | 0.001 ms |
| the same, on a display asking for 2 | — | **39.5 ms** at the capped 1.5 | 0.001 ms |
| 4 lanes, 8 cards, 1000 x 700 points | 0.003 ms | **7.9 ms** | 0.000 ms |
| the header and the rail alone | 0.001 ms | 2.0 ms | 0.000 ms |

**The budget this design opened with was a third of a frame at sixty a second, about 5 ms. It is not met,
and the requirement is revised here rather than the number being dressed up.**

A full board being dragged or scrolled on a 1400-point pane spends about one whole frame in the
rasteriser, so the window runs at something like 40 frames a second for as long as the gesture lasts. A
board with a handful of cards on it — which is what most boards are — is 8 ms and does not.

The revised requirement, stated so it can be argued with:

1. **A still frame must be free.** It is: 0.001 ms and no allocation. This is the one that matters most,
   because a window redrawing on a heartbeat spends nearly all of its time here.
2. **A frame where the drawing changed may cost up to a frame**, on a board large enough to fill a wide
   pane, while something is actually moving. That is accepted, not met-by-definition: it is a real cost
   and it is visible as a slower drag on a full board.
3. **It must be possible to say no.** `plugins.chrome` in `Settings -> Appearance` turns the decoration
   off and the board draws flat, at nothing.
4. **The route back to the original number must be written down.** It is §9.4, it is one line in
   `Cargo.toml`, and it is blocked in somebody else's crate rather than in this design.

Anyone who thinks (2) is the wrong trade has (3), and §9.5 is what to build instead.

Three real costs are **outside** those numbers, and the example says so in its own output:

- **The GPU upload.** `TextureHandle::set` queues a texture delta and eframe uploads it later in the
  frame. Nothing here measures that.
- **The rest of the board's frame** — laying the lanes out, filtering the tickets, every galley of text,
  and egui's own tessellation. The *still* figure is the cost of asking for the decoration on a frame
  where nothing changed, not the cost of that frame.
- **The `ColorImage` the raster is copied into**, one allocation of width x height x 4 bytes on every
  changed frame. It is inside the changed-frame figure rather than broken out of it.

### 9.2 What that buys, and when it is paid

It is paid on a frame where the **drawing** changed: a card dragged, a lane scrolled, a letter typed into
the search box, a ticket moved between lanes. It is **not** paid on a hover, which is the commonest thing
anybody does on a board — §8.4's card keeps its elevation and the pointer's answer is a wash `egui`
paints over it. And it is not paid at all on a still frame, which is what a window redrawing on a
heartbeat spends nearly all of its time doing: 0.001 ms, a comparison of two lists.

**And there is an off switch.** `plugins.chrome` in `Settings -> Appearance` turns the decoration off and
the board draws flat, at no cost at all. That is the honest bottom of this section: the depth is worth
the milliseconds or it is not, and the person decides.

### 9.3 Four things were measured and kept

Each was a real number rather than a guess, and the code carries the reason:

- **A raised surface's shadows are cut to the band around it.** The surface is opaque and is painted over
  its own shadows, so every pixel of Gaussian inside it is computed and thrown away. Unclipped, a lane
  328 by 812 cost **3.3 ms on its own**. `Decor::Clip { outside: true }` with an even-odd frame took the
  whole board from **43 ms to 20**.
- **`push_clip_path`, not `push_clip_layer`.** A clip *layer* renders its contents into a layer and
  composites them; a clip *path* is intersected while the strips are generated, so a tile outside it
  costs nothing.
- **The canvas is the decoration's own bounding box, not the pane's.** Rasterising has a floor of about
  two nanoseconds a pixel whether anything is drawn there or not, so an empty canvas over a 1400 by 900
  pane costs 2 ms before a single shape. Lanes that are as tall as their contents leave most of a tall
  pane empty, and that emptiness is now free.
- **The context is resized, never rebuilt.** `RenderContext::new_with` allocates the whole dispatcher, and
  the canvas moves by a point whenever a lane grows, so rebuilding on a size change meant rebuilding on
  nearly every changed frame: **15.3 ms became 20.2**.

Two more were measured and **rejected**, which is worth writing down because both look obviously right:

- **A band each for the two shadows** rather than one shared band, so the pale highlight is evaluated over
  fewer pixels. It cost more, not less: **15.3 ms became 18.9**, because a clip is not free and doubling
  their number outweighed the pixels saved.
- **Clipping only the large surfaces**, on the theory that a card is small enough not to be worth a clip.
  **28.6 ms.** The cards are where the pixels are.

### 9.4 The lever that cannot be pulled, and it is the big one

`vello_cpu` has a `multithreading` feature. Rasterising is per pixel and embarrassingly parallel, and
turning it on took the board from **20.1 ms to 5.5** — inside the budget, in one line.

**It cannot be used.** Cargo features are additive across the whole dependency graph, and `epaint` shares
this crate — so enabling it changes `RenderSettings::default()` for epaint too. epaint's glyph rasteriser
builds its context with the default settings and never calls `flush()`, which the multithreaded
dispatcher requires, so **every screenshot test panicked inside `vello_cpu` with "attempted to rasterize
before flushing" — from egui's own text**, before a single board was drawn.

It comes back the day epaint calls `flush()`, or the day this renderer stops sharing a crate with it.
That is one line in `Cargo.toml` and three quarters of the cost.

### 9.5 The two levers that could be pulled, and why they are not now

- **A sprite cache.** Every card's decoration is identical apart from where it is, so a board of
  twenty-four cards rasterises the same 332 by 132 picture twenty-four times — and §9.1's own table says
  the cards are nearly the whole cost, since the same board without them is 2 ms. Rasterising one card
  once and drawing it as twenty-four textured quads would make *dragging* free, since a drag moves shapes
  without changing any of them. It is a different design — an atlas and one image shape a card instead of
  one texture a pane — and it should be measured against this one rather than assumed better.
- **Rasterising on a thread**, arranged as `services::text_search` and `services::symbol_index` are. It
  was weighed and refused for a reason that shows up on paper: the card's *surface* is in the decoration
  and its *title* is drawn by egui, so a decoration one frame behind is a card whose ground visibly
  separates from its own words while it is dragged. That is worse than a slower frame.

### 9.6 The resolution is capped, and that is a stated trade

`MAX_SCALE` is 1.5. A display at two pixels a point is four times the work, and the same board went from
20 ms to 39 rather than to 80. The decoration's highest-frequency feature is the edge of a rounded
rectangle; everything else in it is a Gaussian or a gradient, which is exactly the content that survives
being drawn a little coarser and filtered up. The **text is not affected at all**, because egui draws the
text at the display's own resolution.

## 10. Determinism, and the screenshot tests

`crates/quill-app/tests/screenshots.rs` compares rendered PNGs against an accepted set, one per
platform. A renderer whose output varies between runs on one machine would make every board
screenshot flake.

`vello_cpu` renders through `fearless_simd`, and `RenderSettings::default()` calls
`Level::try_detect()` — so a machine with AVX-512 and one with SSE4.2 take different code paths. The
same is true across architectures. Whether those paths are bit-identical is not something the crate
promises.

So: **the tests pin the SIMD level.** `Canvas::for_tests()` builds its `RenderContext` with
`RenderSettings { level: Level::baseline(), num_threads: 0 }`, and the released binary uses the
detected level. That makes the accepted images a property of the code rather than of the machine
that ran them, and it costs nothing in the window, where the detected level is wanted.
`QuillApp::draw_deterministically` is how the harness asks for it, called beside `prepare` in every
one of the five places `tests/screenshots.rs` builds a window.

`Level::fallback()` is deliberately not what is pinned: it is **compiled out** on a target whose own
baseline is already above it, so on x86-64 with SSE4.2 statically enabled the variant does not exist.
`Level::baseline()` is the set of features the target itself guarantees, which is the same on every
machine of that target and is therefore the thing to pin.

Quill already keeps a separate accepted set per platform, so a difference that survives that is
caught rather than papered over.

`the_same_drawing_twice_gives_an_identical_list` is the unit test underneath all of it, and it is
`mermaid::check`'s fifth property restated: the images can only rest on the drawing being a pure
function of the state.

## 11. Testing

Four layers, as everything in Quill is.

**`Decor` and `Chrome`, with no window.** `raised` emits five items in the right order — a band, the two
shadows, the unclip and the surface — with the stylesheet's offsets; `sunken` emits its two inset shadows
clipped to its own shape; the pale tone is a lift of the surface and the dark tone is black at an alpha,
so no elevation ever introduces a hue; the same drawing twice gives the same list; and a `Chrome::off()`
records nothing however much is drawn into it.

**`Canvas`, with no window.** A `Decor` list rasterises into a `Pixmap` of the expected size, and
`a_board_that_did_not_change_is_not_rasterised_again` drives the real entry point and counts: ten calls
with the same drawing rasterise once, a changed drawing rasterises, **two pixel densities that round to
the same pixel size** rasterise separately, and so does the same drawing in a pane that moved. An
earlier version of that test rasterised once, wrote a fingerprint into the field by hand, and asserted
the field equalled itself — it passed and proved nothing, which is worth recording because a cache test
that cannot fail is worse than no cache test. `PremulRgba8` and `Color32` are the same four bytes,
asserted rather than assumed; an inset shadow really darkens the inside of its shape and nothing
outside it; a canvas whose surface was not drawn this frame is dropped.

**The provider and the manifest.** `draws_chrome` is true for Agent-Tasks and false for a provider that
has not asked; `Surfaces::chrome_for` answers with the renderer's **name** for the board and nothing for a
language plugin; a manifest naming `ui.chrome = crayons` is refused with the list of what this version
does have; a manifest naming a renderer on a plugin that is not a `ui` plugin is refused too, because a
renderer with no pane to draw is a line that would do nothing silently; and `plugins.chrome` off records
nothing.

**Screenshots.** `agent_tasks_tab` and its siblings are re-accepted after somebody has opened them and
compared them against `reference-board.png`. Three of them are about this design rather than about the
board's behaviour: `agent_tasks_flat`, the whole board with the decoration switched off, which is a
separate path through every part of it; `agent_tasks_narrow_header`, the width the rail is admitted at,
where the header has to give something up; and `a_board_drawn_twice_is_the_same_picture`, which renders a
real board twice and compares the pixels — the `Decor` list being pure is asserted with no window, and
this is the same property all the way through the rasteriser and the upload, which is what the other 414
accepted images quietly rest on.

## 12. Deliberately not here

| Left out | Why |
|---|---|
| **Text drawn by Vello.** | `vello_cpu` takes shaped glyph ids and positions, so drawing the board's words with it means either a second font stack (`parley`, `fontique`, `swash`) beside `fontdb` and `ab_glyph`, or feeding it Quill's own shaping. Either way the text stops being egui text, and with it go selection, hit testing and every existing screenshot. The chrome is what the picture needs; the words already look right. |
| **The editing area, the explorer, the terminal, the menus.** | This is a plugin's pane. Re-skinning the window is a different ticket with a different risk, and `design/style-guide.md` is the document that would have to change first. |
| **A second canvas layer for hover.** | §9's first lever. Measuring comes before optimising. |
| **`multithreading`.** | §9's second lever. |
| **Animation.** | The reference is a still. A shadow that eased on hover would mean asking for a repaint every frame while the pointer is over a card, which is the opposite of the rule that an idle window draws nothing. |
| **A gradient a manifest could name.** | The palette is closed and `ui.chrome` names a renderer, not a colour. |
| **Depth on a plugin's Settings page or its modal.** | §7.2b. Both live inside furniture that is Quill's, and one page with shadows in a window where no other page has them is the fault the style guide exists to prevent. |
| **A card that lifts under the pointer.** | §9. It would re-rasterise the whole pane every time the pointer crossed a card, which is the commonest thing anybody does on a board. The pointer's answer is a wash `egui` paints over the same decoration. |
| **`vello_svg`.** | The board has no SVG in it. The plugin icons are PNGs already. |
