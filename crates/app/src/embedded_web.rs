use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::*;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::theme;

#[cfg(windows)]
mod native_windows;
#[cfg(windows)]
use native_windows as native;
#[cfg(not(windows))]
mod native_stub;
#[cfg(not(windows))]
use native_stub as native;

pub use native::{Native, NativeEvent, available};

const BAR_HEIGHT: f32 = 28.;
const BAR_TEXT_SIZE: f32 = 11.5;
const OPEN_LABEL: &str = "Open in browser \u{2197}";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Overlays {
    pub panels: bool,
    pub tab_menu: bool,
}

impl Global for Overlays {}

impl Overlays {
    pub fn read(cx: &App) -> Self {
        cx.try_global::<Overlays>().copied().unwrap_or_default()
    }

    pub fn update(cx: &mut App, change: impl FnOnce(&mut Overlays)) {
        let mut next = Overlays::read(cx);
        change(&mut next);
        if next != Overlays::read(cx) {
            cx.set_global(next);
        }
    }

    pub fn any(self) -> bool {
        self.panels || self.tab_menu
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Visibility {
    pub overlays: Overlays,
    pub has_area: bool,
}

pub fn embed_visible(visibility: Visibility) -> bool {
    visibility.has_area && !visibility.overlays.any()
}

pub fn url_bar_text(url: &str) -> String {
    let shown = url.strip_prefix("https://").unwrap_or(url);
    shown.strip_suffix('/').unwrap_or(shown).to_owned()
}

pub enum EmbeddedWebEvent {
    Closed { start_url: String },
}

pub struct EmbeddedWeb {
    tab_id: String,
    start_url: String,
    url: String,
    native: Native,
    _events: Task<()>,
}

impl EventEmitter<EmbeddedWebEvent> for EmbeddedWeb {}

impl EmbeddedWeb {
    pub fn new(
        tab_id: String,
        start_url: String,
        native: Native,
        mut events: UnboundedReceiver<NativeEvent>,
        cx: &mut Context<Self>,
    ) -> Self {
        let task = cx.spawn(async move |this, cx| {
            while let Some(event) = events.recv().await {
                let handled = this.update(cx, |this, cx| this.on_event(event, cx));
                if handled.is_err() {
                    break;
                }
            }
        });
        EmbeddedWeb {
            tab_id,
            url: start_url.clone(),
            start_url,
            native,
            _events: task,
        }
    }

    pub fn tab_id(&self) -> &str {
        &self.tab_id
    }

    fn on_event(&mut self, event: NativeEvent, cx: &mut Context<Self>) {
        match event {
            NativeEvent::Navigated(url) => {
                self.url = url;
                cx.notify();
            }
            NativeEvent::NewWindow(url) => cx.open_url(&url),
            NativeEvent::Closed => cx.emit(EmbeddedWebEvent::Closed {
                start_url: self.start_url.clone(),
            }),
        }
    }
}

impl Drop for EmbeddedWeb {
    fn drop(&mut self) {
        self.native.close();
    }
}

impl Render for EmbeddedWeb {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let native = self.native.clone();
        let open_url = self.url.clone();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(px(BAR_HEIGHT))
                    .flex_none()
                    .w_full()
                    .px(px(12.))
                    .gap(px(8.))
                    .items_center()
                    .border_b_1()
                    .border_color(theme::border())
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .overflow_hidden()
                            .text_ellipsis()
                            .whitespace_nowrap()
                            .text_size(px(BAR_TEXT_SIZE))
                            .text_color(theme::text_muted())
                            .child(url_bar_text(&self.url)),
                    )
                    .child(
                        Button::new("embedded-web-open")
                            .ghost()
                            .compact()
                            .label(OPEN_LABEL)
                            .on_click(move |_, _, cx| cx.open_url(&open_url)),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .bg(theme::background())
                    .child(
                        canvas(
                            move |bounds, window, cx| {
                                let visible = embed_visible(Visibility {
                                    overlays: Overlays::read(cx),
                                    has_area: bounds.size.width > px(0.)
                                        && bounds.size.height > px(0.),
                                });
                                native.place(bounds, window.scale_factor(), visible);
                            },
                            |_, _, _, _| {},
                        )
                        .size_full(),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{Overlays, Visibility, embed_visible, url_bar_text};

    fn visibility(panels: bool, tab_menu: bool, has_area: bool) -> Visibility {
        Visibility {
            overlays: Overlays { panels, tab_menu },
            has_area,
        }
    }

    #[test]
    fn the_page_shows_only_without_overlays_and_with_room() {
        assert!(embed_visible(visibility(false, false, true)));
        assert!(!embed_visible(visibility(true, false, true)));
        assert!(!embed_visible(visibility(false, true, true)));
        assert!(!embed_visible(visibility(false, false, false)));
    }

    #[test]
    fn the_url_bar_drops_the_scheme_and_a_trailing_slash() {
        assert_eq!(url_bar_text("https://docs.example/"), "docs.example");
        assert_eq!(
            url_bar_text("https://docs.example/a/b?c=1"),
            "docs.example/a/b?c=1"
        );
        assert_eq!(url_bar_text("http://plain.example"), "http://plain.example");
    }
}
