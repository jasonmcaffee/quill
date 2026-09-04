//! The shapes a node can be, and how each one is drawn and touched.
//!
//! Mermaid's flowchart has fourteen node shapes and its mindmap has six, and every one of them needs
//! the same three answers: how much bigger than its words it has to be, what to draw, and where a
//! line pointing at it should stop. Keeping those three together is what stops a shape being added
//! with a border that fits and an arrow that lands in the middle of the text.

use super::parts::{self, Outline};
use super::scene::{Item, Paint, Point, Rect, Scene, Size, Stroke};

/// What a node looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shape {
    /// `id[text]`
    #[default]
    Rect,
    /// `id(text)`
    Round,
    /// `id([text])`
    Stadium,
    /// `id[[text]]`
    Subroutine,
    /// `id[(text)]`
    Cylinder,
    /// `id((text))`
    Circle,
    /// `id(((text)))`
    DoubleCircle,
    /// `id>text]`
    Asymmetric,
    /// `id{text}`
    Diamond,
    /// `id{{text}}`
    Hexagon,
    /// `id[/text/]`
    Parallelogram,
    /// `id[\text\]`
    ParallelogramAlt,
    /// `id[/text\]`
    Trapezoid,
    /// `id[\text/]`
    TrapezoidAlt,
    /// A mindmap's `id))text((`
    Bang,
    /// A mindmap's `id)text(`
    Cloud,
    /// A state diagram's start or end marker. Drawn from its own code, and here so that a state
    /// diagram can put one in the same list as its ordinary states.
    Marker,
}

impl Shape {
    /// How much room this shape needs beyond its words, across and down.
    ///
    /// A diamond is the expensive one: its corners are empty, so the words have to sit inside a
    /// shape half the area of the box round it, and Mermaid's own diamonds are correspondingly
    /// generous. A circle has to hold its words on a diagonal, which is the same problem again.
    pub fn padding(self) -> Size {
        match self {
            Shape::Rect | Shape::Subroutine => Size::new(24.0, 16.0),
            Shape::Round | Shape::Stadium => Size::new(28.0, 16.0),
            Shape::Cylinder => Size::new(28.0, 30.0),
            Shape::Circle | Shape::DoubleCircle => Size::new(46.0, 40.0),
            Shape::Diamond => Size::new(60.0, 34.0),
            Shape::Hexagon => Size::new(46.0, 16.0),
            Shape::Asymmetric => Size::new(38.0, 16.0),
            Shape::Parallelogram | Shape::ParallelogramAlt => Size::new(46.0, 16.0),
            Shape::Trapezoid | Shape::TrapezoidAlt => Size::new(58.0, 16.0),
            Shape::Bang | Shape::Cloud => Size::new(40.0, 24.0),
            Shape::Marker => Size::new(0.0, 0.0),
        }
    }

    /// True when the shape has to be as wide as it is tall, whatever its words say.
    pub fn is_round(self) -> bool {
        matches!(self, Shape::Circle | Shape::DoubleCircle)
    }

    /// The size a node of this shape holding a label of `label` needs.
    pub fn size_for(self, label: Size) -> Size {
        let padding = self.padding();
        let mut size = Size::new(label.width + padding.width, label.height + padding.height);
        if self.is_round() {
            let across = size.width.max(size.height);
            size = Size::new(across, across);
        }
        size
    }

    /// Where a line pointing at this node should stop.
    pub fn outline(self, rect: Rect) -> Outline {
        match self {
            Shape::Circle | Shape::DoubleCircle | Shape::Bang | Shape::Cloud => {
                Outline::Circle(rect.centre(), rect.width.min(rect.height) / 2.0)
            }
            Shape::Marker => Outline::Circle(rect.centre(), rect.width.max(rect.height) / 2.0),
            Shape::Diamond => Outline::Polygon(diamond(rect)),
            Shape::Hexagon => Outline::Polygon(hexagon(rect)),
            Shape::Parallelogram => Outline::Polygon(parallelogram(rect, false)),
            Shape::ParallelogramAlt => Outline::Polygon(parallelogram(rect, true)),
            Shape::Trapezoid => Outline::Polygon(trapezoid(rect, false)),
            Shape::TrapezoidAlt => Outline::Polygon(trapezoid(rect, true)),
            Shape::Asymmetric => Outline::Polygon(asymmetric(rect)),
            _ => Outline::Rect(rect),
        }
    }

