use std::time::Instant;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    popover::Popover,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::call_organizer::render_focus_stage;
use super::call_view::{
    GRID_PADDING, STRIP_GAP, STRIP_TILE, STRIP_VISIBLE, TILE_GAP, own_tile, person_tile,
};
use super::widgets::icon;
use crate::app_state::AppState;
use crate::call::{
    ActiveCall, CallLayout, CallModel, FILMSTRIP_MAX, Filmstrip, Focus, GALLERY_MAX, Overflow, Tile, TileSize,
    filmstrip, gallery_grid, shown_bubbles, split_overflow,
};
use calling::{TileDetail, VideoWant};
use crate::theme;

const FALLBACK_STAGE: (f32, f32) = (900., 560.);
const MEASURE_TOLERANCE: f32 = 0.5;
const OVERLAY_INSET: f32 = 16.;
const CHIP_TOP: f32 = 12.;
const FILMSTRIP_TILE_MAX: f32 = 220.;
const TILE_RADIUS: f32 = 12.;
const HIDDEN_LIST_WIDTH: f32 = 260.;
const HIDDEN_LIST_HEIGHT: f32 = 320.;
const HIDDEN_AVATAR: f32 = 28.;
const STACK_MAX: usize = 3;
const STACK_OVERLAP: f32 = 0.35;
const BUBBLE_WIDTH: f32 = 340.;
const BUBBLE_AVATAR: f32 = 28.;
const BUBBLE_GAP: f32 = 8.;
const BUBBLE_FADE_FROM: f32 = 0.9;
const BUBBLE_BACKGROUND: f32 = 0.72;
const CHIP_BACKGROUND: f32 = 0.6;
pub const FULLSCREEN_HINT: &str = "Press Esc to exit full screen";

pub fn measured_stage(call: &ActiveCall) -> (f32, f32) {
    let (width, height) = call.stage_size.get();
    if width < 1. || height < 1. {
        return FALLBACK_STAGE;
    }
    (width, height)
}

fn measure_stage(call: &ActiveCall) -> impl IntoElement {
    let stage_size = call.stage_size.clone();
    canvas(
        |_, _, _| {},
        move |bounds, _, window, _| {
            let measured = (f32::from(bounds.size.width), f32::from(bounds.size.height));
            let known = stage_size.get();
            if (known.0 - measured.0).abs() > MEASURE_TOLERANCE
                || (known.1 - measured.1).abs() > MEASURE_TOLERANCE
            {
                stage_size.set(measured);
                window.refresh();
            }
        },
    )
    .absolute()
    .size_full()
}

fn dark_chip() -> Div {
    h_flex()
        .h(px(30.))
        .px(px(12.))
        .gap(px(8.))
        .items_center()
        .rounded_full()
        .bg(hsla(0., 0., 0., CHIP_BACKGROUND))
        .text_color(theme::white())
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
}

fn without_video_chip(app: &Entity<AppState>, model: &CallModel) -> Option<AnyElement> {
    let count = model.hidden_without_video();
    if !model.view.hide_no_video || count == 0 {
        return None;
    }
    let app = app.clone();
    Some(
        div()
            .absolute()
            .top(px(CHIP_TOP))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                dark_chip()
                    .id("call-no-video-chip")
                    .cursor_pointer()
                    .hover(|chip| chip.opacity(0.85))
                    .tooltip(|window, cx| {
                        Tooltip::new("Show people without video").build(window, cx)
                    })
                    .child(icon(IconName::VideoOff, 15., theme::white()))
                    .child(format!("{count} without video"))
                    .on_click(move |_, _, cx| {
                        app.update(cx, |state, cx| state.toggle_call_hide_no_video(cx))
                    }),
            )
            .into_any_element(),
    )
}

fn show_me_pill(app: &Entity<AppState>, model: &CallModel, bottom: f32) -> Option<AnyElement> {
    if !model.view.hide_me {
        return None;
    }
    let app = app.clone();
    Some(
        div()
            .absolute()
            .right(px(OVERLAY_INSET))
            .bottom(px(bottom))
            .child(
                dark_chip()
                    .id("call-show-me")
                    .cursor_pointer()
                    .hover(|chip| chip.opacity(0.85))
                    .tooltip(|window, cx| Tooltip::new("Show me").build(window, cx))
                    .child(icon(IconName::Eye, 15., theme::white()))
                    .child("You")
                    .on_click(move |_, _, cx| {
                        app.update(cx, |state, cx| state.set_call_hide_me(false, cx))
                    }),
            )
            .into_any_element(),
    )
}

