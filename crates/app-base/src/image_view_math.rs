use std::ops::Range;

pub type Pair = (f32, f32);

pub const MAX_ZOOM: f32 = 4.;
pub const ZOOM_STEP: f32 = 1.25;
const WHEEL_SENSITIVITY: f32 = 0.01;
const SCALE_EPSILON: f32 = 0.001;

pub fn rotated_size(size: Pair, quarter_turns: u8) -> Pair {
    if quarter_turns % 2 == 0 {
        size
    } else {
        (size.1, size.0)
    }
}

pub fn next_rotation(quarter_turns: u8) -> u8 {
    (quarter_turns + 1) % 4
}

pub fn fit_scale(image: Pair, stage: Pair) -> f32 {
    if image.0 <= 0. || image.1 <= 0. || stage.0 <= 0. || stage.1 <= 0. {
        return 1.;
    }
    (stage.0 / image.0).min(stage.1 / image.1).min(1.)
}

pub fn clamp_scale(scale: f32, fit: f32) -> f32 {
    scale.clamp(fit, MAX_ZOOM.max(fit))
}

pub fn display_size(image: Pair, scale: f32) -> Pair {
    (image.0 * scale, image.1 * scale)
}

pub fn clamp_pan(pan: Pair, display: Pair, stage: Pair) -> Pair {
    let limit = |display: f32, stage: f32| ((display - stage) / 2.).max(0.);
    let (limit_x, limit_y) = (limit(display.0, stage.0), limit(display.1, stage.1));
    (
        pan.0.clamp(-limit_x, limit_x),
        pan.1.clamp(-limit_y, limit_y),
    )
}

