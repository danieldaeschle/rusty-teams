use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::avatar::person_avatar;
use super::switcher::clamp_highlight;
use super::widgets::icon;
use crate::app_state::AppState;
use crate::call::TransferCandidate;
use crate::fuzzy;
use crate::theme;

const MAX_RESULTS: usize = 6;
const CARD_WIDTH: f32 = 440.;
const CARD_RADIUS: f32 = 12.;
const CARD_PADDING: f32 = 16.;
const SECTION_GAP: f32 = 12.;
const BACKDROP_OPACITY: f32 = 0.55;
const ROW_HEIGHT: f32 = 44.;
const AVATAR_SIZE: f32 = 28.;
const FIELD_HEIGHT: f32 = 36.;
const CLOSE_SIZE: f32 = 28.;
const DISABLED_OPACITY: f32 = 0.4;

pub fn visible_candidates(candidates: &[(String, TransferCandidate)], query: &str) -> Vec<TransferCandidate> {
    fuzzy::rank(query, candidates, MAX_RESULTS)
}

pub struct TransferPicker {
    app: Entity<AppState>,
    candidates: Vec<(String, TransferCandidate)>,
    search: Entity<InputState>,
    results: Vec<TransferCandidate>,
    highlighted: usize,
    target: Option<TransferCandidate>,
    _subscription: Subscription,
}

impl TransferPicker {
    pub fn new(app: Entity<AppState>, candidates: Vec<TransferCandidate>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let candidates: Vec<(String, TransferCandidate)> = candidates.into_iter().map(|candidate| (candidate.name.clone(), candidate)).collect();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search people"));
        let subscription = cx.subscribe_in(&search, window, Self::on_search_event);
        search.update(cx, |input, cx| input.focus(window, cx));
        let results = visible_candidates(&candidates, "");
        TransferPicker { app, candidates, search, results, highlighted: 0, target: None, _subscription: subscription }
    }

    pub fn choose(&mut self, candidate: TransferCandidate) {
        self.target = Some(candidate);
    }

