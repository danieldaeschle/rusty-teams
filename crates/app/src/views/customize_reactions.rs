use std::collections::HashSet;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::{Dialog, DialogFooter};
use gpui_kit::component::popover::Popover;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::reaction_picker::ReactionPicker;
use super::widgets::{Corner, corner_badge, icon};
use crate::app_state::AppState;
use crate::default_reactions::{
    filled, place_in_slot, quick_reaction_glyphs, reaction_id_for_glyph, slots_for,
};
use crate::emoji;
use crate::emoticons::{SKIN_TONE_COUNT, with_tone};
use crate::theme;

pub const CUSTOMIZE_TOOLTIP: &str = "Customise default reactions";
const TITLE: &str = "Customise your reactions and appearance in Teams";
const DESCRIPTION: &str = "Click on a reaction below, then choose an emoji to replace it with.";
const SKIN_TONE_TEXT: &str =
    "Choose a skin tone. After selection, reactions and emoji will appear with this skin tone.";
const CHOOSE_EMOJI: &str = "Choose an emoji";
const REMOVE_REACTION: &str = "Remove reaction";
const TONE_SAMPLE: &str = "\u{1F44D}";
const DIALOG_WIDTH: f32 = 440.;
const SLOT_SIZE: f32 = 48.;
const SLOT_GLYPH_SIZE: f32 = 26.;
const TONE_SIZE: f32 = 36.;
const TONE_GLYPH_SIZE: f32 = 20.;
const REMOVE_SIZE: f32 = 18.;
const REMOVE_OVERHANG: f32 = 5.;
const SECTION_GAP: f32 = 16.;
const TEXT_SIZE: f32 = 13.;

struct CustomizeForm {
    state: Entity<AppState>,
    picker: Entity<ReactionPicker>,
    saved_ids: Vec<String>,
    saved_tone: u8,
    slots: Vec<Option<String>>,
    skin_tone: u8,
}

impl CustomizeForm {
    fn new(state: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (saved_ids, saved_tone, store) = {
            let app = state.read(cx);
            (
                app.default_reactions(),
                teams_core::skin_tone(&app.store),
                app.store.clone(),
            )
        };
        let recent = cx.new(|_| emoji::Recent::load(&store));
        let picker_state = state.clone();
        let picker =
            cx.new(|cx| ReactionPicker::new(picker_state, recent, window, cx).for_customizing());
        CustomizeForm {
            state,
            picker,
            slots: slots_for(&saved_ids),
            saved_ids,
            saved_tone,
            skin_tone: saved_tone,
        }
    }

    fn unchanged(&self) -> bool {
        filled(&self.slots) == self.saved_ids && self.skin_tone == self.saved_tone
    }

    fn place(&mut self, index: usize, glyph: &str, cx: &mut Context<Self>) {
        if let Some(reaction_id) = reaction_id_for_glyph(glyph) {
            place_in_slot(&mut self.slots, index, reaction_id);
            cx.notify();
        }
    }

    fn remove(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(slot) = self.slots.get_mut(index) {
            *slot = None;
            cx.notify();
        }
    }

    fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let reaction_ids = filled(&self.slots);
        let (tone_changed, tone) = (self.skin_tone != self.saved_tone, self.skin_tone);
        self.state.update(cx, |state, cx| {
            if tone_changed {
                state.set_skin_tone(tone, cx);
            }
            state.set_default_reactions(reaction_ids, cx);
        });
        window.close_dialog(cx);
    }

    fn slot_glyph(&self, reaction_id: &str) -> Option<String> {
        quick_reaction_glyphs(&[reaction_id.to_owned()], self.skin_tone)
            .into_iter()
            .next()
    }

    fn slot(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let glyph = self
            .slots
            .get(index)
            .cloned()
            .flatten()
            .and_then(|reaction_id| self.slot_glyph(&reaction_id));
        let label = if glyph.is_some() {
            REMOVE_REACTION
        } else {
            CHOOSE_EMOJI
        };
        let trigger = Button::new(("customize-reaction-slot", index))
            .size(px(SLOT_SIZE))
            .rounded(px(10.))
            .tooltip(label)
            .map(|button| match &glyph {
                Some(glyph) => {
                    button.child(div().text_size(px(SLOT_GLYPH_SIZE)).child(glyph.clone()))
                }
                None => button.child(icon(IconName::Plus, 18., theme::text_muted())),
            });
        let (picker, view) = (self.picker.clone(), cx.entity().downgrade());
        let selected: HashSet<String> = glyph.iter().cloned().collect();
        let popover = Popover::new(("customize-reaction-picker", index))
            .anchor(Anchor::TopLeft)
            .trigger(trigger)
            .on_open_change({
                let picker = self.picker.clone();
                move |open, window, cx| {
                    if *open {
                        picker.update(cx, |picker, cx| picker.reset(window, cx));
                    }
                }
            })
            .content(move |_, _, cx| {
                let popover = cx.entity().downgrade();
                let view = view.clone();
                picker.update(cx, |picker, _| {
                    picker.set_target(
                        Rc::new(move |glyph, _, cx| {
                            view.update(cx, |form, cx| form.place(index, glyph, cx))
                                .ok();
                        }),
                        Rc::new(|_, _| {}),
                        popover,
                        selected.clone(),
                    )
                });
                picker.clone()
            });
        let remove = glyph.is_some().then(|| {
            corner_badge(Corner::TopRight, REMOVE_OVERHANG).child(
                div()
                    .id(("customize-reaction-remove", index))
                    .size(px(REMOVE_SIZE))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .cursor_pointer()
                    .bg(theme::popup_hover())
                    .hover(|button| button.bg(theme::row_hover()))
                    .child(icon(IconName::X, 10., theme::text_soft()))
                    .tooltip(|window, cx| Tooltip::new(REMOVE_REACTION).build(window, cx))
                    .on_click(cx.listener(move |form, _, _, cx| {
                        cx.stop_propagation();
                        form.remove(index, cx);
                    })),
            )
        });
        div()
            .relative()
            .child(popover)
            .children(remove)
            .into_any_element()
    }

    fn tone_choice(&self, tone: u8, cx: &mut Context<Self>) -> AnyElement {
        let selected = tone == self.skin_tone;
        div()
            .id(("customize-skin-tone", usize::from(tone)))
            .size(px(TONE_SIZE))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .cursor_pointer()
            .text_size(px(TONE_GLYPH_SIZE))
            .when(selected, |choice| choice.bg(theme::reaction_on_own()))
            .when(!selected, |choice| {
                choice.hover(|choice| choice.bg(theme::popup_hover()))
            })
            .child(with_tone(TONE_SAMPLE, tone))
            .on_click(cx.listener(move |form, _, _, cx| {
                form.skin_tone = tone;
                cx.notify();
            }))
            .into_any_element()
    }
}

impl Render for CustomizeForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let slots: Vec<AnyElement> = (0..self.slots.len())
            .map(|index| self.slot(index, cx))
            .collect();
        let tones: Vec<AnyElement> = (0..=SKIN_TONE_COUNT)
            .map(|tone| self.tone_choice(tone, cx))
            .collect();
        v_flex()
            .gap(px(SECTION_GAP))
            .text_size(px(TEXT_SIZE))
            .text_color(theme::text())
            .child(div().text_color(theme::text_soft()).child(DESCRIPTION))
            .child(
                h_flex()
                    .justify_center()
                    .gap(px(10.))
                    .py(px(4.))
                    .children(slots),
            )
            .child(div().text_color(theme::text_soft()).child(SKIN_TONE_TEXT))
            .child(h_flex().gap(px(6.)).children(tones))
    }
}

pub fn open_customize_reactions(state: Entity<AppState>, window: &mut Window, cx: &mut App) {
    let form = cx.new(|cx| CustomizeForm::new(state, window, cx));
    window.open_dialog(cx, move |dialog: Dialog, _, cx| {
        let submit_form = form.clone();
        let unchanged = form.read(cx).unchanged();
        let footer = DialogFooter::new()
            .child(
                Button::new("customize-reactions-cancel")
                    .label("Cancel")
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            )
            .child(
                Button::new("customize-reactions-save")
                    .primary()
                    .label("Save")
                    .disabled(unchanged)
                    .on_click(move |_, window, cx| {
                        submit_form.update(cx, |form, cx| form.save(window, cx));
                    }),
            );
        dialog
            .title(TITLE)
            .w(px(DIALOG_WIDTH))
            .child(form.clone())
            .footer(footer)
    });
}