    /// Draw the shape itself, with no words in it.
    pub fn draw(self, scene: &mut Scene, rect: Rect, fill: Paint, stroke: Stroke) {
        let (fill, stroke) = (Some(fill), Some(stroke));
        match self {
            Shape::Rect => scene.add(Item::Rect { rect, radius: 0.0, fill, stroke }),
            Shape::Round => {
                scene.add(Item::Rect { rect, radius: parts::CORNER * 2.0, fill, stroke })
            }
            Shape::Stadium => {
                scene.add(Item::Rect { rect, radius: rect.height / 2.0, fill, stroke })
            }
            Shape::Subroutine => {
                scene.add(Item::Rect { rect, radius: 0.0, fill, stroke });
                // The two inner bars are what makes it read as a call rather than as a step.
                for inset in [8.0, rect.width - 8.0] {
                    scene.add(Item::Line {
                        points: vec![
                            Point::new(rect.x + inset, rect.y),
                            Point::new(rect.x + inset, rect.bottom()),
                        ],
                        stroke: stroke.expect("a stroke was just made"),
                        dash: super::scene::Dash::Solid,
                    });
                }
            }
            Shape::Cylinder => draw_cylinder(scene, rect, fill, stroke),
            Shape::Circle | Shape::Marker => scene.add(Item::Circle {
                centre: rect.centre(),
                radius: rect.width.min(rect.height) / 2.0,
                fill,
                stroke,
            }),
            Shape::DoubleCircle => {
                let radius = rect.width.min(rect.height) / 2.0;
                scene.add(Item::Circle { centre: rect.centre(), radius, fill, stroke });
                scene.add(Item::Circle {
                    centre: rect.centre(),
                    radius: radius - 5.0,
                    fill: None,
                    stroke,
                });
            }
            Shape::Bang | Shape::Cloud => {
                let points = wobbly(rect, if self == Shape::Bang { 10 } else { 12 });
                scene.add(Item::Polygon { points, fill, stroke });
            }
            Shape::Diamond => scene.add(Item::Polygon { points: diamond(rect), fill, stroke }),
            Shape::Hexagon => scene.add(Item::Polygon { points: hexagon(rect), fill, stroke }),
            Shape::Parallelogram => scene.add(Item::Polygon {
                points: parallelogram(rect, false),
                fill,
                stroke,
            }),
            Shape::ParallelogramAlt => scene.add(Item::Polygon {
                points: parallelogram(rect, true),
                fill,
                stroke,
            }),
            Shape::Trapezoid => {
                scene.add(Item::Polygon { points: trapezoid(rect, false), fill, stroke })
            }
            Shape::TrapezoidAlt => {
                scene.add(Item::Polygon { points: trapezoid(rect, true), fill, stroke })
            }
            Shape::Asymmetric => {
                scene.add(Item::Polygon { points: asymmetric(rect), fill, stroke })
            }
        }
    }
}

/// A cylinder: a rectangle with an ellipse at each end, drawn as the top disc and the body.
fn draw_cylinder(scene: &mut Scene, rect: Rect, fill: Option<Paint>, stroke: Option<Stroke>) {
    let lip = (rect.height * 0.18).min(12.0);
    let body = Rect::new(rect.x, rect.y + lip, rect.width, rect.height - lip * 2.0);
    scene.add(Item::Rect { rect: body, radius: 0.0, fill, stroke: None });
    // The two sides, drawn as lines so the discs are not cut across.
    if let Some(stroke) = stroke {
        for x in [rect.left(), rect.right()] {
            scene.add(Item::Line {
                points: vec![Point::new(x, rect.y + lip), Point::new(x, rect.bottom() - lip)],
                stroke,
                dash: super::scene::Dash::Solid,
            });
        }
    }
    for centre in [rect.y + lip, rect.bottom() - lip] {
        scene.add(Item::Polygon {
            points: oval(Point::new(rect.centre().x, centre), rect.width / 2.0, lip),
            fill,
            stroke,
        });
    }
}