fn chat_bubbles(
    app: &Entity<AppState>,
    state: &AppState,
    call: &ActiveCall,
    now: Instant,
    bottom: f32,
) -> Option<AnyElement> {
    if call.chat_open || !call.model.view.chat_bubbles {
        return None;
    }
    let bubbles = shown_bubbles(&call.bubbles, now);
    if bubbles.is_empty() {
        return None;
    }
    let shown = crate::call::BUBBLE_SHOWN;
    Some(
        v_flex()
            .absolute()
            .right(px(OVERLAY_INSET))
            .bottom(px(bottom))
            .items_end()
            .gap(px(BUBBLE_GAP))
            .children(bubbles.into_iter().map(|bubble| {
                let app = app.clone();
                let id = SharedString::from(format!("call-bubble-{}", bubble.id));
                h_flex()
                    .id(id.clone())
                    .max_w(px(BUBBLE_WIDTH))
                    .pl(px(6.))
                    .pr(px(14.))
                    .py(px(6.))
                    .gap(px(8.))
                    .items_center()
                    .rounded_full()
                    .bg(hsla(0., 0., 0., BUBBLE_BACKGROUND))
                    .text_color(theme::white())
                    .text_size(px(13.))
                    .cursor_pointer()
                    .child(person_avatar(
                        &state.directory,
                        bubble.sender_id.as_deref(),
                        &bubble.name,
                        BUBBLE_AVATAR,
                    ))
                    .child(
                        h_flex()
                            .min_w_0()
                            .flex_1()
                            .gap(px(6.))
                            .child(
                                div()
                                    .flex_none()
                                    .font_weight(FontWeight::BOLD)
                                    .child(bubble.name.clone()),
                            )
                            .child(div().min_w_0().truncate().child(bubble.text.clone())),
                    )
                    .on_click(move |_, _, cx| {
                        app.update(cx, |state, cx| {
                            if state.call_chat_thread().is_none() {
                                state.toggle_call_chat(cx);
                            }
                        })
                    })
                    .with_animation(id, Animation::new(shown), |bubble, delta| {
                        let fade =
                            ((delta - BUBBLE_FADE_FROM) / (1. - BUBBLE_FADE_FROM)).clamp(0., 1.);
                        bubble.opacity(1. - fade)
                    })
            }))
            .into_any_element(),
    )
}

fn fullscreen_hint(call: &ActiveCall, now: Instant, bottom: f32) -> Option<AnyElement> {
    call.fullscreen
        .filter(|fullscreen| fullscreen.hint_visible(now))
        .map(|_| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(bottom))
                .flex()
                .justify_center()
                .child(dark_chip().h(px(32.)).child(FULLSCREEN_HINT))
                .into_any_element()
        })
}

fn stacked_avatars(state: &AppState, hidden: &[&Tile], diameter: f32) -> Div {
    let shown: Vec<&&Tile> = hidden.iter().take(STACK_MAX).collect();
    let step = diameter * (1. - STACK_OVERLAP);
    div()
        .relative()
        .h(px(diameter))
        .w(px(diameter + step * (shown.len().saturating_sub(1)) as f32))
        .children(shown.into_iter().enumerate().map(|(index, tile)| {
            div()
                .absolute()
                .top_0()
                .left(px(step * index as f32))
                .rounded_full()
                .bg(theme::row_hover())
                .child(person_avatar(
                    &state.directory,
                    tile.user_id.as_deref(),
                    &tile.name,
                    diameter,
                ))
        }))
}

