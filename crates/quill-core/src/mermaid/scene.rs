//! What a laid-out diagram is: a list of shapes at absolute positions.
//!
//! `quill-core` has no user interface dependency and cannot name a drawing type, and it should not
//! want to. A diagram, once it has been worked out, is rectangles, circles, polygons, lines and
//! pieces of text at absolute positions — which is a value, not a picture.
//!
//! **Five kinds and nothing else**, which is the point. Everything a diagram is made of is built out
//! of them here, where it can be tested with no window: an arrowhead is a filled polygon of three
//! points, a pie slice is a polygon whose arc has been flattened into segments, a crow's foot is
//! three lines, a dashed relationship is a line with a dash pattern. So the painter in `quill-app`
//! knows nothing about diagrams, and a diagram type added later needs no change there at all.
//!
//! Coordinates are in points with the origin at the top left of the diagram, and [`Scene::size`] is
//! how large the whole thing is. Nothing here knows about a pane: whoever draws it decides where the
//! origin goes and whether to scale it.

use crate::style::Color;

/// A point in the diagram's own coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// The point this far along the straight line to `other`, with `t` from zero to one.
    pub fn towards(self, other: Point, t: f32) -> Point {
        Point::new(self.x + (other.x - self.x) * t, self.y + (other.y - self.y) * t)
    }

    pub fn distance(self, other: Point) -> f32 {
        ((other.x - self.x).powi(2) + (other.y - self.y).powi(2)).sqrt()
    }
}

/// How wide and how tall something is.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub const fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

/// A rectangle, by its top left corner and its size.
///
/// `layout::Rect` is the same four numbers and is what the editor uses; this one is separate because
/// a diagram wants a great many convenience methods on it — the four edges, the centre, growing it
/// by a margin — and putting those on the editor's rectangle would be furnishing one module out of
/// another module's needs.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    /// A rectangle of `size` centred on `centre`.
    pub fn around(centre: Point, size: Size) -> Self {
        Self::new(centre.x - size.width / 2.0, centre.y - size.height / 2.0, size.width, size.height)
    }

    pub fn left(&self) -> f32 {
        self.x
    }

    pub fn right(&self) -> f32 {
        self.x + self.width
    }

    pub fn top(&self) -> f32 {
        self.y
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }

    pub fn centre(&self) -> Point {
        Point::new(self.x + self.width / 2.0, self.y + self.height / 2.0)
    }

    pub fn size(&self) -> Size {
        Size::new(self.width, self.height)
    }

    /// The same rectangle with `by` added on every side.
    pub fn grown(&self, by: f32) -> Rect {
        Rect::new(self.x - by, self.y - by, self.width + by * 2.0, self.height + by * 2.0)
    }

    /// The same rectangle moved.
    pub fn moved(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(self.x + dx, self.y + dy, self.width, self.height)
    }

    pub fn contains(&self, at: Point) -> bool {
        at.x >= self.x && at.x <= self.right() && at.y >= self.y && at.y <= self.bottom()
    }

    /// True when this rectangle and `other` share any area at all.
    pub fn overlaps(&self, other: &Rect) -> bool {
        self.left() < other.right()
            && other.left() < self.right()
            && self.top() < other.bottom()
            && other.top() < self.bottom()
    }

    /// The smallest rectangle holding both.
    pub fn union(&self, other: &Rect) -> Rect {
        let left = self.left().min(other.left());
        let top = self.top().min(other.top());
        Rect::new(left, top, self.right().max(other.right()) - left, self.bottom().max(other.bottom()) - top)
    }
}

/// A colour with an alpha.
///
/// `style::Color` has no alpha, deliberately: text in Quill is always opaque. A diagram does want a
/// wash behind a subgraph and a translucent fill under a radar curve, so it carries its own rather
/// than changing that decision for everyone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paint {
    pub color: Color,
    pub alpha: u8,
}

impl Paint {
    pub const fn solid(color: Color) -> Self {
        Self { color, alpha: 255 }
    }

    pub const fn faded(color: Color, alpha: u8) -> Self {
        Self { color, alpha }
    }
}

/// A line's colour and thickness.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Stroke {
    pub color: Color,
    pub width: f32,
}

impl Stroke {
    pub const fn new(color: Color, width: f32) -> Self {
        Self { color, width }
    }
}

/// Whether a line is solid, and if not, how long the marks and the gaps are.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Dash {
    #[default]
    Solid,
    /// Marks this long with gaps this long.
    Dashed(f32, f32),
}

