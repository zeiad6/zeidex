use serde::{Deserialize, Serialize};

/// A point in image pixel space. The origin is the top-left corner.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// An axis-aligned rectangle in image pixel space. `x1 >= x0` and `y1 >= y0`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BBox {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl BBox {
    pub fn new(x0: f32, y0: f32, x1: f32, y1: f32) -> Self {
        Self { x0, y0, x1, y1 }
    }

    pub fn width(&self) -> f32 {
        (self.x1 - self.x0).max(0.0)
    }

    pub fn height(&self) -> f32 {
        (self.y1 - self.y0).max(0.0)
    }

    pub fn area(&self) -> f32 {
        self.width() * self.height()
    }

    pub fn center(&self) -> Point {
        Point {
            x: (self.x0 + self.x1) / 2.0,
            y: (self.y0 + self.y1) / 2.0,
        }
    }

    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.x0 && point.x <= self.x1 && point.y >= self.y0 && point.y <= self.y1
    }

    /// Intersection-over-union, used to drop boxes a layout model and a text
    /// detector both produced.
    pub fn iou(&self, other: &Self) -> f32 {
        let x0 = self.x0.max(other.x0);
        let y0 = self.y0.max(other.y0);
        let x1 = self.x1.min(other.x1);
        let y1 = self.y1.min(other.y1);
        if x1 <= x0 || y1 <= y0 {
            return 0.0;
        }
        let intersection = (x1 - x0) * (y1 - y0);
        let union = self.area() + other.area() - intersection;
        if union <= 0.0 {
            0.0
        } else {
            intersection / union
        }
    }

    pub fn clamp(&self, width: u32, height: u32) -> Self {
        let width = width as f32;
        let height = height as f32;
        Self {
            x0: self.x0.clamp(0.0, width),
            y0: self.y0.clamp(0.0, height),
            x1: self.x1.clamp(0.0, width),
            y1: self.y1.clamp(0.0, height),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iou_is_one_for_identical_boxes() {
        let a = BBox::new(0.0, 0.0, 10.0, 10.0);
        assert!((a.iou(&a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn iou_is_zero_for_disjoint_boxes() {
        let a = BBox::new(0.0, 0.0, 10.0, 10.0);
        let b = BBox::new(20.0, 20.0, 30.0, 30.0);
        assert_eq!(a.iou(&b), 0.0);
    }

    #[test]
    fn clamp_keeps_a_box_inside_the_image() {
        let box_rect = BBox::new(-5.0, 0.0, 120.0, 40.0).clamp(100, 50);
        assert_eq!(box_rect.x0, 0.0);
        assert_eq!(box_rect.x1, 100.0);
        assert_eq!(box_rect.y1, 40.0);
    }

    #[test]
    fn center_and_containment() {
        let box_rect = BBox::new(0.0, 0.0, 10.0, 20.0);
        assert_eq!(box_rect.center(), Point { x: 5.0, y: 10.0 });
        assert!(box_rect.contains(Point { x: 5.0, y: 10.0 }));
        assert!(!box_rect.contains(Point { x: 11.0, y: 10.0 }));
    }
}
