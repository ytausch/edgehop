//! Desktop geometry: which outer edge of the desktop the cursor is at.

use std::fmt;

/// A position in the platform's global desktop coordinates: y grows
/// downwards, and displays left of or above the primary one have negative
/// coordinates.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    Left,
    Right,
    Top,
    Bottom,
}

/// The cursor and the displays, as the platform reports them.
pub trait Desktop {
    /// The cursor position, or `None` when it cannot be read (e.g. while the
    /// screen is locked).
    fn cursor(&self) -> Option<Point>;

    /// Whether any display shows `point`.
    fn contains(&self, point: Point) -> bool;

    /// The first of `edges` that `point` lies on. A point is on an edge of the
    /// desktop when no display continues past it in that direction, so borders
    /// between adjacent displays never count, however the displays are
    /// arranged.
    fn edge_at(&self, point: Point, edges: impl IntoIterator<Item = Edge>) -> Option<Edge> {
        edges
            .into_iter()
            .find(|edge| !self.contains(edge.beyond(point)))
    }
}

impl Edge {
    pub const ALL: [Self; 4] = [Self::Left, Self::Right, Self::Top, Self::Bottom];

    /// The pixel next to `point` in this edge's direction.
    fn beyond(self, Point { x, y }: Point) -> Point {
        match self {
            Self::Left => Point { x: x - 1, y },
            Self::Right => Point { x: x + 1, y },
            Self::Top => Point { x, y: y - 1 },
            Self::Bottom => Point { x, y: y + 1 },
        }
    }

    /// How far `to` lies from `from` away from this edge, into the desktop.
    pub fn inland_distance(self, from: Point, to: Point) -> i32 {
        match self {
            Self::Left => to.x - from.x,
            Self::Right => from.x - to.x,
            Self::Top => to.y - from.y,
            Self::Bottom => from.y - to.y,
        }
    }
}

impl fmt::Display for Edge {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
        })
    }
}

#[cfg(test)]
pub mod fake {
    use std::cell::Cell;

    use super::{Desktop, Point};

    /// Displays as rectangles `(left, top, width, height)`.
    pub struct FakeDesktop {
        pub cursor: Cell<Option<Point>>,
        pub displays: Vec<(i32, i32, i32, i32)>,
    }

    impl FakeDesktop {
        pub fn new(displays: &[(i32, i32, i32, i32)]) -> Self {
            Self {
                cursor: Cell::new(None),
                displays: displays.to_vec(),
            }
        }

        pub fn move_to(&self, x: i32, y: i32) {
            self.cursor.set(Some(Point { x, y }));
        }
    }

    impl Desktop for FakeDesktop {
        fn cursor(&self) -> Option<Point> {
            self.cursor.get()
        }

        fn contains(&self, Point { x, y }: Point) -> bool {
            self.displays.iter().any(|&(left, top, width, height)| {
                (left..left + width).contains(&x) && (top..top + height).contains(&y)
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{fake::FakeDesktop, *};

    /// A 1920x1080 primary display with a smaller 1280x720 one to its left,
    /// aligned at the top.
    fn two_displays() -> FakeDesktop {
        FakeDesktop::new(&[(0, 0, 1920, 1080), (-1280, 0, 1280, 720)])
    }

    fn edge_at(desktop: &FakeDesktop, x: i32, y: i32) -> Option<Edge> {
        desktop.edge_at(Point { x, y }, Edge::ALL)
    }

    #[test]
    fn finds_the_outer_edges() {
        let desktop = two_displays();
        assert_eq!(edge_at(&desktop, -1280, 300), Some(Edge::Left));
        assert_eq!(edge_at(&desktop, 1919, 300), Some(Edge::Right));
        assert_eq!(edge_at(&desktop, 500, 0), Some(Edge::Top));
        assert_eq!(edge_at(&desktop, 500, 1079), Some(Edge::Bottom));
        assert_eq!(edge_at(&desktop, 500, 300), None);
    }

    #[test]
    fn ignores_borders_between_displays() {
        let desktop = two_displays();
        assert_eq!(edge_at(&desktop, 0, 300), None);
        assert_eq!(edge_at(&desktop, -1, 300), None);
    }

    #[test]
    fn counts_exposed_display_sides_as_edges() {
        // Below the smaller display there is nothing, so its bottom is an
        // outer edge, and so is the primary display's left side next to it.
        let desktop = two_displays();
        assert_eq!(edge_at(&desktop, -600, 719), Some(Edge::Bottom));
        assert_eq!(edge_at(&desktop, 0, 900), Some(Edge::Left));
    }

    #[test]
    fn only_considers_the_given_edges() {
        let desktop = two_displays();
        assert_eq!(
            desktop.edge_at(Point { x: 0, y: 0 }, [Edge::Right, Edge::Top]),
            Some(Edge::Top)
        );
        assert_eq!(desktop.edge_at(Point { x: 0, y: 0 }, [Edge::Right]), None);
    }

    #[test]
    fn measures_distance_away_from_each_edge() {
        let from = Point { x: 100, y: 100 };
        let to = Point { x: 130, y: 60 };
        assert_eq!(Edge::Left.inland_distance(from, to), 30);
        assert_eq!(Edge::Right.inland_distance(from, to), -30);
        assert_eq!(Edge::Top.inland_distance(from, to), -40);
        assert_eq!(Edge::Bottom.inland_distance(from, to), 40);
    }

    #[test]
    fn displays_edge_names() {
        let names = Edge::ALL.map(|edge| edge.to_string());
        assert_eq!(names, ["left", "right", "top", "bottom"]);
    }
}
