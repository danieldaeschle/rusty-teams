use std::rc::Rc;

use chrono::Local;
use gpui_kit::base::GlobalState;
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::reaction_picker::PickHandler;
use crate::data::Directory;
use crate::reaction_model::{
    ReactionEntry, ReactionTab, Reactor, chip_tooltip, reaction_entries, reaction_tabs,
    reaction_time_label, shows_faces,
};
use crate::rows::ReactionChip;
use crate::theme;

const PILL_HEIGHT: f32 = 24.;
const PILL_EMOJI_SIZE: f32 = 14.;
const PILL_TEXT_SIZE: f32 = 12.;
const PILL_GAP: f32 = 3.;
const FACE_SIZE: f32 = 18.;
const FACE_RING: f32 = 1.5;
const FACE_OVERLAP: f32 = 6.;
const POPOVER_WIDTH: f32 = 290.;
const POPOVER_MAX_HEIGHT: f32 = 320.;
const POPOVER_GAP: f32 = 4.;
const TAB_HEIGHT: f32 = 26.;
const ENTRY_HEIGHT: f32 = 48.;
const ENTRY_AVATAR_SIZE: f32 = 32.;

type OpenReaction = Rc<dyn Fn(&str, &mut App)>;
type SelectTab = Rc<dyn Fn(Option<String>, &mut App)>;

#[derive(Clone)]
pub struct ReactionPopover {
    pub anchor_glyph: String,
    pub tab: Option<String>,
}

#[derive(Clone)]
pub struct ReactionControls {
    pub open: OpenReaction,
    pub select_tab: SelectTab,
    pub close: Rc<dyn Fn(&mut App)>,
    pub popover: Option<ReactionPopover>,
}

fn face(position: usize, reactor: &Reactor, ring: Hsla, directory: &Directory) -> Div {
    let step = FACE_SIZE + 2. * FACE_RING - FACE_OVERLAP;
    div()
        .absolute()
        .top_0()
        .left(px(position as f32 * step))
        .size(px(FACE_SIZE + 2. * FACE_RING))
        .p(px(FACE_RING))
        .rounded_full()
        .bg(ring)
        .child(person_avatar(
            directory,
            reactor.user_id.as_deref(),
            &reactor.name,
            FACE_SIZE,
        ))
}

fn faces_width(count: usize) -> f32 {
    let diameter = FACE_SIZE + 2. * FACE_RING;
    diameter + count.saturating_sub(1) as f32 * (diameter - FACE_OVERLAP)
}

fn pill_content(chip: &ReactionChip, background: Hsla, directory: &Directory) -> Div {
    let content = h_flex().items_center().child(
        div()
            .text_size(px(PILL_EMOJI_SIZE))
            .child(chip.label.clone()),
    );
    if shows_faces(chip.reactors.len()) {
        content.child(
            div()
                .relative()
                .flex_none()
                .ml(px(PILL_GAP - FACE_RING))
                .w(px(faces_width(chip.reactors.len())))
                .h(px(FACE_SIZE + 2. * FACE_RING))
                .children(
                    chip.reactors
                        .iter()
                        .enumerate()
                        .map(|(position, reactor)| face(position, reactor, background, directory)),
                ),
        )
    } else {
        content
            .gap(px(PILL_GAP))
            .pr(px(1.))
            .child(chip.count.to_string())
    }
}

fn pill(
    chip: &ReactionChip,
    index: usize,
    own: bool,
    directory: &Directory,
    react: Option<PickHandler>,
    controls: Option<&ReactionControls>,
) -> Stateful<Div> {
    let (background, foreground) = if chip.mine {
        (theme::accent(), theme::white())
    } else if own {
        (theme::reaction_on_own(), theme::text_soft())
    } else {
        (theme::border_strong(), theme::text_soft())
    };
    let glyph = chip.label.clone();
    let tooltip_text = chip_tooltip(chip);
    let toggle = controls.map(|controls| {
        let is_open = controls
            .popover
            .as_ref()
            .is_some_and(|state| state.anchor_glyph == chip.glyph());
        (
            controls.open.clone(),
            controls.close.clone(),
            chip.glyph(),
            is_open,
        )
    });
    h_flex()
        .id(ElementId::Name(
            format!("reaction-{index}-{}", chip.reaction_type).into(),
        ))
        .h(px(PILL_HEIGHT))
        .flex_none()
        .pl(px(5.))
        .pr(px(2.))
        .items_center()
        .rounded(px(PILL_HEIGHT / 2.))
        .bg(background)
        .text_size(px(PILL_TEXT_SIZE))
        .line_height(px(PILL_HEIGHT))
        .text_color(foreground)
        .child(pill_content(chip, background, directory))
        .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
        .when_some(toggle, |pill, (open, close, glyph, is_open)| {
            pill.on_mouse_down(MouseButton::Right, move |_, _, cx| {
                cx.stop_propagation();
                if is_open {
                    close(cx);
                } else {
                    open(&glyph, cx);
                }
            })
        })
        .when_some(react, |pill, react| {
            pill.cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| {
                    GlobalState::suppress_text_selection(cx)
                })
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    react(&glyph, window, cx);
                })
        })
}

