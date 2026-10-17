use std::sync::{Arc, OnceLock};

use gpui_kit::assets::IconName;
use gpui_kit::component::{TitleBar, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::widgets::icon;
use crate::app_state::Mode;
use crate::theme;
use crate::updater::UpdateStatus;

pub const TITLE_BAR_HEIGHT: f32 = 40.;
const SEARCH_WIDTH: f32 = 380.;
const SEARCH_MIN_WIDTH: f32 = 240.;
const SEARCH_MIN_VISIBLE_WIDTH: f32 = 36.;
const SEARCH_HEIGHT: f32 = 28.;
const SEARCH_GAP: f32 = 8.;
const SHORTCUT_MIN_WIDTH: f32 = 200.;
const SEARCH_HINT: &str = "Search chats, people and channels";
const SHORTCUT_HINT: &str = "Ctrl K";
const APP_ICON_SIZE: f32 = 18.;
const APP_ICON_PNG: &[u8] = include_bytes!("../../assets/icon/teams-fast-256.png");

fn app_icon() -> Arc<Image> {
    static ICON: OnceLock<Arc<Image>> = OnceLock::new();
    ICON.get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, APP_ICON_PNG.to_vec())))
        .clone()
}

fn mode_pill(label: &'static str) -> Div {
    div()
        .debug_selector(|| "shell/title-bar/mode-pill".into())
        .flex_none()
        .px(px(8.))
        .h(px(18.))
        .flex()
        .items_center()
        .rounded_full()
        .border_1()
        .border_color(theme::amber().opacity(0.5))
        .bg(theme::amber().opacity(0.14))
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::amber())
        .child(label)
}

pub fn mode_label(mode: Mode) -> Option<&'static str> {
    if mode.demo {
        Some("Demo")
    } else if mode.read_only {
        Some("Read-only")
    } else {
        None
    }
}