/// A flattened ellipse, for the ends of a cylinder.
fn oval(centre: Point, across: f32, down: f32) -> Vec<Point> {
    (0..32)
        .map(|step| {
            let angle = step as f32 / 32.0 * std::f32::consts::TAU;
            Point::new(centre.x + across * angle.cos(), centre.y + down * angle.sin())
        })
        .collect()
}

/// A rough closed outline, for a mindmap's bang and cloud.
///
/// The bumps are worked out from the point number rather than drawn at random, so the same node is
/// the same shape every time it is laid out.
fn wobbly(rect: Rect, bumps: usize) -> Vec<Point> {
    let centre = rect.centre();
    let steps = bumps * 4;
    (0..steps)
        .map(|step| {
            let angle = step as f32 / steps as f32 * std::f32::consts::TAU;
            let wave = 1.0 + 0.12 * (angle * bumps as f32).cos();
            Point::new(
                centre.x + rect.width / 2.0 * wave * angle.cos(),
                centre.y + rect.height / 2.0 * wave * angle.sin(),
            )
        })
        .collect()
}

fn diamond(rect: Rect) -> Vec<Point> {
    let centre = rect.centre();
    vec![
        Point::new(centre.x, rect.top()),
        Point::new(rect.right(), centre.y),
        Point::new(centre.x, rect.bottom()),
        Point::new(rect.left(), centre.y),
    ]
}

fn hexagon(rect: Rect) -> Vec<Point> {
    let notch = (rect.width * 0.16).min(rect.height * 0.6);
    vec![
        Point::new(rect.left() + notch, rect.top()),
        Point::new(rect.right() - notch, rect.top()),
        Point::new(rect.right(), rect.centre().y),
        Point::new(rect.right() - notch, rect.bottom()),
        Point::new(rect.left() + notch, rect.bottom()),
        Point::new(rect.left(), rect.centre().y),
    ]
}

/// A parallelogram leaning right, or left when `back` is true.
fn parallelogram(rect: Rect, back: bool) -> Vec<Point> {
    let lean = (rect.width * 0.16).min(24.0);
    if back {
        vec![
            Point::new(rect.left(), rect.top()),
            Point::new(rect.right() - lean, rect.top()),
            Point::new(rect.right(), rect.bottom()),
            Point::new(rect.left() + lean, rect.bottom()),
        ]
    } else {
        vec![
            Point::new(rect.left() + lean, rect.top()),
            Point::new(rect.right(), rect.top()),
            Point::new(rect.right() - lean, rect.bottom()),
            Point::new(rect.left(), rect.bottom()),
        ]
    }
}

/// A trapezoid narrow at the top, or narrow at the bottom when `back` is true.
fn trapezoid(rect: Rect, back: bool) -> Vec<Point> {
    let lean = (rect.width * 0.18).min(28.0);
    if back {
        vec![
            Point::new(rect.left(), rect.top()),
            Point::new(rect.right(), rect.top()),
            Point::new(rect.right() - lean, rect.bottom()),
            Point::new(rect.left() + lean, rect.bottom()),
        ]
    } else {
        vec![
            Point::new(rect.left() + lean, rect.top()),
            Point::new(rect.right() - lean, rect.top()),
            Point::new(rect.right(), rect.bottom()),
            Point::new(rect.left(), rect.bottom()),
        ]
    }
}