/// Where a piece of text sits relative to the point it is placed at, across the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Anchor {
    #[default]
    Start,
    Middle,
    End,
}

/// How a piece of text in a diagram looks.
///
/// Not `CharStyle`, because a diagram's text needs none of the underline, the strikethrough or the
/// span machinery, and because what the painter wants is exactly these five things.
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub family: String,
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub color: Color,
}

/// One thing to draw.
#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Rect { rect: Rect, radius: f32, fill: Option<Paint>, stroke: Option<Stroke> },
    Circle { centre: Point, radius: f32, fill: Option<Paint>, stroke: Option<Stroke> },
    /// A closed shape. Filled, stroked, or both.
    Polygon { points: Vec<Point>, fill: Option<Paint>, stroke: Option<Stroke> },
    /// An open path of two or more points. Never filled.
    Line { points: Vec<Point>, stroke: Stroke, dash: Dash },
    /// One line of text. `at` is the left, middle or right of the text's own baseline box, by
    /// `anchor`, and the top of it: the painter puts the top of the line there, as it does
    /// everywhere else in Quill.
    Text { at: Point, text: String, style: TextStyle, anchor: Anchor },
}

impl Item {
    /// The area this item covers, which is what the scene's size is worked out from.
    ///
    /// Text is measured by whoever produced it and reported through [`Scene::claim`], because this
    /// module cannot measure a string. So a `Text` item claims only its own origin here, and the
    /// builder is what widens the scene to hold the words.
    pub fn bounds(&self) -> Rect {
        match self {
            Item::Rect { rect, stroke, .. } => rect.grown(stroke.map_or(0.0, |s| s.width / 2.0)),
            Item::Circle { centre, radius, stroke, .. } => {
                let edge = radius + stroke.map_or(0.0, |s| s.width / 2.0);
                Rect::new(centre.x - edge, centre.y - edge, edge * 2.0, edge * 2.0)
            }
            Item::Polygon { points, stroke, .. } => {
                bounds_of(points).grown(stroke.map_or(0.0, |s| s.width / 2.0))
            }
            Item::Line { points, stroke, .. } => bounds_of(points).grown(stroke.width / 2.0),
            Item::Text { at, .. } => Rect::new(at.x, at.y, 0.0, 0.0),
        }
    }
}

/// The smallest rectangle holding every point.
fn bounds_of(points: &[Point]) -> Rect {
    let Some(first) = points.first() else {
        return Rect::default();
    };
    let mut rect = Rect::new(first.x, first.y, 0.0, 0.0);
    for point in points {
        rect = rect.union(&Rect::new(point.x, point.y, 0.0, 0.0));
    }
    rect
}

/// A diagram, laid out.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Scene {
    /// How large the whole diagram is, in points.
    pub size: Size,
    pub items: Vec<Item>,
}

impl Scene {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add an item, and widen the scene to hold it.
    pub fn add(&mut self, item: Item) {
        self.claim(item.bounds());
        self.items.push(item);
    }

    /// Add every item of `other`, moved by `dx` and `dy`.
    ///
    /// This is what lets a diagram be built out of pieces that were each laid out from their own
    /// origin — a subgraph, a card, a legend — rather than every renderer having to thread an offset
    /// through all of its arithmetic.
    pub fn add_scene(&mut self, other: Scene, dx: f32, dy: f32) {
        for item in other.items {
            self.add(moved(item, dx, dy));
        }
    }