pub fn update_button(
    update: &UpdateStatus,
    on_restart: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Option<AnyElement> {
    let installing = match update {
        UpdateStatus::Ready => false,
        UpdateStatus::Installing => true,
        UpdateStatus::UpToDate | UpdateStatus::Failed(_) => return None,
    };
    Some(
        div()
            .id("title-update-restart")
            .debug_selector(|| "shell/title-bar/update".into())
            .flex_none()
            .h(px(24.))
            .px(px(10.))
            .flex()
            .items_center()
            .rounded(px(6.))
            .bg(theme::accent())
            .text_color(theme::on_accent())
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .when(!installing, |button| {
                button
                    .cursor_pointer()
                    .hover(|button| button.bg(theme::accent_text()))
                    .on_click(on_restart)
            })
            .when(installing, |button| button.opacity(0.7))
            .child(if installing {
                "Restarting..."
            } else {
                "Restart to update"
            })
            .into_any_element(),
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct SearchSpan {
    left: f32,
    width: f32,
}

fn search_span(window_width: f32, leading_end: f32, trailing_start: f32) -> Option<SearchSpan> {
    let start = leading_end + SEARCH_GAP;
    let end = trailing_start - SEARCH_GAP;
    let room = end - start;
    if room < SEARCH_MIN_VISIBLE_WIDTH {
        return None;
    }
    let center = window_width / 2.;
    let centered_width = (2. * (center - start).min(end - center)).min(SEARCH_WIDTH);
    let width = centered_width.max(SEARCH_MIN_WIDTH).min(room);
    let left = (center - width / 2.).clamp(start, end - width);
    Some(SearchSpan { left, width })
}

#[derive(Clone, Copy, PartialEq)]
struct Reserved {
    leading_end: f32,
    trailing_from_right: f32,
}

#[derive(IntoElement)]
struct TitleBarRow {
    mode_label: Option<&'static str>,
    update: Option<AnyElement>,
    own: Option<AnyElement>,
    on_search: Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
}

pub fn render_title_bar(
    mode_label: Option<&'static str>,
    update: Option<AnyElement>,
    own: Option<AnyElement>,
    on_search: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    TitleBarRow {
        mode_label,
        update,
        own,
        on_search: Box::new(on_search),
    }
}

fn search_box(
    span: SearchSpan,
    on_search: Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
) -> impl IntoElement {
    h_flex()
        .id("title-search")
        .debug_selector(|| "shell/title-bar/search".into())
        .occlude()
        .absolute()
        .top(px((TITLE_BAR_HEIGHT - SEARCH_HEIGHT) / 2.))
        .left(px(span.left))
        .w(px(span.width))
        .h(px(SEARCH_HEIGHT))
        .px(px(10.))
        .gap(px(8.))
        .items_center()
        .overflow_hidden()
        .rounded(px(6.))
        .bg(theme::surface())
        .cursor_pointer()
        .text_size(px(12.))
        .text_color(theme::text_muted())
        .child(
            div()
                .flex_none()
                .child(icon(IconName::Search, 14., theme::text_muted())),
        )
        .child(div().flex_1().min_w_0().truncate().child(SEARCH_HINT))
        .when(span.width >= SHORTCUT_MIN_WIDTH, |search| {
            search.child(
                div()
                    .flex_none()
                    .px(px(6.))
                    .py(px(1.))
                    .rounded(px(4.))
                    .bg(theme::surface_raised())
                    .text_size(px(11.))
                    .child(SHORTCUT_HINT),
            )
        })
        .on_click(move |event, window, cx| on_search(event, window, cx))
}

impl RenderOnce for TitleBarRow {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let reserved = window.use_keyed_state("title-bar-reserved", cx, |_, _| None::<Reserved>);
        let window_width = f32::from(window.viewport_size().width);
        let span = (*reserved.read(cx)).and_then(|reserved| {
            search_span(
                window_width,
                reserved.leading_end,
                window_width - reserved.trailing_from_right,
            )
        });
        let leading = h_flex()
            .debug_selector(|| "shell/title-bar/leading".into())
            .flex_none()
            .h_full()
            .gap(px(12.))
            .items_center()
            .child(img(app_icon()).size(px(APP_ICON_SIZE)).flex_none())
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text())
                    .child(app_base::APP_NAME),
            )
            .children(self.mode_label.map(mode_pill));
        let trailing = h_flex()
            .debug_selector(|| "shell/title-bar/trailing".into())
            .flex_none()
            .h_full()
            .gap(px(12.))
            .items_center()
            .children(self.update)
            .children(self.own)
            .child(div().w(px(4.)));
        let row = h_flex()
            .flex_1()
            .h_full()
            .gap(px(12.))
            .items_center()
            .child(leading)
            .child(div().flex_1())
            .child(trailing)
            .on_children_prepainted(move |children, window, cx| {
                let [leading, _, trailing] = children[..] else {
                    return;
                };
                let measured = Reserved {
                    leading_end: f32::from(leading.right()),
                    trailing_from_right: f32::from(window.viewport_size().width - trailing.left()),
                };
                reserved.update(cx, |current, cx| {
                    let unchanged = current.is_some_and(|current| {
                        (current.leading_end - measured.leading_end).abs() < 0.5
                            && (current.trailing_from_right - measured.trailing_from_right).abs()
                                < 0.5
                    });
                    if !unchanged {
                        *current = Some(measured);
                        cx.notify();
                    }
                });
            });
        let bar = TitleBar::new()
            .h(px(TITLE_BAR_HEIGHT))
            .border_b_0()
            .bg(theme::background())
            .child(row);
        let on_search = self.on_search;
        div()
            .debug_selector(|| "shell/title-bar".into())
            .relative()
            .flex_none()
            .w_full()
            .child(bar)
            .children(span.map(|span| search_box(span, on_search)))
    }
}

#[cfg(test)]
mod tests {
    use super::{SEARCH_MIN_WIDTH, SEARCH_WIDTH, search_span};

    #[test]
    fn search_stays_centered_while_it_fits() {
        let span = search_span(1280., 200., 1080.).expect("visible");
        assert_eq!(span.width, SEARCH_WIDTH);
        assert_eq!(span.left + span.width / 2., 640.);
    }

    #[test]
    fn search_shrinks_centered_before_it_shifts() {
        let span = search_span(800., 200., 580.).expect("visible");
        assert_eq!(span.left + span.width / 2., 400.);
        assert_eq!(span.width, 344.);
        let shifted = search_span(800., 200., 500.).expect("visible");
        assert_eq!(shifted.width, SEARCH_MIN_WIDTH);
        assert_eq!(shifted.left + shifted.width, 492.);
    }

    #[test]
    fn search_takes_the_room_left_and_hides_without_any() {
        let span = search_span(640., 200., 400.).expect("visible");
        assert_eq!((span.left, span.width), (208., 184.));
        assert_eq!(search_span(640., 300., 340.), None);
    }
}
