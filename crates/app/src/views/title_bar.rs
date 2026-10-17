use std::sync::{Arc, OnceLock};

use gpui_kit::assets::IconName;
use gpui_kit::component::{TitleBar, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::widgets::icon;
use crate::app_state::Mode;
use crate::appearance::AppIcon;
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

pub fn app_icon_image(app_icon: AppIcon) -> Arc<Image> {
    static IMAGES: [OnceLock<Arc<Image>>; AppIcon::ALL.len()] =
        [const { OnceLock::new() }; AppIcon::ALL.len()];
    let index = AppIcon::ALL
        .iter()
        .position(|candidate| *candidate == app_icon)
        .unwrap_or(0);
    IMAGES[index]
        .get_or_init(|| Arc::new(Image::from_bytes(ImageFormat::Png, app_icon.png().to_vec())))
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
    app_icon: AppIcon,
    mode_label: Option<&'static str>,
    controls: Vec<AnyElement>,
    on_search: Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>,
}

pub fn render_title_bar(
    app_icon: AppIcon,
    mode_label: Option<&'static str>,
    controls: impl IntoIterator<Item = AnyElement>,
    on_search: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    TitleBarRow {
        app_icon,
        mode_label,
        controls: controls.into_iter().collect(),
        on_search: Box::new(on_search),
    }
}

// The Windows caption hit-test takes every point the Drag area reaches, so a control without occlusion never gets its click.
fn outside_caption(control: AnyElement) -> Div {
    div().flex_none().occlude().child(control)
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
            .child(
                img(app_icon_image(self.app_icon))
                    .size(px(APP_ICON_SIZE))
                    .flex_none(),
            )
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
            .children(self.controls.into_iter().map(outside_caption))
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
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use gpui_kit::component::button::{Button, ButtonVariants as _};
    use gpui_kit::{
        Context, Hitbox, HitboxBehavior, InteractiveElement as _, IntoElement, Modifiers,
        ParentElement, Render, Styled, TestAppContext, Window, canvas, div, px, size,
    };

    use super::{
        AppIcon, SEARCH_MIN_WIDTH, SEARCH_WIDTH, UpdateStatus, render_title_bar, search_span,
        update_button,
    };

    struct CaptionHarness {
        caption: Rc<RefCell<Option<Hitbox>>>,
        restarts: Rc<Cell<usize>>,
    }

    impl Render for CaptionHarness {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let caption = self.caption.clone();
            let restarts = self.restarts.clone();
            let own = div()
                .debug_selector(|| "shell/title-bar/own-status".into())
                .child(Button::new("own").ghost().size(px(32.)).child("O"))
                .into_any_element();
            div()
                .relative()
                .size_full()
                .child(
                    canvas(
                        |bounds, window, _| window.insert_hitbox(bounds, HitboxBehavior::Normal),
                        move |_, hitbox, _, _| *caption.borrow_mut() = Some(hitbox),
                    )
                    .absolute()
                    .size_full(),
                )
                .child(render_title_bar(
                    AppIcon::ALL[0],
                    Some("Demo"),
                    [
                        update_button(&UpdateStatus::Ready, move |_, _, _| {
                            restarts.set(restarts.get() + 1)
                        }),
                        Some(own),
                    ]
                    .into_iter()
                    .flatten(),
                    |_, _, _| {},
                ))
        }
    }

    #[gpui_kit::test]
    fn title_bar_controls_stay_out_of_the_caption(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(crate::theme::apply);
        let caption = Rc::new(RefCell::new(None));
        let restarts = Rc::new(Cell::new(0));
        let (_, window_cx) = cx.add_window_view({
            let caption = caption.clone();
            let restarts = restarts.clone();
            move |_, _| CaptionHarness { caption, restarts }
        });
        window_cx.simulate_resize(size(px(1280.), px(800.)));
        for _ in 0..2 {
            window_cx.update(|window, cx| window.draw(cx).clear(cx));
        }
        let mut found = Vec::new();
        for (selector, caption_expected) in [
            ("shell/title-bar/update", false),
            ("shell/title-bar/own-status", false),
            ("shell/title-bar/search", false),
            ("shell/title-bar/leading", true),
        ] {
            let Some(bounds) = window_cx.debug_bounds(selector) else {
                found.push(format!("{selector} was not rendered"));
                continue;
            };
            let center = bounds.center();
            let in_caption = window_cx.update(|window, _| {
                caption
                    .borrow()
                    .as_ref()
                    .is_some_and(|hitbox| hitbox.is_hovered_at(center, window))
            });
            if in_caption != caption_expected {
                found.push(format!("{selector} in caption: {in_caption}"));
            }
        }
        assert!(found.is_empty(), "{}", found.join("\n"));
        let update = window_cx
            .debug_bounds("shell/title-bar/update")
            .expect("update button rendered");
        window_cx.simulate_click(update.center(), Modifiers::default());
        assert_eq!(restarts.get(), 1);
    }

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