    fn on_search_event(&mut self, input: &Entity<InputState>, event: &InputEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event {
            InputEvent::Change => {
                let query = input.read(cx).value().to_string();
                self.results = visible_candidates(&self.candidates, &query);
                self.highlighted = 0;
                cx.notify();
            }
            InputEvent::PressEnter { .. } => {
                if let Some(candidate) = self.results.get(self.highlighted).cloned() {
                    self.target = Some(candidate);
                    cx.notify();
                }
            }
            _ => {}
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        match event.keystroke.key.as_str() {
            "escape" => self.close(cx),
            "down" => {
                self.highlighted = clamp_highlight(self.highlighted, 1, self.results.len());
                cx.notify();
            }
            "up" => {
                self.highlighted = clamp_highlight(self.highlighted, -1, self.results.len());
                cx.notify();
            }
            _ => {}
        }
    }

    fn close(&self, cx: &mut Context<Self>) {
        self.app.update(cx, |state, cx| state.close_transfer_picker(cx));
    }

    fn list(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let directory = &self.app.read(cx).directory;
        let rows: Vec<AnyElement> = self
            .results
            .iter()
            .enumerate()
            .map(|(index, candidate)| {
                let chosen = self.target.as_ref() == Some(candidate);
                let picked = candidate.clone();
                h_flex()
                    .id(ElementId::Name(format!("transfer-target-{index}").into()))
                    .h(px(ROW_HEIGHT))
                    .flex_none()
                    .px(px(8.))
                    .gap(px(10.))
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .when(chosen || (self.target.is_none() && index == self.highlighted), |row| row.bg(theme::surface_raised()))
                    .hover(|row| row.bg(theme::row_hover()))
                    .child(person_avatar(directory, candidate.user_id.as_deref(), &candidate.name, AVATAR_SIZE))
                    .child(div().flex_1().min_w_0().truncate().text_size(px(14.)).child(candidate.name.clone()))
                    .when(chosen, |row| row.child(icon(IconName::Check, 16., theme::accent_text())))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.choose(picked.clone());
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        v_flex().id("transfer-targets").w_full().max_h(px(ROW_HEIGHT * MAX_RESULTS as f32)).overflow_y_scroll().children(rows)
    }

    fn action(&self, id: &'static str, label: &'static str, primary: bool, run: fn(&mut AppState, TransferCandidate, &mut Context<AppState>)) -> AnyElement {
        let button = Button::new(id).label(label);
        let button = if primary { button } else { button.ghost() };
        let Some(target) = self.target.clone() else {
            return button.opacity(DISABLED_OPACITY).into_any_element();
        };
        let app = self.app.clone();
        button.on_click(move |_, _, cx| app.update(cx, |state, cx| run(state, target.clone(), cx))).into_any_element()
    }
}

impl Render for TransferPicker {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let field = h_flex()
            .w_full()
            .h(px(FIELD_HEIGHT))
            .px(px(10.))
            .gap(px(8.))
            .items_center()
            .rounded(px(6.))
            .bg(theme::surface())
            .border_1()
            .border_color(theme::border())
            .child(icon(IconName::Search, 14., theme::text_muted()))
            .child(div().flex_1().child(Input::new(&self.search).appearance(false).bordered(false)));
        let actions = h_flex()
            .w_full()
            .justify_end()
            .gap(px(8.))
            .child(Button::new("transfer-cancel").ghost().label("Cancel").on_click(cx.listener(|this, _, _, cx| this.close(cx))))
            .child(self.action("transfer-consult", "Consult first", false, AppState::start_consult))
            .child(self.action("transfer-now", "Transfer now", true, AppState::transfer_call_blind));
        div()
            .id("transfer-layer")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .child(
                div()
                    .id("transfer-backdrop")
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .bg(black().opacity(BACKDROP_OPACITY))
                    .occlude()
                    .on_click(cx.listener(|this, _, _, cx| this.close(cx))),
            )
            .child(
                v_flex()
                    .id("transfer-card")
                    .w(px(CARD_WIDTH))
                    .max_w(relative(1.))
                    .p(px(CARD_PADDING))
                    .gap(px(SECTION_GAP))
                    .rounded(px(CARD_RADIUS))
                    .bg(theme::background())
                    .border_1()
                    .border_color(theme::border_strong())
                    .text_color(theme::text())
                    .shadow_lg()
                    .occlude()
                    .on_key_down(cx.listener(Self::on_key_down))
                    .child(
                        h_flex()
                            .w_full()
                            .items_center()
                            .gap(px(SECTION_GAP))
                            .child(div().flex_1().text_size(px(15.)).font_weight(FontWeight::SEMIBOLD).child("Transfer call"))
                            .child(
                                div()
                                    .id("transfer-close")
                                    .size(px(CLOSE_SIZE))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .cursor_pointer()
                                    .hover(|button| button.bg(theme::row_hover()))
                                    .on_click(cx.listener(|this, _, _, cx| this.close(cx)))
                                    .child(icon(IconName::X, 16., theme::text_muted())),
                            ),
                    )
                    .child(field)
                    .child(self.list(cx))
                    .child(actions),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{MAX_RESULTS, visible_candidates};
    use crate::call::TransferCandidate;

    fn candidate(name: &str) -> (String, TransferCandidate) {
        (name.to_owned(), TransferCandidate { mri: format!("8:orgid:{name}"), name: name.to_owned(), user_id: Some(name.to_owned()), chat_id: format!("19:{name}") })
    }

    #[test]
    fn the_search_ranks_people_by_name() {
        let people = vec![candidate("Ana Weiss"), candidate("Bo Brandt"), candidate("Anja Vogel")];
        let found: Vec<String> = visible_candidates(&people, "anj").into_iter().map(|person| person.name).collect();
        assert_eq!(found.first().map(String::as_str), Some("Anja Vogel"));
        assert_eq!(visible_candidates(&people, "").len(), 3);
        assert!(visible_candidates(&people, "zzz").is_empty());
    }

    #[test]
    fn the_list_is_capped() {
        let people: Vec<(String, TransferCandidate)> = (0..20).map(|index| candidate(&format!("Person {index}"))).collect();
        assert_eq!(visible_candidates(&people, "").len(), MAX_RESULTS);
    }
}