/// A rectangle with a point on its left, which is Mermaid's asymmetric node.
fn asymmetric(rect: Rect) -> Vec<Point> {
    let point = (rect.width * 0.14).min(20.0);
    vec![
        Point::new(rect.left(), rect.top()),
        Point::new(rect.right(), rect.top()),
        Point::new(rect.right(), rect.bottom()),
        Point::new(rect.left(), rect.bottom()),
        Point::new(rect.left() + point, rect.centre().y),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_circle_is_as_wide_as_it_is_tall_whatever_its_words() {
        let size = Shape::Circle.size_for(Size::new(120.0, 20.0));
        assert_eq!(size.width, size.height, "a circle is round: {size:?}");
        assert!(size.width >= 120.0, "and still holds its words");
    }

    #[test]
    fn a_diamond_is_roomier_than_a_rectangle_for_the_same_words() {
        // Its corners are empty, so the same words need a bigger box round them.
        let words = Size::new(80.0, 20.0);
        assert!(Shape::Diamond.size_for(words).width > Shape::Rect.size_for(words).width);
        assert!(Shape::Diamond.size_for(words).height > Shape::Rect.size_for(words).height);
    }

    #[test]
    fn every_shape_gives_a_size_that_holds_its_words() {
        let words = Size::new(90.0, 40.0);
        for shape in [
            Shape::Rect, Shape::Round, Shape::Stadium, Shape::Subroutine, Shape::Cylinder,
            Shape::Circle, Shape::DoubleCircle, Shape::Asymmetric, Shape::Diamond, Shape::Hexagon,
            Shape::Parallelogram, Shape::ParallelogramAlt, Shape::Trapezoid, Shape::TrapezoidAlt,
            Shape::Bang, Shape::Cloud,
        ] {
            let size = shape.size_for(words);
            assert!(size.width >= words.width, "{shape:?} is too narrow");
            assert!(size.height >= words.height, "{shape:?} is too short");
        }
    }

    #[test]
    fn every_shape_draws_something_and_nothing_it_draws_is_outside_its_box() {
        // A shape drawn outside the rectangle the layout gave it would overlap its neighbour, which
        // the layout has no way to know about.
        let rect = Rect::new(10.0, 20.0, 120.0, 50.0);
        for shape in [
            Shape::Rect, Shape::Round, Shape::Stadium, Shape::Subroutine, Shape::Cylinder,
            Shape::Circle, Shape::DoubleCircle, Shape::Asymmetric, Shape::Diamond, Shape::Hexagon,
            Shape::Parallelogram, Shape::ParallelogramAlt, Shape::Trapezoid, Shape::TrapezoidAlt,
            Shape::Marker,
        ] {
            let mut scene = Scene::new();
            shape.draw(
                &mut scene,
                rect,
                Paint::solid(crate::style::Color::WHITE),
                Stroke::new(crate::style::Color::WHITE, 1.0),
            );
            assert!(!scene.is_empty(), "{shape:?} drew nothing");
            for item in &scene.items {
                let bounds = item.bounds();
                assert!(
                    bounds.left() >= rect.left() - 1.0 && bounds.right() <= rect.right() + 1.0,
                    "{shape:?} drew outside its box across: {bounds:?}"
                );
                assert!(
                    bounds.top() >= rect.top() - 1.0 && bounds.bottom() <= rect.bottom() + 1.0,
                    "{shape:?} drew outside its box down: {bounds:?}"
                );
            }
        }
    }

    #[test]
    fn a_line_to_a_diamond_stops_further_in_than_a_line_to_a_rectangle() {
        let rect = Rect::new(0.0, 0.0, 100.0, 60.0);
        let towards = Point::new(200.0, -100.0);
        let square = Shape::Rect.outline(rect).border_towards(towards);
        let diamond = Shape::Diamond.outline(rect).border_towards(towards);
        assert!(
            diamond.distance(rect.centre()) < square.distance(rect.centre()),
            "a diamond's corner is cut off, so the arrow stops sooner"
        );
    }
}
