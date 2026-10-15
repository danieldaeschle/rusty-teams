use calling::{TileDetail, VideoWant};

use crate::model::{ActiveCall, CallModel, Focus, Tile};
use crate::stage_layout::{
    FILMSTRIP_MAX, Filmstrip, GALLERY_MAX, Overflow, filmstrip, share_strip, split_overflow,
};
use crate::view_settings::CallLayout;

pub const TILE_GAP: f32 = 20.;
pub const GRID_PADDING: f32 = 24.;
pub const STRIP_GAP: f32 = 12.;
pub const FILMSTRIP_TILE_MAX: f32 = 220.;
const FALLBACK_STAGE: (f32, f32) = (900., 560.);

pub fn measured_stage(call: &ActiveCall) -> (f32, f32) {
    let (width, height) = call.stage_size.get();
    if width < 1. || height < 1. {
        return FALLBACK_STAGE;
    }
    (width, height)
}

pub fn gallery_split(model: &CallModel) -> (Vec<&Tile>, Overflow) {
    let others = model.layout_tiles();
    let overflow = split_overflow(others.len(), model.own_cells(), GALLERY_MAX);
    (others, overflow)
}

pub struct SpeakerSplit<'a> {
    pub focus: Option<Focus>,
    pub others: Vec<&'a Tile>,
    pub own_cells: usize,
    pub plan: Filmstrip,
}

pub fn speaker_split(model: &CallModel, stage: (f32, f32)) -> SpeakerSplit<'_> {
    let focus = model.speaker_focus();
    let focus_mri = focus.as_ref().map(|focus| focus.mri.as_str());
    let mut others: Vec<&Tile> = model
        .layout_tiles()
        .into_iter()
        .filter(|tile| Some(tile.mri.as_str()) != focus_mri)
        .collect();
    let own_cells = usize::from(model.own_cells() > 0 && !focus.as_ref().is_some_and(|f| f.own));
    let plan = filmstrip(
        others.len(),
        own_cells,
        stage.0 - 2. * GRID_PADDING,
        TILE_GAP,
        FILMSTRIP_TILE_MAX,
    );
    if plan.overflow.shown < FILMSTRIP_MAX.min(others.len()) {
        others.sort_by_key(|tile| !model.is_recent_speaker(&tile.mri));
    }
    SpeakerSplit { focus, others, own_cells, plan }
}

pub struct StripSplit<'a> {
    pub width: f32,
    pub others: Vec<&'a Tile>,
    pub plan: Filmstrip,
}

pub fn strip_split(model: &CallModel, strip_width: f32, stage: (f32, f32)) -> StripSplit<'_> {
    let available = (stage.0 - 2. * GRID_PADDING, (stage.1 - 2. * GRID_PADDING).max(0.));
    let width = strip_width.min(((available.0 - TILE_GAP) / 2.).max(0.));
    let others: Vec<&Tile> = model
        .strip_tiles()
        .into_iter()
        .filter(|tile| !model.hidden_for_no_video(tile))
        .collect();
    let plan = share_strip(others.len(), model.own_cells(), (width, available.1), STRIP_GAP);
    StripSplit { width, others, plan }
}

const LARGE_GALLERY_TILES: usize = 4;

fn want(tile: &Tile, detail: TileDetail) -> VideoWant {
    VideoWant { mri: tile.mri.clone(), detail }
}