fn overflow_tile(
    app: &Entity<AppState>,
    state: &AppState,
    size: TileSize,
    hidden: &[&Tile],
) -> AnyElement {
    let people: Vec<(String, String, Option<String>)> = hidden
        .iter()
        .map(|tile| (tile.mri.clone(), tile.name.clone(), tile.user_id.clone()))
        .collect();
    let diameter = (size.height * 0.3).clamp(22., 44.);
    let face = v_flex()
        .w(px(size.width))
        .h(px(size.height))
        .items_center()
        .justify_center()
        .gap(px(size.height * 0.06))
        .rounded(px(TILE_RADIUS))
        .bg(theme::row_hover())
        .child(stacked_avatars(state, hidden, diameter))
        .child(
            div()
                .text_size(px((size.height * 0.2).clamp(14., 28.)))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text_muted())
                .child(format!("+{}", hidden.len())),
        );
    let app = app.clone();
    Popover::new("call-hidden-popover")
        .anchor(Anchor::BottomLeft)
        .offset(px(8.))
        .trigger(
            Button::new("call-hidden")
                .ghost()
                .p_0()
                .w(px(size.width))
                .h(px(size.height))
                .rounded(px(TILE_RADIUS))
                .child(face),
        )
        .content(move |_, _, cx| {
            let popover = cx.entity().downgrade();
            v_flex()
                .id("call-hidden-list")
                .w(px(HIDDEN_LIST_WIDTH))
                .max_h(px(HIDDEN_LIST_HEIGHT))
                .overflow_y_scroll()
                .gap(px(2.))
                .children(people.iter().map(|(mri, name, user_id)| {
                    let app_for_click = app.clone();
                    let popover = popover.clone();
                    let pinned_mri = mri.clone();
                    h_flex()
                        .id(SharedString::from(format!("call-hidden-{mri}")))
                        .w_full()
                        .px(px(8.))
                        .py(px(6.))
                        .gap(px(10.))
                        .items_center()
                        .rounded(px(8.))
                        .cursor_pointer()
                        .hover(|row| row.bg(theme::row_hover()))
                        .child(person_avatar(
                            &app.read(cx).directory,
                            user_id.as_deref(),
                            name,
                            HIDDEN_AVATAR,
                        ))
                        .child(div().min_w_0().flex_1().truncate().child(name.clone()))
                        .on_click(move |_, window, cx| {
                            app_for_click
                                .update(cx, |state, cx| state.toggle_call_pin(&pinned_mri, cx));
                            popover
                                .update(cx, |state, cx| state.dismiss(window, cx))
                                .ok();
                        })
                }))
        })
        .into_any_element()
}

fn own_slot(model: &CallModel, shown: &[&Tile]) -> usize {
    match model.own_hand {
        None => shown.len(),
        Some(rank) => shown
            .iter()
            .filter(|tile| tile.hand.is_some_and(|other| other < rank))
            .count(),
    }
}

fn rows_of(cells: Vec<AnyElement>, columns: usize) -> Vec<Div> {
    let mut rows = Vec::new();
    let mut remaining = cells.into_iter();
    loop {
        let row: Vec<AnyElement> = remaining.by_ref().take(columns).collect();
        if row.is_empty() {
            return rows;
        }
        rows.push(h_flex().gap(px(TILE_GAP)).justify_center().children(row));
    }
}

fn gallery_split(model: &CallModel) -> (Vec<&Tile>, Overflow) {
    let others = model.layout_tiles();
    let overflow = split_overflow(others.len(), model.own_cells(), GALLERY_MAX);
    (others, overflow)
}

fn gallery_body(
    app: &Entity<AppState>,
    state: &AppState,
    model: &CallModel,
    now: Instant,
    stage: (f32, f32),
) -> AnyElement {
    let (others, overflow) = gallery_split(model);
    let own_cells = model.own_cells();
    let cells = overflow.shown + own_cells + usize::from(overflow.hidden > 0);
    let grid = gallery_grid(
        cells,
        stage.0 - 2. * GRID_PADDING,
        stage.1 - 2. * GRID_PADDING,
        TILE_GAP,
    );
    let size = TileSize::sixteen_by_nine(grid.tile_width.floor());
    let shown = &others[..overflow.shown];
    let mut tiles: Vec<AnyElement> = shown
        .iter()
        .map(|tile| person_tile(app, state, model, tile, size, now))
        .collect();
    if own_cells > 0 {
        tiles.insert(
            own_slot(model, shown),
            own_tile(app, state, model, size, now),
        );
    }
    if overflow.hidden > 0 {
        tiles.push(overflow_tile(app, state, size, &others[overflow.shown..]));
    }
    v_flex()
        .absolute()
        .inset_0()
        .items_center()
        .justify_center()
        .gap(px(TILE_GAP))
        .children(rows_of(tiles, grid.columns))
        .into_any_element()
}

struct SpeakerSplit<'a> {
    focus: Option<Focus>,
    others: Vec<&'a Tile>,
    own_cells: usize,
    plan: Filmstrip,
}

fn speaker_split(model: &CallModel, stage: (f32, f32)) -> SpeakerSplit<'_> {
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

