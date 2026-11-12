use gpui_kit::component::input::InlineToken;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{LineKind, RULE_MARK};

use super::Composer;
use crate::theme;

const TOKEN_PREFIX: &str = "rule-";
pub(super) const RULE_TOKEN_TAG: &str = "composer/rule";

fn token(number: usize) -> InlineToken {
    InlineToken::new(format!("{TOKEN_PREFIX}{number}"), RULE_MARK.to_string()).block()
}

pub(super) fn is_rule_token(token: &InlineToken) -> bool {
    token.id().starts_with(TOKEN_PREFIX)
}

/// A horizontal rule line across the composer, like Teams' `ck-horizontal-line`.
pub(super) fn render_rule_token(width: Pixels, height: Pixels, selected: bool) -> AnyElement {
    div()
        .debug_selector(|| RULE_TOKEN_TAG.into())
        .w(width)
        .h(height)
        .flex()
        .items_center()
        .rounded(px(4.))
        .when(selected, |rule| rule.bg(theme::selection()))
        .child(div().w_full().h(px(1.)).bg(theme::border_strong()))
        .into_any_element()
}

impl Composer {
    /// Every rule line gets its block token.
    pub(super) fn sync_rules(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.read(cx).value().as_ref() != self.draft.text() {
            return;
        }
        let tokens: Vec<_> = self
            .input
            .read(cx)
            .tokens()
            .iter()
            .filter(|span| is_rule_token(span.token()))
            .map(|span| span.range())
            .collect();
        let ranges = self.draft.line_ranges();
        for (index, line) in ranges.into_iter().enumerate() {
            if self.draft.lines()[index] != LineKind::Rule || tokens.contains(&line) {
                continue;
            }
            self.table_counter += 1;
            let token = token(self.table_counter);
            self.input.update(cx, |state, cx| {
                let selection = state.selected_range();
                if state
                    .replace_range_with_token(line, token, window, cx)
                    .is_ok()
                {
                    state.set_selected_range(selection, cx);
                }
            });
        }
    }
}
