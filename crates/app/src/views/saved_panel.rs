use chrono::{DateTime, Local, Offset};
use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::*;
use teams_core::SavedMessage;

use super::avatar::person_avatar;
use super::title_bar::TITLE_BAR_HEIGHT;
use super::widgets::icon;
use crate::app_state::{AppState, selection_title};
use crate::format;
use crate::notify::selection_for;
use crate::theme;

const PANEL_WIDTH: f32 = 420.;
const PANEL_MAX_HEIGHT_RATIO: f32 = 0.7;
const PANEL_MARGIN: f32 = 8.;
const AVATAR_SIZE: f32 = 32.;
const BUTTON_SIZE: f32 = 24.;
const PREVIEW_LINES: usize = 2;
const EMPTY_TEXT: &str = "No saved messages. Use Save message in the ... menu of a message.";
const UNKNOWN_AUTHOR: &str = "Unknown";

pub enum SavedPanelEvent {
    Close,
    Open {
        conversation_id: String,
        message_id: String,
    },
}

pub struct SavedPanel {
    app: Entity<AppState>,
    focus_handle: FocusHandle,
    scroll: ScrollHandle,
    _observation: Subscription,
}

impl EventEmitter<SavedPanelEvent> for SavedPanel {}

pub fn count_label(count: usize) -> String {
    match count {
        0 => String::new(),
        count => count.to_string(),
    }
}

impl SavedPanel {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle, cx);
        let observation = cx.observe(&app, |_, _, cx| cx.notify());
        app.update(cx, |state, cx| state.refresh_saved(cx));
        SavedPanel {
            app,
            focus_handle,
            scroll: ScrollHandle::new(),
            _observation: observation,
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if event.keystroke.key == "escape" {
            cx.emit(SavedPanelEvent::Close);
        }
    }

    fn header(&self, count: usize) -> Div {
        h_flex()
            .px(px(12.))
            .pt(px(10.))
            .pb(px(8.))
            .gap(px(8.))
            .items_center()
            .border_b_1()
            .border_color(theme::border())
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text())
                    .child("Saved"),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme::text_muted())
                    .child(count_label(count)),
            )
    }

    fn row(
        &self,
        item: &SavedMessage,
        index: usize,
        title: String,
        panel: Entity<SavedPanel>,
        cx: &App,
    ) -> AnyElement {
        let app = self.app.clone();
        let state = self.app.read(cx);
        let now = Local::now();
        let author = item.author_name.as_deref().unwrap_or(UNKNOWN_AUTHOR);
        let group = SharedString::from(format!("saved-row-group-{index}"));
        let (conversation_id, message_id) = (item.conversation_id.clone(), item.message_id.clone());
        let unsave_item = item.clone();
        h_flex()
            .id(ElementId::Name(format!("saved-row-{index}").into()))
            .group(group.clone())
            .mx(px(6.))
            .px(px(6.))
            .py(px(8.))
            .gap(px(10.))
            .items_start()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|row| row.bg(theme::row_hover()))
            .child(div().flex_none().size(px(AVATAR_SIZE)).child(person_avatar(
                &state.directory,
                item.author_id.as_deref(),
                author,
                AVATAR_SIZE,
            )))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        h_flex()
                            .gap(px(6.))
                            .items_baseline()
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(13.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text())
                                    .child(author.to_owned()),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(theme::text_muted())
                                    .child(title),
                            ),
                    )
                    .child(
                        div()
                            .line_clamp(PREVIEW_LINES)
                            .text_size(px(12.5))
                            .text_color(theme::text_soft())
                            .child(item.preview.clone()),
                    ),
            )
            .child(
                v_flex()
                    .flex_none()
                    .items_end()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(theme::text_muted())
                            .child(saved_time(item.saved_at, now)),
                    )
                    .child(
                        div()
                            .id(ElementId::Name(format!("saved-unsave-{index}").into()))
                            .size(px(BUTTON_SIZE))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .rounded(px(6.))
                            .cursor_pointer()
                            .opacity(0.)
                            .group_hover(group, |button| button.opacity(1.))
                            .hover(|button| button.bg(theme::border_strong()))
                            .child(icon(IconName::BookmarkOff, 14., theme::text_muted()))
                            .tooltip(|window, cx| Tooltip::new("Unsave").build(window, cx))
                            .on_click(move |_, _, cx| {
                                cx.stop_propagation();
                                let entry = unsave_item.clone();
                                app.update(cx, |state, cx| state.set_saved(entry, false, cx));
                            }),
                    ),
            )
            .on_click(move |_, _, cx| {
                let event = SavedPanelEvent::Open {
                    conversation_id: conversation_id.clone(),
                    message_id: message_id.clone(),
                };
                panel.update(cx, |_, cx| cx.emit(event));
            })
            .into_any_element()
    }
}

fn saved_time(saved_at: DateTime<chrono::Utc>, now: DateTime<Local>) -> String {
    format::list_time_label(saved_at, now.date_naive(), now.offset().fix())
}

impl Render for SavedPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let panel = cx.entity();
        let state = self.app.read(cx);
        let items = state.saved.items().to_vec();
        let mut rows: Vec<AnyElement> = items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let title = selection_for(&state.sidebar, &item.conversation_id)
                    .map(|selection| selection_title(&state.sidebar, &selection))
                    .or_else(|| item.topic.clone())
                    .unwrap_or_default();
                self.row(item, index, title, panel.clone(), cx)
            })
            .collect();
        if rows.is_empty() {
            rows.push(
                div()
                    .py(px(32.))
                    .px(px(24.))
                    .flex()
                    .justify_center()
                    .text_center()
                    .text_size(px(13.))
                    .text_color(theme::text_muted())
                    .child(EMPTY_TEXT)
                    .into_any_element(),
            );
        }
        let max_height = f32::from(window.viewport_size().height) * PANEL_MAX_HEIGHT_RATIO;
        div()
            .id("saved-layer")
            .absolute()
            .top(px(TITLE_BAR_HEIGHT))
            .left_0()
            .right_0()
            .bottom_0()
            .occlude()
            .flex()
            .justify_end()
            .items_start()
            .p(px(PANEL_MARGIN))
            .on_click(cx.listener(|_, _, _, cx| cx.emit(SavedPanelEvent::Close)))
            .child(
                v_flex()
                    .id("saved-card")
                    .track_focus(&self.focus_handle)
                    .w(px(PANEL_WIDTH))
                    .max_h(px(max_height))
                    .rounded(px(10.))
                    .border_1()
                    .border_color(theme::border_strong())
                    .bg(theme::surface())
                    .shadow_lg()
                    .overflow_hidden()
                    .occlude()
                    .on_click(|_, _, cx| cx.stop_propagation())
                    .on_key_down(cx.listener(Self::on_key_down))
                    .child(self.header(items.len()))
                    .child(
                        v_flex()
                            .id("saved-list")
                            .flex_1()
                            .min_h_0()
                            .py(px(4.))
                            .overflow_y_scroll()
                            .track_scroll(&self.scroll)
                            .children(rows),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::count_label;

    #[test]
    fn the_header_count_is_hidden_when_nothing_is_saved() {
        assert_eq!(count_label(0), "");
        assert_eq!(count_label(2), "2");
    }
}