fn speaker_body(
    app: &Entity<AppState>,
    state: &AppState,
    model: &CallModel,
    now: Instant,
    stage: (f32, f32),
) -> AnyElement {
    let SpeakerSplit { focus, others, own_cells, plan } = speaker_split(model, stage);
    let main = focus
        .as_ref()
        .and_then(|focus| render_focus_stage(app, state, focus, now));
    let size = TileSize::sixteen_by_nine(plan.tile_width.floor());
    let shown = &others[..plan.overflow.shown];
    let mut tiles: Vec<AnyElement> = shown
        .iter()
        .map(|tile| person_tile(app, state, model, tile, size, now))
        .collect();
    if own_cells > 0 {
        tiles.insert(
            own_slot(model, shown),
            own_tile(app, state, model, size, now),
        );
    }
    if plan.overflow.hidden > 0 {
        tiles.push(overflow_tile(
            app,
            state,
            size,
            &others[plan.overflow.shown..],
        ));
    }
    v_flex()
        .absolute()
        .inset_0()
        .p(px(GRID_PADDING))
        .gap(px(TILE_GAP))
        .child(
            h_flex()
                .flex_1()
                .min_h_0()
                .w_full()
                .items_center()
                .children(main),
        )
        .when(!tiles.is_empty(), |column| {
            column.child(
                h_flex()
                    .id("call-filmstrip")
                    .flex_none()
                    .w_full()
                    .gap(px(TILE_GAP))
                    .justify_center()
                    .children(tiles),
            )
        })
        .into_any_element()
}

fn people_strip(
    app: &Entity<AppState>,
    state: &AppState,
    model: &CallModel,
    now: Instant,
) -> Stateful<Div> {
    let mut cells: Vec<AnyElement> = model
        .strip_tiles()
        .into_iter()
        .filter(|tile| !model.hidden_for_no_video(tile))
        .map(|remote| person_tile(app, state, model, remote, STRIP_TILE, now))
        .collect();
    if model.own_cells() > 0 {
        let own_index = model.own_cell_index(cells.len());
        cells.insert(own_index, own_tile(app, state, model, STRIP_TILE, now));
    }
    v_flex()
        .id("call-strip")
        .flex_none()
        .w(px(STRIP_TILE.width))
        .max_h(px(
            STRIP_VISIBLE * (STRIP_TILE.height + STRIP_GAP) - STRIP_GAP
        ))
        .gap(px(STRIP_GAP))
        .overflow_y_scroll()
        .children(cells)
}

fn share_body(
    app: &Entity<AppState>,
    state: &AppState,
    model: &CallModel,
    now: Instant,
    stage: AnyElement,
    layout: CallLayout,
) -> AnyElement {
    h_flex()
        .absolute()
        .inset_0()
        .p(px(GRID_PADDING))
        .gap(px(TILE_GAP))
        .items_center()
        .child(stage)
        .when(layout != CallLayout::Content, |row| {
            row.child(people_strip(app, state, model, now))
        })
        .into_any_element()
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
        return model
            .strip_tiles()
            .into_iter()
            .filter(|tile| !model.hidden_for_no_video(tile))
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

pub(super) fn stage_region(
    app: &Entity<AppState>,
    state: &AppState,
    call: &ActiveCall,
    now: Instant,
    hold: Option<AnyElement>,
    share_stage: Option<AnyElement>,
) -> AnyElement {
    let model = &call.model;
    let layout = model.effective_layout();
    let stage = measured_stage(call);
    let shows_people = hold.is_none() && !(share_stage.is_some() && layout == CallLayout::Content);
    let body = match (hold, share_stage) {
        (Some(hold), _) => v_flex().absolute().inset_0().child(hold).into_any_element(),
        (None, Some(share)) => share_body(app, state, model, now, share, layout),
        (None, None) if layout == CallLayout::Speaker => {
            speaker_body(app, state, model, now, stage)
        }
        (None, None) => gallery_body(app, state, model, now, stage),
    };
    div()
        .relative()
        .debug_selector(|| "shell/islands/main/call-stage".into())
        .flex_1()
        .min_h_0()
        .w_full()
        .child(measure_stage(call))
        .child(body)
        .children(
            without_video_chip(app, model)
                .filter(|_| shows_people)
                .into_iter()
                .chain(show_me_pill(app, model, OVERLAY_INSET).filter(|_| shows_people))
                .chain(chat_bubbles(app, state, call, now, OVERLAY_INSET))
                .chain(fullscreen_hint(call, now, OVERLAY_INSET)),
        )
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use calling::{CallUpdate, RosterEntry, TileDetail, VideoWant};

    use super::{
        CallLayout, CallModel, FILMSTRIP_TILE_MAX, GALLERY_MAX, GRID_PADDING, TILE_GAP, filmstrip, split_overflow,
        video_wants,
    };

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
    fn a_big_gallery_asks_for_small_pictures_and_skips_the_plus_n_people() {
        let model = crowd(14);
        let wants = video_wants(&model, STAGE);
        assert_eq!(wants.len(), split_overflow(14, 1, GALLERY_MAX).shown);
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
}
