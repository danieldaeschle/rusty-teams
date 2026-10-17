use calling::ShareKind;

use super::model::{CallModel, Tile, TileState};
use crate::notify::platform::{ScreenRect, WorkArea};

pub const GALLERY_WIDTH: f32 = 192.;
pub const GALLERY_INSET: f32 = 3.;
pub const GALLERY_TILE_WIDTH: f32 = GALLERY_WIDTH - 2. * GALLERY_INSET;
pub const GALLERY_TILE_HEIGHT: f32 = GALLERY_TILE_WIDTH * 9. / 16.;
pub const GALLERY_TITLE_HEIGHT: f32 = 32.;
pub const GALLERY_CONTROLS_HEIGHT: f32 = 40.;
pub const GALLERY_DIVIDER_HEIGHT: f32 = 20.;
pub const GALLERY_MAX_TILES: usize = 4;
pub const GALLERY_MARGIN: f32 = 30.;
pub const GALLERY_POSITION_KEY: &str = "call_gallery_position";
pub const MINIMIZE_OVERLAP_PERCENT: i64 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalShare {
    None,
    Screen,
    Window,
}

impl LocalShare {
    pub fn of(kind: Option<ShareKind>, sharing: bool) -> LocalShare {
        match (sharing, kind) {
            (false, _) => LocalShare::None,
            (true, Some(ShareKind::Window)) => LocalShare::Window,
            (true, _) => LocalShare::Screen,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Monitor {
    None,
    Mini,
    Gallery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Main,
    ShareBar,
    Border,
    Mini,
    Gallery,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OverlayPlan {
    pub monitor: Monitor,
    pub share_bar: bool,
    pub exclude_main: bool,
}

pub fn overlay_plan(
    in_call: bool,
    share: LocalShare,
    main_minimized: bool,
    speaker_only: bool,
) -> OverlayPlan {
    let monitor = match (in_call && main_minimized, share) {
        (false, _) => Monitor::None,
        (true, LocalShare::None) => Monitor::Mini,
        (true, _) if speaker_only => Monitor::Mini,
        (true, _) => Monitor::Gallery,
    };
    OverlayPlan {
        monitor,
        share_bar: in_call && share != LocalShare::None,
        exclude_main: excluded_from_capture(Surface::Main, share),
    }
}

pub fn excluded_from_capture(surface: Surface, share: LocalShare) -> bool {
    match surface {
        Surface::Main => share == LocalShare::Screen,
        Surface::ShareBar | Surface::Border | Surface::Mini | Surface::Gallery => true,
    }
}

fn overlap_area(first: ScreenRect, second: ScreenRect) -> i64 {
    let width = (first.right.min(second.right) - first.left.max(second.left)).max(0);
    let height = (first.bottom.min(second.bottom) - first.top.max(second.top)).max(0);
    i64::from(width) * i64::from(height)
}

pub fn minimize_on_share_start(
    share: LocalShare,
    main: Option<ScreenRect>,
    shared: Option<ScreenRect>,
) -> bool {
    if share != LocalShare::Screen {
        return false;
    }
    let (Some(main), Some(shared)) = (main, shared) else {
        return true;
    };
    let main_area = i64::from(main.width().max(0)) * i64::from(main.height().max(0));
    if main_area == 0 {
        return true;
    }
    overlap_area(main, shared) * 100 >= MINIMIZE_OVERLAP_PERCENT * main_area
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GalleryLayout {
    pub tiles: usize,
    pub overflow: usize,
    pub divider: bool,
    pub self_tile: bool,
}

impl GalleryLayout {
    pub fn height(&self) -> f32 {
        let segments = self.tiles + usize::from(self.self_tile);
        let gaps = (segments + usize::from(self.divider)).saturating_sub(1);
        GALLERY_TITLE_HEIGHT
            + GALLERY_INSET
            + segments as f32 * GALLERY_TILE_HEIGHT
            + if self.divider {
                GALLERY_DIVIDER_HEIGHT
            } else {
                0.
            }
            + gaps as f32 * GALLERY_INSET
            + GALLERY_INSET
            + GALLERY_CONTROLS_HEIGHT
    }
}

pub fn gallery_layout(others: usize, self_tile: bool, available_height: f32) -> GalleryLayout {
    let build = |tiles: usize| GalleryLayout {
        tiles,
        overflow: others - tiles,
        divider: others > 0 && self_tile,
        self_tile,
    };
    let mut layout = build(others.min(GALLERY_MAX_TILES));
    while layout.tiles > 1 && layout.height() > available_height {
        layout = build(layout.tiles - 1);
    }
    layout
}

pub fn gallery_available_height(work: WorkArea) -> f32 {
    (work.bottom - work.top) as f32 / work.scale - 2. * GALLERY_MARGIN
}

pub fn gallery_rect(
    work: WorkArea,
    layout: GalleryLayout,
    origin: Option<(i32, i32)>,
) -> ScreenRect {
    let scaled = |logical: f32| (logical * work.scale).round() as i32;
    let width = scaled(GALLERY_WIDTH);
    let height = scaled(layout.height());
    let margin = scaled(GALLERY_MARGIN);
    let (left, top) =
        origin.unwrap_or((work.right - width - margin, work.bottom - height - margin));
    let left = left.clamp(work.left, (work.right - width).max(work.left));
    let top = top.clamp(work.top, (work.bottom - height).max(work.top));
    ScreenRect {
        left,
        top,
        right: left + width,
        bottom: top + height,
    }
}

fn prioritized(model: &CallModel, tile: &Tile) -> bool {
    model.is_pinned(&tile.mri) || tile.spotlight.is_some() || tile.hand.is_some()
}

pub fn gallery_candidates(model: &CallModel) -> Vec<&Tile> {
    let mut tiles: Vec<&Tile> = model
        .layout_tiles()
        .into_iter()
        .filter(|tile| tile.state == TileState::Present)
        .collect();
    tiles.sort_by_key(|tile| {
        (
            !prioritized(model, tile),
            !(model.tile_speaking(tile) || model.is_recent_speaker(&tile.mri)),
        )
    });
    tiles
}

#[cfg(test)]
mod tests {
    use super::*;

    fn work(scale: f32) -> WorkArea {
        WorkArea {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1040,
            scale,
        }
    }

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> ScreenRect {
        ScreenRect {
            left,
            top,
            right,
            bottom,
        }
    }

    #[test]
    fn decision_table() {
        use LocalShare::*;
        use Monitor as M;
        let rows = [
            (None, false, M::None, false, false),
            (None, true, M::Mini, false, false),
            (Screen, false, M::None, true, true),
            (Screen, true, M::Gallery, true, true),
            (Window, false, M::None, true, false),
            (Window, true, M::Gallery, true, false),
        ];
        for (share, minimized, monitor, share_bar, exclude_main) in rows {
            assert_eq!(
                overlay_plan(true, share, minimized, false),
                OverlayPlan {
                    monitor,
                    share_bar,
                    exclude_main
                },
                "{share:?} minimized={minimized}"
            );
        }
    }

    #[test]
    fn speaker_only_swaps_the_gallery_for_the_mini_tile() {
        let plan = overlay_plan(true, LocalShare::Screen, true, true);
        assert_eq!(plan.monitor, Monitor::Mini);
        assert!(plan.exclude_main);
        assert_eq!(
            overlay_plan(true, LocalShare::None, true, true).monitor,
            Monitor::Mini
        );
    }

    #[test]
    fn nothing_shows_outside_a_call() {
        let plan = overlay_plan(false, LocalShare::None, true, false);
        assert_eq!(plan.monitor, Monitor::None);
        assert!(!plan.share_bar);
    }

    #[test]
    fn overlays_are_always_excluded_and_main_only_for_a_screen() {
        for share in [LocalShare::None, LocalShare::Screen, LocalShare::Window] {
            for surface in [
                Surface::ShareBar,
                Surface::Border,
                Surface::Mini,
                Surface::Gallery,
            ] {
                assert!(excluded_from_capture(surface, share));
            }
        }
        assert!(excluded_from_capture(Surface::Main, LocalShare::Screen));
        assert!(!excluded_from_capture(Surface::Main, LocalShare::Window));
        assert!(!excluded_from_capture(Surface::Main, LocalShare::None));
    }

    #[test]
    fn unknown_source_kind_counts_as_a_screen() {
        assert_eq!(LocalShare::of(None, true), LocalShare::Screen);
        assert_eq!(
            LocalShare::of(Some(ShareKind::Window), true),
            LocalShare::Window
        );
        assert_eq!(
            LocalShare::of(Some(ShareKind::Screen), false),
            LocalShare::None
        );
    }

    #[test]
    fn screen_share_minimizes_only_when_the_main_window_is_on_the_shared_monitor() {
        let monitor = rect(0, 0, 1920, 1080);
        let on_monitor = Some(rect(100, 100, 1100, 800));
        let other_monitor = Some(rect(2000, 100, 3000, 800));
        let edge = Some(rect(1880, 0, 2880, 1000));
        assert!(minimize_on_share_start(
            LocalShare::Screen,
            on_monitor,
            Some(monitor)
        ));
        assert!(!minimize_on_share_start(
            LocalShare::Screen,
            other_monitor,
            Some(monitor)
        ));
        assert!(!minimize_on_share_start(
            LocalShare::Screen,
            edge,
            Some(monitor)
        ));
        assert!(minimize_on_share_start(
            LocalShare::Screen,
            None,
            Some(monitor)
        ));
        assert!(minimize_on_share_start(
            LocalShare::Screen,
            on_monitor,
            None
        ));
    }

    #[test]
    fn window_share_and_no_share_never_minimize() {
        let monitor = Some(rect(0, 0, 1920, 1080));
        assert!(!minimize_on_share_start(
            LocalShare::Window,
            monitor,
            monitor
        ));
        assert!(!minimize_on_share_start(LocalShare::None, monitor, monitor));
    }

    #[test]
    fn gallery_stacks_four_tiles_a_divider_and_the_own_tile() {
        let layout = gallery_layout(9, true, 2000.);
        assert_eq!(
            layout,
            GalleryLayout {
                tiles: 4,
                overflow: 5,
                divider: true,
                self_tile: true
            }
        );
        let expected = GALLERY_TITLE_HEIGHT
            + 5. * GALLERY_TILE_HEIGHT
            + GALLERY_DIVIDER_HEIGHT
            + 7. * GALLERY_INSET
            + GALLERY_CONTROLS_HEIGHT;
        assert!((layout.height() - expected).abs() < 0.01);
    }

    #[test]
    fn gallery_chrome_matches_teams() {
        let chrome = GalleryLayout {
            tiles: 0,
            overflow: 0,
            divider: false,
            self_tile: false,
        }
        .height();
        assert_eq!(chrome, 78.);
        assert_eq!(GALLERY_TILE_WIDTH, 186.);
    }

    #[test]
    fn gallery_drops_tiles_to_fit_a_short_screen() {
        let layout = gallery_layout(9, true, 500.);
        assert!(layout.height() <= 500.);
        assert_eq!(layout.tiles, 2);
        assert_eq!(layout.overflow, 7);
        assert_eq!(gallery_layout(9, true, 10.).tiles, 1);
    }

    #[test]
    fn gallery_without_others_has_no_divider() {
        let layout = gallery_layout(0, true, 2000.);
        assert_eq!(
            (layout.tiles, layout.divider, layout.self_tile),
            (0, false, true)
        );
        let hidden_self = gallery_layout(3, false, 2000.);
        assert_eq!((hidden_self.tiles, hidden_self.divider), (3, false));
    }

    #[test]
    fn gallery_defaults_to_the_bottom_right_with_the_teams_margin() {
        let layout = gallery_layout(2, true, 2000.);
        let placed = gallery_rect(work(1.), layout, None);
        assert_eq!(placed.width(), 192);
        assert_eq!((placed.right, placed.bottom), (1920 - 30, 1040 - 30));
        assert_eq!(placed.height(), layout.height().round() as i32);
    }

    #[test]
    fn gallery_keeps_a_dragged_origin_inside_the_work_area() {
        let layout = gallery_layout(4, true, 2000.);
        assert_eq!(gallery_rect(work(1.), layout, Some((200, 100))).left, 200);
        let pushed = gallery_rect(work(1.), layout, Some((5000, 900)));
        assert_eq!((pushed.right, pushed.bottom), (1920, 1040));
    }

    #[test]
    fn gallery_scales_with_the_monitor() {
        let layout = gallery_layout(1, false, 2000.);
        let placed = gallery_rect(work(2.), layout, None);
        assert_eq!(placed.width(), 384);
        assert_eq!(gallery_available_height(work(2.)), 520. - 60.);
    }

    fn present(mri: &str, name: &str) -> Tile {
        Tile {
            state: TileState::Present,
            ..Tile::invited(mri, name)
        }
    }

    #[test]
    fn candidates_put_pinned_then_speakers_first() {
        let mut model = CallModel::test();
        model.tiles = vec![
            present("8:a", "Ada"),
            present("8:b", "Bo"),
            present("8:c", "Cy"),
            Tile::invited("8:d", "Dee"),
        ];
        model.speaking_mris = vec!["8:c".to_owned()];
        model.toggle_pin("8:b");
        let names: Vec<&str> = gallery_candidates(&model)
            .iter()
            .map(|tile| tile.name.as_str())
            .collect();
        assert_eq!(names, ["Bo", "Cy", "Ada"]);
    }
}
