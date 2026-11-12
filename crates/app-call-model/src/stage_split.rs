use calling::{TileDetail, VideoWant};

use crate::model::{ActiveCall, CallModel, Focus, Tile};
use crate::stage_layout::{
    FILMSTRIP_MAX, Filmstrip, filmstrip, gallery_page_range, gallery_pages,
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

pub struct GalleryPage<'a> {
    pub tiles: Vec<&'a Tile>,
    pub own: bool,
    pub page: usize,
    pub pages: usize,
}

pub fn gallery_page(model: &CallModel) -> GalleryPage<'_> {
    let others = model.layout_tiles();
    let own_cells = model.own_cells();
    let pages = gallery_pages(others.len(), own_cells);
    let page = model.gallery_page.min(pages - 1);
    let range = gallery_page_range(others.len(), own_cells, page);
    GalleryPage {
        tiles: others[range].to_vec(),
        own: own_cells > 0 && page == 0,
        page,
        pages,
    }
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
    pub others: Vec<&'a Tile>,
    pub plan: Filmstrip,
}

pub fn strip_split(model: &CallModel, stage: (f32, f32)) -> StripSplit<'_> {
    let others: Vec<&Tile> = model
        .strip_tiles()
        .into_iter()
        .filter(|tile| !model.hidden_for_no_video(tile))
        .collect();
    let plan = filmstrip(others.len(), model.own_cells(), stage.0 - 2. * GRID_PADDING, STRIP_GAP, FILMSTRIP_TILE_MAX);
    StripSplit { others, plan }
}

const LARGE_GALLERY_TILES: usize = 4;

fn want(tile: &Tile, detail: TileDetail) -> VideoWant {
    VideoWant { mri: tile.mri.clone(), detail }
}

pub fn video_wants(model: &CallModel, stage: (f32, f32)) -> Vec<VideoWant> {
    if model.has_content() {
        if model.effective_layout() == CallLayout::Content {
            return Vec::new();
        }
        let split = strip_split(model, stage);
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
    let page = gallery_page(model);
    let detail = if page.tiles.len() <= LARGE_GALLERY_TILES { TileDetail::Stage } else { TileDetail::Small };
    page.tiles.iter().map(|tile| want(tile, detail)).collect()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use calling::{CallUpdate, RosterEntry, TileDetail, VideoWant};

    use super::{FILMSTRIP_TILE_MAX, GRID_PADDING, STRIP_GAP, TILE_GAP, video_wants};
    use crate::model::CallModel;
    use crate::stage_layout::{filmstrip, first_page_size};
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
        let wants = video_wants(&crowd(3), STAGE);
        assert_eq!(wants.len(), 3);
        assert!(details(&wants).iter().all(|detail| *detail == TileDetail::Stage));
    }

    #[test]
    fn a_big_gallery_asks_for_small_pictures_and_skips_the_other_pages() {
        let model = crowd(14);
        let wants = video_wants(&model, STAGE);
        assert_eq!(wants.len(), first_page_size(14, 1));
        assert!(details(&wants).iter().all(|detail| *detail == TileDetail::Small));
        let hidden: Vec<&str> = model.layout_tiles()[wants.len()..].iter().map(|tile| tile.mri.as_str()).collect();
        assert!(wants.iter().all(|want| !hidden.contains(&want.mri.as_str())));
    }

    #[test]
    fn people_hidden_for_having_no_video_get_no_picture_request() {
        let mut model = meeting(vec![person(0, true), person(1, false), person(2, false)]);
        model.view.hide_no_video = true;
        let wants = video_wants(&model, STAGE);
        assert_eq!(wants.iter().map(|want| want.mri.as_str()).collect::<Vec<_>>(), vec!["8:orgid:p0"]);
    }

    #[test]
    fn the_speaker_stage_is_large_and_the_filmstrip_small() {
        let mut model = crowd(5);
        model.view.choose(CallLayout::Speaker);
        model.refresh_layout(Instant::now());
        let focus = model.speaker_focus().expect("a stage person").mri;
        let wants = video_wants(&model, STAGE);
        assert_eq!(wants[0], VideoWant { mri: focus.clone(), detail: TileDetail::Stage });
        assert_eq!(wants.len(), 5);
        assert!(wants[1..].iter().all(|want| want.detail == TileDetail::Small && want.mri != focus));
    }

    #[test]
    fn the_speaker_filmstrip_drops_people_that_do_not_fit() {
        let mut model = crowd(12);
        model.view.choose(CallLayout::Speaker);
        model.refresh_layout(Instant::now());
        let wants = video_wants(&model, (500., 800.));
        let fitting = filmstrip(11, 1, 500. - 2. * GRID_PADDING, TILE_GAP, FILMSTRIP_TILE_MAX).overflow.shown;
        assert!(fitting < 11);
        assert_eq!(wants.len(), 1 + fitting);
    }

    #[test]
    fn next_to_shared_content_everybody_in_the_strip_is_small_and_content_focus_wants_nobody() {
        let mut model = crowd(3);
        model.apply(CallUpdate::ScreenShare(Some("8:orgid:p1".into())));
        let wants = video_wants(&model, STAGE);
        assert_eq!(wants.len(), 3);
        assert!(details(&wants).iter().all(|detail| *detail == TileDetail::Small));
        model.view.choose(CallLayout::Content);
        assert!(video_wants(&model, STAGE).is_empty());
    }

    #[test]
    fn the_share_strip_on_top_wants_video_for_every_shown_tile_and_none_behind_the_plus_tile() {
        let mut model = crowd(9);
        model.apply(CallUpdate::ScreenShare(Some("8:orgid:p1".into())));
        let fitting = filmstrip(9, 1, 1400. - 2. * GRID_PADDING, STRIP_GAP, FILMSTRIP_TILE_MAX).overflow.shown;
        assert!(fitting < 9);
        assert_eq!(video_wants(&model, (1400., 800.)).len(), fitting);
    }
}