    /// Widen the scene so that `rect` is inside it.
    ///
    /// Called on its own where an item's own bounds are not the whole story: text, whose width this
    /// module cannot measure, and a margin somebody wants left round the edge.
    pub fn claim(&mut self, rect: Rect) {
        self.size.width = self.size.width.max(rect.right());
        self.size.height = self.size.height.max(rect.bottom());
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Every rectangle in the scene, which is what the shared "nothing overlaps" test reads.
    pub fn rects(&self) -> Vec<Rect> {
        self.items
            .iter()
            .filter_map(|item| match item {
                Item::Rect { rect, .. } => Some(*rect),
                _ => None,
            })
            .collect()
    }

    /// Every piece of text in the scene, in the order it was added.
    pub fn texts(&self) -> Vec<&str> {
        self.items
            .iter()
            .filter_map(|item| match item {
                Item::Text { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
}

/// One item, moved.
fn moved(item: Item, dx: f32, dy: f32) -> Item {
    let shift = |point: Point| Point::new(point.x + dx, point.y + dy);
    match item {
        Item::Rect { rect, radius, fill, stroke } => {
            Item::Rect { rect: rect.moved(dx, dy), radius, fill, stroke }
        }
        Item::Circle { centre, radius, fill, stroke } => {
            Item::Circle { centre: shift(centre), radius, fill, stroke }
        }
        Item::Polygon { points, fill, stroke } => {
            Item::Polygon { points: points.into_iter().map(shift).collect(), fill, stroke }
        }
        Item::Line { points, stroke, dash } => {
            Item::Line { points: points.into_iter().map(shift).collect(), stroke, dash }
        }
        Item::Text { at, text, style, anchor } => Item::Text { at: shift(at), text, style, anchor },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rectangle_knows_its_edges_and_its_centre() {
        let rect = Rect::new(10.0, 20.0, 100.0, 40.0);
        assert_eq!((rect.left(), rect.top(), rect.right(), rect.bottom()), (10.0, 20.0, 110.0, 60.0));
        assert_eq!(rect.centre(), Point::new(60.0, 40.0));
        assert_eq!(rect.grown(5.0), Rect::new(5.0, 15.0, 110.0, 50.0));
    }

    #[test]
    fn a_rectangle_around_a_point_is_centred_on_it() {
        let rect = Rect::around(Point::new(50.0, 50.0), Size::new(20.0, 10.0));
        assert_eq!(rect, Rect::new(40.0, 45.0, 20.0, 10.0));
        assert_eq!(rect.centre(), Point::new(50.0, 50.0));
    }

    #[test]
    fn two_rectangles_that_touch_along_an_edge_do_not_overlap() {
        // A layout that puts one node's left edge exactly on another's right edge is correct, so the
        // overlap test every diagram type is run through must not call it a fault.
        let left = Rect::new(0.0, 0.0, 50.0, 20.0);
        let right = Rect::new(50.0, 0.0, 50.0, 20.0);
        assert!(!left.overlaps(&right));
        assert!(left.overlaps(&Rect::new(49.0, 0.0, 50.0, 20.0)));
    }

    #[test]
    fn adding_an_item_widens_the_scene_to_hold_it() {
        let mut scene = Scene::new();
        scene.add(Item::Rect {
            rect: Rect::new(10.0, 10.0, 80.0, 30.0),
            radius: 0.0,
            fill: None,
            stroke: None,
        });
        assert_eq!(scene.size, Size::new(90.0, 40.0));
    }

    #[test]
    fn a_stroke_counts_half_its_width_towards_the_size() {
        // Otherwise the outer half of the border of the rightmost box is drawn outside the scene and
        // is clipped away, which is visible as a box missing one edge.
        let mut scene = Scene::new();
        scene.add(Item::Rect {
            rect: Rect::new(0.0, 0.0, 100.0, 20.0),
            radius: 0.0,
            fill: None,
            stroke: Some(Stroke::new(Color::WHITE, 4.0)),
        });
        assert_eq!(scene.size, Size::new(102.0, 22.0));
    }

    #[test]
    fn a_scene_added_to_another_is_moved_whole() {
        let mut inner = Scene::new();
        inner.add(Item::Circle {
            centre: Point::new(10.0, 10.0),
            radius: 5.0,
            fill: None,
            stroke: None,
        });
        inner.add(Item::Text {
            at: Point::new(0.0, 0.0),
            text: "a".to_owned(),
            style: TextStyle {
                family: "Helvetica".to_owned(),
                size: 12.0,
                bold: false,
                italic: false,
                color: Color::WHITE,
            },
            anchor: Anchor::Start,
        });
        let mut outer = Scene::new();
        outer.add_scene(inner, 100.0, 50.0);
        match &outer.items[0] {
            Item::Circle { centre, .. } => assert_eq!(*centre, Point::new(110.0, 60.0)),
            other => panic!("expected the circle, found {other:?}"),
        }
        match &outer.items[1] {
            Item::Text { at, .. } => assert_eq!(*at, Point::new(100.0, 50.0)),
            other => panic!("expected the text, found {other:?}"),
        }
    }

    #[test]
    fn a_point_can_be_walked_towards_another() {
        let from = Point::new(0.0, 0.0);
        let to = Point::new(10.0, 20.0);
        assert_eq!(from.towards(to, 0.5), Point::new(5.0, 10.0));
        assert_eq!(from.towards(to, 0.0), from);
        assert_eq!(from.towards(to, 1.0), to);
    }
}