pub fn video_wants(model: &CallModel, stage: (f32, f32), strip_width: f32) -> Vec<VideoWant> {
    if model.has_content() {
        if model.effective_layout() == CallLayout::Content {
            return Vec::new();
        }
        let split = strip_split(model, strip_width, stage);
        return split.others[..split.plan.overflow.shown]
            .iter()
            .map(|tile| want(tile, TileDetail::Small))
            .collect();
    }
    if model.effective_layout() == CallLayout::Speaker {
        let split = speaker_split(model, stage);
        let stage_person = split.focus.iter().filter(|focus| !focus.own).map(|focus| VideoWant {
            mri: focus.mri.clone(),
            detail: TileDetail::Stage,
        });
        return stage_person
            .chain(split.others[..split.plan.overflow.shown].iter().map(|tile| want(tile, TileDetail::Small)))
            .collect();
    }
    let (others, overflow) = gallery_split(model);
    let detail = if overflow.shown <= LARGE_GALLERY_TILES { TileDetail::Stage } else { TileDetail::Small };
    others[..overflow.shown].iter().map(|tile| want(tile, detail)).collect()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use calling::{CallUpdate, RosterEntry, TileDetail, VideoWant};

    use super::{FILMSTRIP_TILE_MAX, GRID_PADDING, TILE_GAP, video_wants};
    use crate::model::CallModel;
    use crate::share_view::STRIP_DEFAULT_WIDTH;
    use crate::stage_layout::{GALLERY_MAX, filmstrip, split_overflow};
    use crate::view_settings::CallLayout;

    const STAGE: (f32, f32) = (1400., 800.);

    fn person(index: usize, has_video: bool) -> RosterEntry {
        RosterEntry {
            mri: format!("8:orgid:p{index}"),
            display_name: format!("P{index}"),
            muted: false,
            in_lobby: false,
            has_video,
            sharing: false,
            hand: None,
            spotlight: None,
            organizer: false,
            role: None,
            limits: Default::default(),
        }
    }

    fn meeting(people: Vec<RosterEntry>) -> CallModel {
        let mut model = CallModel::meeting("Standup", false);
        model.apply(CallUpdate::OwnIdentity { mri: "8:orgid:me".into() });
        model.apply(CallUpdate::Roster(people));
        model
    }

    fn crowd(count: usize) -> CallModel {
        meeting((0..count).map(|index| person(index, true)).collect())
    }

    fn details(wants: &[VideoWant]) -> Vec<TileDetail> {
        wants.iter().map(|want| want.detail).collect()
    }

    #[test]
    fn a_small_gallery_asks_for_large_pictures() {
        let wants = video_wants(&crowd(3), STAGE, STRIP_DEFAULT_WIDTH);
        assert_eq!(wants.len(), 3);
        assert!(details(&wants).iter().all(|detail| *detail == TileDetail::Stage));
    }

    #[test]
    fn a_big_gallery_asks_for_small_pictures_and_skips_the_plus_n_people() {
        let model = crowd(14);
        let wants = video_wants(&model, STAGE, STRIP_DEFAULT_WIDTH);
        assert_eq!(wants.len(), split_overflow(14, 1, GALLERY_MAX).shown);
        assert!(details(&wants).iter().all(|detail| *detail == TileDetail::Small));
        let hidden: Vec<&str> = model.layout_tiles()[wants.len()..].iter().map(|tile| tile.mri.as_str()).collect();
        assert!(wants.iter().all(|want| !hidden.contains(&want.mri.as_str())));
    }

    #[test]
    fn people_hidden_for_having_no_video_get_no_picture_request() {
        let mut model = meeting(vec![person(0, true), person(1, false), person(2, false)]);
        model.view.hide_no_video = true;
        let wants = video_wants(&model, STAGE, STRIP_DEFAULT_WIDTH);
        assert_eq!(wants.iter().map(|want| want.mri.as_str()).collect::<Vec<_>>(), vec!["8:orgid:p0"]);
    }

    #[test]
    fn the_speaker_stage_is_large_and_the_filmstrip_small() {
        let mut model = crowd(5);
        model.view.choose(CallLayout::Speaker);
        model.refresh_layout(Instant::now());
        let focus = model.speaker_focus().expect("a stage person").mri;
        let wants = video_wants(&model, STAGE, STRIP_DEFAULT_WIDTH);
        assert_eq!(wants[0], VideoWant { mri: focus.clone(), detail: TileDetail::Stage });
        assert_eq!(wants.len(), 5);
        assert!(wants[1..].iter().all(|want| want.detail == TileDetail::Small && want.mri != focus));
    }

    #[test]
    fn the_speaker_filmstrip_drops_people_that_do_not_fit() {
        let mut model = crowd(12);
        model.view.choose(CallLayout::Speaker);
        model.refresh_layout(Instant::now());
        let wants = video_wants(&model, (500., 800.), STRIP_DEFAULT_WIDTH);
        let fitting = filmstrip(11, 1, 500. - 2. * GRID_PADDING, TILE_GAP, FILMSTRIP_TILE_MAX).overflow.shown;
        assert!(fitting < 11);
        assert_eq!(wants.len(), 1 + fitting);
    }

    #[test]
    fn next_to_shared_content_everybody_in_the_strip_is_small_and_content_focus_wants_nobody() {
        let mut model = crowd(3);
        model.apply(CallUpdate::ScreenShare(Some("8:orgid:p1".into())));
        let wants = video_wants(&model, STAGE, STRIP_DEFAULT_WIDTH);
        assert_eq!(wants.len(), 3);
        assert!(details(&wants).iter().all(|detail| *detail == TileDetail::Small));
        model.view.choose(CallLayout::Content);
        assert!(video_wants(&model, STAGE, STRIP_DEFAULT_WIDTH).is_empty());
    }

    #[test]
    fn a_full_share_strip_wants_video_for_every_shown_tile_and_none_behind_the_plus_tile() {
        let mut model = crowd(9);
        model.apply(CallUpdate::ScreenShare(Some("8:orgid:p1".into())));
        assert_eq!(video_wants(&model, (1400., 1100.), STRIP_DEFAULT_WIDTH).len(), 9);
        let short = video_wants(&model, (1400., 600.), STRIP_DEFAULT_WIDTH);
        assert!(short.len() >= 3 && short.len() < 9, "{} wants", short.len());
    }
}
