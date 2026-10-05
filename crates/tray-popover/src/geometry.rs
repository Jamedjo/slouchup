#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

impl Point {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Size {
    pub width: i32,
    pub height: i32,
}

impl Size {
    pub const fn new(width: i32, height: i32) -> Self {
        Self { width, height }
    }
}

/// A rectangle from its top-left corner. The right and bottom edges are exclusive.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    pub const fn at(point: Point) -> Self {
        Self::new(point.x, point.y, 0, 0)
    }

    pub const fn right(&self) -> i32 {
        self.x + self.width
    }

    pub const fn bottom(&self) -> i32 {
        self.y + self.height
    }

    pub const fn centre(&self) -> Point {
        Point::new(self.x + self.width / 2, self.y + self.height / 2)
    }

    pub const fn contains(&self, point: Point) -> bool {
        point.x >= self.x && point.x < self.right() && point.y >= self.y && point.y < self.bottom()
    }

    pub const fn is_empty(&self) -> bool {
        self.width <= 0 || self.height <= 0
    }

    /// How far `point` is outside, squared; 0 inside.
    pub fn distance_squared(&self, point: Point) -> i64 {
        let dx = (self.x - point.x).max(point.x - (self.right() - 1)).max(0) as i64;
        let dy = (self.y - point.y).max(point.y - (self.bottom() - 1)).max(0) as i64;
        dx * dx + dy * dy
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edges_are_exclusive() {
        let rect = Rect::new(10, 20, 100, 50);
        assert!(rect.contains(Point::new(10, 20)));
        assert!(rect.contains(Point::new(109, 69)));
        assert!(!rect.contains(Point::new(110, 20)));
        assert!(!rect.contains(Point::new(10, 70)));
        assert_eq!(rect.centre(), Point::new(60, 45));
    }

    #[test]
    fn distance_is_zero_inside_and_grows_outside() {
        let rect = Rect::new(0, 0, 10, 10);
        assert_eq!(rect.distance_squared(Point::new(5, 5)), 0);
        assert_eq!(rect.distance_squared(Point::new(12, 5)), 9);
        assert_eq!(rect.distance_squared(Point::new(-3, -4)), 25);
    }
}