pub fn image_origin(display: Pair, stage: Pair, pan: Pair) -> Pair {
    (
        (stage.0 - display.0) / 2. + pan.0,
        (stage.1 - display.1) / 2. + pan.1,
    )
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct View {
    pub scale: f32,
    pub pan: Pair,
}

pub fn zoom_at(view: View, wanted: f32, anchor: Pair, image: Pair, stage: Pair) -> View {
    let fit = fit_scale(image, stage);
    let scale = clamp_scale(wanted, fit);
    let ratio = scale / view.scale;
    let pan = (
        anchor.0 - (anchor.0 - view.pan.0) * ratio,
        anchor.1 - (anchor.1 - view.pan.1) * ratio,
    );
    View {
        scale,
        pan: clamp_pan(pan, display_size(image, scale), stage),
    }
}

pub fn wheel_scale(scale: f32, delta_y: f32) -> f32 {
    scale * (delta_y * WHEEL_SENSITIVITY).exp()
}

pub fn step_scale(scale: f32, direction: i32) -> f32 {
    scale * ZOOM_STEP.powi(direction)
}

pub fn toggle_target(scale: f32, fit: f32) -> f32 {
    if scale > fit + SCALE_EPSILON {
        fit
    } else {
        1_f32.max(fit)
    }
}

pub fn percent_label(scale: f32) -> String {
    format!("{}%", (scale * 100.).round() as u32)
}

pub fn step_index(current: usize, len: usize, delta: isize) -> usize {
    if len == 0 {
        return 0;
    }
    current.saturating_add_signed(delta).min(len - 1)
}

pub fn counter_label(index: usize, len: usize) -> String {
    format!("{} of {}", index + 1, len)
}

pub fn thumbnail_window(index: usize, len: usize, capacity: usize) -> Range<usize> {
    let capacity = capacity.max(1).min(len);
    let start = index
        .saturating_sub(capacity / 2)
        .min(len.saturating_sub(capacity));
    start..start + capacity
}

#[cfg(test)]
mod tests {
    use super::*;

    const STAGE: Pair = (800., 600.);

    #[test]
    fn fit_contains_large_images_and_never_upscales() {
        assert_eq!(fit_scale((1600., 1200.), STAGE), 0.5);
        assert_eq!(fit_scale((1600., 300.), STAGE), 0.5);
        assert_eq!(fit_scale((100., 100.), STAGE), 1.);
        assert_eq!(fit_scale((0., 100.), STAGE), 1.);
    }

    #[test]
    fn rotation_swaps_the_sides_on_odd_quarter_turns() {
        assert_eq!(rotated_size((400., 200.), 0), (400., 200.));
        assert_eq!(rotated_size((400., 200.), 1), (200., 400.));
        assert_eq!(rotated_size((400., 200.), 2), (400., 200.));
        assert_eq!(rotated_size((400., 200.), 3), (200., 400.));
        assert_eq!(next_rotation(3), 0);
        assert_eq!(next_rotation(0), 1);
    }

    #[test]
    fn scale_is_limited_to_fit_and_four_times_the_source() {
        assert_eq!(clamp_scale(0.1, 0.5), 0.5);
        assert_eq!(clamp_scale(9., 0.5), MAX_ZOOM);
        assert_eq!(clamp_scale(2., 0.5), 2.);
    }

    #[test]
    fn pan_is_clamped_to_the_image_bounds() {
        let display = (1600., 400.);
        assert_eq!(clamp_pan((900., 50.), display, STAGE), (400., 0.));
        assert_eq!(clamp_pan((-900., -50.), display, STAGE), (-400., 0.));
        assert_eq!(clamp_pan((10., 10.), (400., 300.), STAGE), (0., 0.));
    }

    #[test]
    fn image_origin_centers_and_follows_the_pan() {
        assert_eq!(image_origin((400., 300.), STAGE, (0., 0.)), (200., 150.));
        assert_eq!(image_origin((1600., 600.), STAGE, (-100., 0.)), (-500., 0.));
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor_fixed() {
        let image = (2000., 1500.);
        let start = View {
            scale: 0.4,
            pan: (0., 0.),
        };
        let anchor = (100., 50.);
        let zoomed = zoom_at(start, 1., anchor, image, STAGE);
        assert_eq!(zoomed.scale, 1.);
        let before = (
            (anchor.0 - start.pan.0) / start.scale,
            (anchor.1 - start.pan.1) / start.scale,
        );
        let after = (
            (anchor.0 - zoomed.pan.0) / zoomed.scale,
            (anchor.1 - zoomed.pan.1) / zoomed.scale,
        );
        assert!((before.0 - after.0).abs() < 0.01);
        assert!((before.1 - after.1).abs() < 0.01);
    }

    #[test]
    fn zooming_back_to_fit_recenters() {
        let image = (2000., 1500.);
        let zoomed = View {
            scale: 2.,
            pan: (300., -200.),
        };
        let fit = zoom_at(zoomed, 0.1, (50., 50.), image, STAGE);
        assert_eq!(fit.scale, 0.4);
        assert_eq!(fit.pan, (0., 0.));
    }

    #[test]
    fn zoom_never_pans_past_the_edges() {
        let image = (2000., 1500.);
        let start = View {
            scale: 0.4,
            pan: (0., 0.),
        };
        let zoomed = zoom_at(start, 4., (-400., -300.), image, STAGE);
        let limit = (2000. * 4. - 800.) / 2.;
        assert!(zoomed.pan.0.abs() <= limit);
    }

    #[test]
    fn wheel_zooms_in_up_and_out_down_symmetrically() {
        assert!(wheel_scale(1., 20.) > 1.);
        assert!(wheel_scale(1., -20.) < 1.);
        assert!((wheel_scale(wheel_scale(1., 20.), -20.) - 1.).abs() < 1e-5);
        assert_eq!(wheel_scale(1., 0.), 1.);
    }

    #[test]
    fn keyboard_steps_multiply_and_divide_by_the_step() {
        assert_eq!(step_scale(1., 1), ZOOM_STEP);
        assert!((step_scale(ZOOM_STEP, -1) - 1.).abs() < 1e-6);
    }

    #[test]
    fn double_click_toggles_between_fit_and_full_size() {
        assert_eq!(toggle_target(0.4, 0.4), 1.);
        assert_eq!(toggle_target(1., 0.4), 0.4);
        assert_eq!(toggle_target(2., 0.4), 0.4);
        assert_eq!(toggle_target(1., 1.), 1.);
    }

    #[test]
    fn percent_label_is_relative_to_the_source_size() {
        assert_eq!(percent_label(1.), "100%");
        assert_eq!(percent_label(0.375), "38%");
        assert_eq!(percent_label(4.), "400%");
    }

    #[test]
    fn navigation_stops_at_both_ends() {
        assert_eq!(step_index(0, 5, -1), 0);
        assert_eq!(step_index(4, 5, 1), 4);
        assert_eq!(step_index(2, 5, 1), 3);
        assert_eq!(step_index(2, 5, -1), 1);
        assert_eq!(step_index(0, 0, 1), 0);
    }

    #[test]
    fn counter_is_one_based() {
        assert_eq!(counter_label(2, 12), "3 of 12");
    }

    #[test]
    fn thumbnail_window_follows_the_current_image_inside_the_list() {
        assert_eq!(thumbnail_window(0, 20, 5), 0..5);
        assert_eq!(thumbnail_window(10, 20, 5), 8..13);
        assert_eq!(thumbnail_window(19, 20, 5), 15..20);
        assert_eq!(thumbnail_window(1, 3, 5), 0..3);
        assert_eq!(thumbnail_window(0, 0, 5), 0..0);
        assert_eq!(thumbnail_window(2, 4, 0), 2..3);
    }
}
