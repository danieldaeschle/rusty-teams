#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbedBounds {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl EmbedBounds {
    pub fn from_logical(x: f32, y: f32, width: f32, height: f32, scale: f32) -> Self {
        let physical = |value: f32| (value * scale).round() as i32;
        EmbedBounds {
            x: physical(x),
            y: physical(y),
            width: physical(width),
            height: physical(height),
        }
    }

    pub fn has_area(self) -> bool {
        self.width > 0 && self.height > 0
    }

    fn right(self) -> i32 {
        self.x + self.width
    }

    fn bottom(self) -> i32 {
        self.y + self.height
    }

    fn within(self, outer: EmbedBounds) -> Option<EmbedBounds> {
        let left = self.x.max(outer.x);
        let top = self.y.max(outer.y);
        let right = self.right().min(outer.right());
        let bottom = self.bottom().min(outer.bottom());
        (right > left && bottom > top).then_some(EmbedBounds {
            x: left,
            y: top,
            width: right - left,
            height: bottom - top,
        })
    }
}

/// Cutouts clipped to the content rectangle, relative to its top-left corner.
pub fn local_cutouts(content: EmbedBounds, cutouts: &[EmbedBounds]) -> Vec<EmbedBounds> {
    cutouts
        .iter()
        .filter_map(|cutout| cutout.within(content))
        .map(|clipped| EmbedBounds {
            x: clipped.x - content.x,
            y: clipped.y - content.y,
            ..clipped
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{EmbedBounds, local_cutouts};

    fn bounds(x: i32, y: i32, width: i32, height: i32) -> EmbedBounds {
        EmbedBounds {
            x,
            y,
            width,
            height,
        }
    }

    #[test]
    fn logical_bounds_scale_to_physical_pixels() {
        assert_eq!(
            EmbedBounds::from_logical(10., 20.5, 100., 50., 1.5),
            bounds(15, 31, 150, 75)
        );
        assert_eq!(
            EmbedBounds::from_logical(10., 20., 100., 50., 1.),
            bounds(10, 20, 100, 50)
        );
    }

    #[test]
    fn a_cutout_inside_the_content_becomes_content_relative() {
        let content = bounds(100, 50, 800, 600);
        assert_eq!(
            local_cutouts(content, &[bounds(500, 400, 200, 100)]),
            vec![bounds(400, 350, 200, 100)]
        );
    }

    #[test]
    fn a_cutout_sticking_out_is_clipped_to_the_content() {
        let content = bounds(100, 50, 800, 600);
        assert_eq!(
            local_cutouts(content, &[bounds(850, 600, 200, 200)]),
            vec![bounds(750, 550, 50, 50)]
        );
        assert_eq!(
            local_cutouts(content, &[bounds(0, 0, 150, 100)]),
            vec![bounds(0, 0, 50, 50)]
        );
    }

    #[test]
    fn cutouts_outside_or_empty_are_dropped() {
        let content = bounds(100, 50, 800, 600);
        assert!(local_cutouts(content, &[bounds(0, 0, 50, 50)]).is_empty());
        assert!(local_cutouts(content, &[bounds(100, 650, 100, 20)]).is_empty());
        assert!(local_cutouts(content, &[bounds(200, 200, 0, 40)]).is_empty());
    }

    #[test]
    fn scaled_cutouts_line_up_with_scaled_content() {
        let scale = 1.25;
        let content = EmbedBounds::from_logical(240., 80., 600., 400., scale);
        let toast = EmbedBounds::from_logical(660., 380., 180., 100., scale);
        assert_eq!(
            local_cutouts(content, &[toast]),
            vec![bounds(525, 375, 225, 125)]
        );
    }

    #[test]
    fn has_area_needs_both_sides() {
        assert!(bounds(0, 0, 1, 1).has_area());
        assert!(!bounds(0, 0, 0, 10).has_area());
        assert!(!bounds(0, 0, 10, 0).has_area());
    }
}
