use gpui_kit::component::input::InlineToken;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{CODE_HEADER_MARK, CODE_LANGUAGES, LineKind, PLAIN_LANGUAGE};

use super::Composer;
use crate::theme::{self, ElevatedSurface as _};
use crate::views::draft_style::CODE_INDENT;
use crate::views::widgets::symbol;

const TOKEN_PREFIX: &str = "code-header-";
pub(super) const CODE_LANGUAGE_TAG: &str = "composer/code-language";
pub(super) const CODE_LANGUAGE_ROW_TAG: &str = "composer/code-language-row-";
const HEADER_HEIGHT: f32 = 36.;
/// Room between the rule and the first code line.
const CODE_TOP_GAP: f32 = 6.;
const MENU_WIDTH: f32 = 180.;
const MENU_HEIGHT: f32 = 280.;

pub(super) fn is_code_header_token(token: &InlineToken) -> bool {
    token.id().starts_with(TOKEN_PREFIX)
}

impl Composer {
    /// Every code block's first line gets its header token in place of the header mark.
    pub(super) fn sync_code_headers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).value().as_ref() != self.draft.text() {
            return;
        }
        let tokens: Vec<_> = self
            .input
            .read(cx)
            .tokens()
            .iter()
            .filter(|span| is_code_header_token(span.token()))
            .map(|span| span.range())
            .collect();
        let marks: Vec<_> = self
            .draft
            .text()
            .match_indices(CODE_HEADER_MARK)
            .map(|(at, mark)| at..at + mark.len())
            .filter(|mark| !tokens.contains(mark))
            .collect();
        for mark in marks {
            self.table_counter += 1;
            let token = InlineToken::new(
                format!("{TOKEN_PREFIX}{}", self.table_counter),
                CODE_HEADER_MARK.to_string(),
            )
            .header();
            self.input.update(cx, |state, cx| {
                let selection = state.selected_range();
                if state
                    .replace_range_with_token(mark, token, window, cx)
                    .is_ok()
                {
                    state.set_selected_range(selection, cx);
                }
            });
        }
    }

    /// Teams' code block header: the language, which opens its picker, over a rule.
    pub(super) fn render_code_header(
        &self,
        composer: WeakEntity<Composer>,
        at: usize,
        width: Pixels,
    ) -> AnyElement {
        let line = self.draft.line_at(at);
        let language = match self.draft.lines().get(line) {
            Some(LineKind::Code(language)) => language.clone(),
            _ => None,
        };
        let chosen = language.unwrap_or_else(|| PLAIN_LANGUAGE.to_owned());
        let name = teams_core::code_language(&chosen)
            .map_or(chosen.clone(), |language| language.name.to_owned());
        let open = self.code_language_menu == Some(line);
        let toggle = composer.clone();
        let button = h_flex()
            .id(ElementId::Name(format!("code-language-{at}").into()))
            .debug_selector(|| CODE_LANGUAGE_TAG.into())
            .h(px(24.))
            .px(px(6.))
            .gap(px(4.))
            .items_center()
            .rounded(px(4.))
            .text_size(px(13.))
            .text_color(theme::text())
            .cursor_pointer()
            .hover(|button| button.bg(theme::popup_hover()))
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_click(move |_, _, cx| {
                toggle
                    .update(cx, |this, cx| {
                        this.code_language_menu = (!open).then_some(line);
                        cx.notify();
                    })
                    .ok();
            })
            .child(symbol("code", 14., theme::text_muted()))
            .child(name)
            .child(symbol("keyboard_arrow_down", 14., theme::text_muted()));
        v_flex()
            .w(width)
            .pb(px(CODE_TOP_GAP))
            .child(
                h_flex()
                    .h(px(HEADER_HEIGHT))
                    .px(px(6.))
                    .items_center()
                    .border_b_1()
                    .border_color(theme::code_frame())
                    .child(button),
            )
            .into_any_element()
    }

    /// The open picker, drawn over the composer: inside the input it could not be clicked
    /// where it reaches past the input's bounds.
    pub(super) fn render_code_language_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let line = self.code_language_menu?;
        let at = self.draft.line_ranges().get(line)?.start;
        let LineKind::Code(language) = self.draft.lines().get(line)? else {
            return None;
        };
        let chosen = language
            .as_deref()
            .and_then(teams_core::code_language)
            .map_or(PLAIN_LANGUAGE, |language| language.id);
        let header = self.input.read(cx).range_to_bounds(&(at..at))?;
        let rows = CODE_LANGUAGES.iter().map(|language| {
            let (id, name) = (language.id, language.name);
            div()
                .id(ElementId::Name(format!("code-language-{id}").into()))
                .debug_selector(move || format!("{CODE_LANGUAGE_ROW_TAG}{id}"))
                .px(px(10.))
                .py(px(5.))
                .rounded(px(6.))
                .text_size(px(13.))
                .text_color(theme::text())
                .cursor_pointer()
                .when(id == chosen, |row| row.bg(theme::popup_hover()))
                .hover(|row| row.bg(theme::popup_hover()))
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.choose_code_language(at, id, window, cx)
                }))
                .child(name)
        });
        // Lined up with the block's edge, left of the line indent where `at` sits.
        let left = header.left() - px(CODE_INDENT);
        Some(
            deferred(
                // Above the header, as in Teams: the composer sits at the window's bottom.
                anchored()
                    .position(point(left, header.top()))
                    .anchor(Anchor::BottomLeft)
                    .snap_to_window_with_margin(px(8.))
                    .child(
                        v_flex()
                            .id("code-language-menu")
                            .mb(px(4.))
                            .w(px(MENU_WIDTH))
                            .max_h(px(MENU_HEIGHT))
                            .overflow_y_scroll()
                            .p(px(4.))
                            .rounded(px(10.))
                            .flyout_surface()
                            .occlude()
                            .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                                this.code_language_menu = None;
                                cx.notify();
                            }))
                            .children(rows),
                    ),
            )
            .with_priority(1)
            .into_any_element(),
        )
    }

    pub(super) fn choose_code_language(
        &mut self,
        at: usize,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let language = (id != PLAIN_LANGUAGE).then(|| id.to_owned());
        self.draft.set_code_language(at, language);
        self.code_language_menu = None;
        self.commit(None, window, cx);
        cx.notify();
    }
}