fn tab_button(tab: &ReactionTab, active: bool, select_tab: SelectTab) -> Stateful<Div> {
    let glyph = tab.glyph.clone();
    h_flex()
        .id(ElementId::Name(
            format!("reaction-tab-{}", glyph.as_deref().unwrap_or("all")).into(),
        ))
        .h(px(TAB_HEIGHT))
        .px(px(10.))
        .gap(px(4.))
        .items_center()
        .rounded(px(7.))
        .cursor_pointer()
        .text_size(px(12.5))
        .text_color(if active {
            theme::text()
        } else {
            theme::text_muted()
        })
        .when(active, |button| button.bg(theme::border_strong()))
        .child(tab.label.clone())
        .child(tab.count.to_string())
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            select_tab(glyph.clone(), cx);
        })
}

fn entry_row(entry: &ReactionEntry, directory: &Directory) -> Div {
    h_flex()
        .h(px(ENTRY_HEIGHT))
        .px(px(8.))
        .gap(px(10.))
        .items_center()
        .child(person_avatar(
            directory,
            entry.reactor.user_id.as_deref(),
            &entry.reactor.name,
            ENTRY_AVATAR_SIZE,
        ))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .text_size(px(13.5))
                        .text_color(theme::text())
                        .truncate()
                        .child(entry.reactor.name.clone()),
                )
                .children(
                    entry
                        .reactor
                        .created_at
                        .map(|time| reaction_time_label(time, &Local::now()))
                        .map(|time| {
                            div()
                                .text_size(px(11.5))
                                .text_color(theme::text_muted())
                                .child(time)
                        }),
                ),
        )
        .child(div().text_size(px(17.)).child(entry.label.clone()))
}

fn popover(
    chips: &[ReactionChip],
    selected: Option<&str>,
    directory: &Directory,
    controls: &ReactionControls,
) -> AnyElement {
    let selected = selected.filter(|glyph| chips.iter().any(|chip| chip.glyph() == *glyph));
    let tabs = reaction_tabs(chips);
    let entries = reaction_entries(chips, selected);
    let close = controls.close.clone();
    let panel = v_flex()
        .id("reaction-popover")
        .w(px(POPOVER_WIDTH))
        .p(px(6.))
        .gap(px(4.))
        .rounded(px(12.))
        .border_1()
        .border_color(theme::border_strong())
        .bg(theme::surface_raised())
        .shadow_lg()
        .occlude()
        .on_mouse_down_out(move |_, _, cx| close(cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| {
            GlobalState::suppress_text_selection(cx);
            cx.stop_propagation();
        })
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
        .when(!tabs.is_empty(), |panel| {
            panel.child(h_flex().gap(px(2.)).children(tabs.iter().map(|tab| {
                tab_button(
                    tab,
                    tab.glyph.as_deref() == selected,
                    controls.select_tab.clone(),
                )
            })))
        })
        .child(
            v_flex()
                .id("reaction-popover-entries")
                .max_h(px(POPOVER_MAX_HEIGHT))
                .overflow_y_scroll()
                .children(entries.iter().map(|entry| entry_row(entry, directory))),
        );
    deferred(
        anchored()
            .anchor(Anchor::TopLeft)
            .offset(point(px(0.), px(PILL_HEIGHT + POPOVER_GAP)))
            .snap_to_window_with_margin(px(8.))
            .child(panel),
    )
    .with_priority(2)
    .into_any_element()
}

pub fn reaction_pills(
    chips: &[ReactionChip],
    index: usize,
    own: bool,
    directory: &Directory,
    react: Option<PickHandler>,
    controls: Option<&ReactionControls>,
) -> Div {
    h_flex()
        .gap(px(4.))
        .flex_wrap()
        .children(chips.iter().map(|chip| {
            let open_here = controls
                .and_then(|controls| controls.popover.as_ref().map(|state| (controls, state)))
                .filter(|(_, state)| state.anchor_glyph == chip.glyph());
            div()
                .relative()
                .flex_none()
                .child(pill(chip, index, own, directory, react.clone(), controls))
                .when_some(open_here, |wrapper, (controls, state)| {
                    wrapper.child(popover(chips, state.tab.as_deref(), directory, controls))
                })
        }))
}
