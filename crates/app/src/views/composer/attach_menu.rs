use std::time::Duration;

use chrono::{DateTime, Utc};
use gpui_kit::component::fluent::{
    MENU_ICON_LABEL_GAP, MENU_ITEM_GAP, MENU_ITEM_HEIGHT, MENU_ITEM_PADDING_X, MENU_LIST_PADDING,
};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::PopupTriggerTooltip as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::substrate_search::SearchSession;
use teams_core::{CloudFile, FileKind, FileReference};

use super::cloud_picker::{ATTACH_CLOUD_LABEL, CloudPicker, CloudPickerEvent};
use super::{COMPOSER_BUTTON_SIZE, COMPOSER_ICON_SIZE, Composer};
use crate::runtime;
use crate::theme::{self, ElevatedSurface as _};
use crate::views::attachments::file_badge;
use crate::views::widgets::symbol;

pub const ATTACH_BUTTON_TAG: &str = "composer/actions/attach";
pub const ATTACH_MENU_TAG: &str = "composer/attach-menu";
const BUTTON_LABEL: &str = "Attach file";
const SEARCH_PLACEHOLDER: &str = "Search cloud files";
const RECENT_TITLE: &str = "Recent cloud files";
const RESULTS_TITLE: &str = "Cloud files";
const UPLOAD_LABEL: &str = "Upload from this device";
const LOADING: &str = "Loading ...";
const NO_RECENT: &str = "Your recent files will show up here.";
const NO_RESULTS: &str = "No files found.";
const FAILED: &str = "Couldn't load files.";
const ATTACH_FAILED: &str = "Couldn't attach";
const MENU_WIDTH: f32 = 316.;
const LIST_MAX_HEIGHT: f32 = 300.;
const ROW_HEIGHT: f32 = 48.;
const BADGE_SIZE: f32 = 24.;
const SEARCH_HEIGHT: f32 = 32.;
const SIDE_INSET: f32 = 12.;
const SEARCH_DEBOUNCE: Duration = Duration::from_millis(300);
const MINUTE: i64 = 60;
const HOUR: i64 = 60 * MINUTE;
const DAY: i64 = 24 * HOUR;

#[derive(Default)]
pub(super) struct AttachMenu {
    pub(super) open: bool,
    recent: Listing,
    found: Option<Listing>,
    search_input: Option<Entity<InputState>>,
    search_session: Option<SearchSession>,
    search_generation: u64,
    search_task: Option<Task<()>>,
    _search_events: Option<Subscription>,
}

impl AttachMenu {
    pub(super) fn input_focused(&self, window: &Window, cx: &App) -> bool {
        self.search_input
            .as_ref()
            .is_some_and(|input| input.read(cx).focus_handle(cx).is_focused(window))
    }
}

#[derive(Default)]
enum Listing {
    #[default]
    Loading,
    Ready(Vec<CloudFile>),
    Failed,
}

pub fn opened_label(opened: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - opened).num_seconds().max(0);
    let count = |value: i64, unit: &str| {
        let plural = if value == 1 { "" } else { "s" };
        format!("Opened {value} {unit}{plural} ago")
    };
    match seconds {
        0..MINUTE => "Opened just now".to_owned(),
        MINUTE..HOUR => count(seconds / MINUTE, "minute"),
        HOUR..DAY => count(seconds / HOUR, "hour"),
        _ => count(seconds / DAY, "day"),
    }
}

fn demo_files() -> Vec<CloudFile> {
    let now = Utc::now();
    [
        ("Release plan Q4.pptx", 2_480_000, 4 * HOUR),
        ("Architecture review.pdf", 860_000, 2 * DAY),
        ("Budget 2027.xlsx", 74_000, 9 * DAY),
    ]
    .into_iter()
    .map(|(name, size, age)| CloudFile {
        name: name.to_owned(),
        size,
        web_url: format!("https://contoso.sharepoint.com/demo/{name}"),
        drive_id: None,
        item_id: None,
        last_opened: Some(now - chrono::Duration::seconds(age)),
    })
    .collect()
}

impl Composer {
    pub(super) fn render_attach_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let open = self.attach_menu.open;
        div()
            .id("composer-attach")
            .debug_selector(|| ATTACH_BUTTON_TAG.into())
            .size(px(COMPOSER_BUTTON_SIZE))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(8.))
            .cursor_pointer()
            .when(open, |button| button.bg(theme::popup_hover()))
            .hover(|button| button.bg(theme::popup_hover()))
            .popup_trigger_tooltip_text(open, BUTTON_LABEL)
            .child(symbol(
                "attach_file",
                COMPOSER_ICON_SIZE,
                if open {
                    theme::accent_text()
                } else {
                    theme::text_muted()
                },
            ))
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_click(cx.listener(|this, _, window, cx| this.toggle_attach_menu(window, cx)))
    }

    fn toggle_attach_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.attach_menu.open {
            self.close_attach_menu(window, cx);
            return;
        }
        self.attach_menu.open = true;
        self.plus_menu = false;
        self.load_recent_cloud_files(cx);
        let input = self.attach_search_input(window, cx);
        input.update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
    }

    pub(super) fn close_attach_menu(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.attach_menu.open {
            return false;
        }
        self.attach_menu.open = false;
        self.attach_menu.found = None;
        self.attach_menu.search_task = None;
        self.attach_menu.search_generation += 1;
        if let Some(input) = &self.attach_menu.search_input {
            input.update(cx, |input, cx| input.set_value("", window, cx));
        }
        self.focus(window, cx);
        cx.notify();
        true
    }

    fn attach_search_input(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some(input) = &self.attach_menu.search_input {
            return input.clone();
        }
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(SEARCH_PLACEHOLDER));
        let subscription =
            cx.subscribe_in(&input, window, |this, input, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change) {
                    let text = input.read(cx).value().trim().to_owned();
                    this.search_cloud_files(text, cx);
                }
            });
        self.attach_menu.search_input = Some(input.clone());
        self.attach_menu._search_events = Some(subscription);
        input
    }

    fn load_recent_cloud_files(&mut self, cx: &mut Context<Self>) {
        let state = self.app.read(cx);
        if state.mode.demo {
            self.attach_menu.recent = Listing::Ready(demo_files());
            return;
        }
        let Some(engine) = state.engine.clone() else {
            self.attach_menu.recent = Listing::Failed;
            return;
        };
        if !matches!(self.attach_menu.recent, Listing::Ready(_)) {
            self.attach_menu.recent = Listing::Loading;
        }
        let receiver = runtime::spawn(async move { engine.recent_cloud_files().await });
        cx.spawn(async move |this, cx| {
            let listing = match receiver.await {
                Ok(Ok(files)) => Listing::Ready(files),
                _ => Listing::Failed,
            };
            this.update(cx, |this, cx| {
                if !matches!(listing, Listing::Failed)
                    || !matches!(this.attach_menu.recent, Listing::Ready(_))
                {
                    this.attach_menu.recent = listing;
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn search_cloud_files(&mut self, text: String, cx: &mut Context<Self>) {
        self.attach_menu.search_generation += 1;
        let generation = self.attach_menu.search_generation;
        if text.is_empty() {
            self.attach_menu.found = None;
            self.attach_menu.search_task = None;
            cx.notify();
            return;
        }
        let state = self.app.read(cx);
        if state.mode.demo {
            let needle = text.to_lowercase();
            let matches = demo_files()
                .into_iter()
                .filter(|file| file.name.to_lowercase().contains(&needle))
                .collect();
            self.attach_menu.found = Some(Listing::Ready(matches));
            cx.notify();
            return;
        }
        let Some(engine) = state.engine.clone() else {
            return;
        };
        let search_session = self
            .attach_menu
            .search_session
            .get_or_insert_with(SearchSession::new)
            .clone();
        self.attach_menu.found = Some(Listing::Loading);
        self.attach_menu.search_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(SEARCH_DEBOUNCE).await;
            let receiver =
                runtime::spawn(
                    async move { engine.search_cloud_files(&search_session, &text).await },
                );
            let listing = match receiver.await {
                Ok(Ok(files)) => Listing::Ready(files),
                _ => Listing::Failed,
            };
            this.update(cx, |this, cx| {
                if this.attach_menu.search_generation == generation {
                    this.attach_menu.found = Some(listing);
                    cx.notify();
                }
            })
            .ok();
        }));
        cx.notify();
    }

    fn attach_cloud_file(&mut self, file: CloudFile, window: &mut Window, cx: &mut Context<Self>) {
        self.close_attach_menu(window, cx);
        let state = self.app.read(cx);
        if state.mode.demo {
            let reference = FileReference {
                attachment_id: uuid::Uuid::new_v4().to_string(),
                content_url: file.web_url.clone(),
                name: file.name.clone(),
            };
            self.tray.add_cloud_file(file.name, file.size, reference);
            self.schedule_draft_save(cx);
            cx.notify();
            return;
        }
        let (Some(engine), Some(conversation_id)) =
            (state.engine.clone(), self.conversation_id.clone())
        else {
            return;
        };
        let task_file = file.clone();
        let receiver =
            runtime::spawn(
                async move { engine.attach_cloud_file(&conversation_id, &task_file).await },
            );
        cx.spawn(async move |this, cx| {
            let attached = receiver.await;
            this.update(cx, |this, cx| {
                match attached {
                    Ok(Ok(reference)) => {
                        this.tray.add_cloud_file(file.name, file.size, reference);
                        this.schedule_draft_save(cx);
                    }
                    _ => this
                        .tray
                        .set_notice(format!("{ATTACH_FAILED} {}.", file.name)),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn open_cloud_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.close_attach_menu(window, cx);
        let app = self.app.clone();
        let picker = cx.new(|cx| CloudPicker::new(app, cx));
        let subscription = cx.subscribe_in(
            &picker,
            window,
            |this, _, event: &CloudPickerEvent, window, cx| {
                let files = match event {
                    CloudPickerEvent::Attach(files) => files.clone(),
                    CloudPickerEvent::Close => Vec::new(),
                };
                this.cloud_picker = None;
                for file in files {
                    this.attach_cloud_file(file, window, cx);
                }
                this.focus(window, cx);
                cx.notify();
            },
        );
        picker.read(cx).focus_handle(cx).focus(window, cx);
        self.cloud_picker = Some((picker, subscription));
        cx.notify();
    }

    pub(super) fn render_attach_menu(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.attach_menu.open {
            return None;
        }
        let search = self.attach_menu.search_input.clone().map(|input| {
            h_flex()
                .mx(px(SIDE_INSET))
                .mt(px(4.))
                .h(px(SEARCH_HEIGHT))
                .px(px(10.))
                .gap(px(8.))
                .items_center()
                .rounded(px(6.))
                .bg(theme::surface_raised())
                .child(symbol("search", 16., theme::text_muted()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(Input::new(&input).appearance(false).bordered(false)),
                )
        });
        let (title, listing) = match &self.attach_menu.found {
            Some(found) => (RESULTS_TITLE, found),
            None => (RECENT_TITLE, &self.attach_menu.recent),
        };
        let now = Utc::now();
        let body = match listing {
            Listing::Loading => vec![self.menu_note(LOADING)],
            Listing::Failed => vec![self.menu_note(FAILED)],
            Listing::Ready(files) if files.is_empty() => {
                vec![self.menu_note(if self.attach_menu.found.is_some() {
                    NO_RESULTS
                } else {
                    NO_RECENT
                })]
            }
            Listing::Ready(files) => files
                .iter()
                .enumerate()
                .map(|(index, file)| self.cloud_file_row(index, file, now, cx).into_any_element())
                .collect(),
        };
        let browse = h_flex()
            .id("composer-attach-cloud")
            .w_full()
            .h(MENU_ITEM_HEIGHT)
            .px(MENU_ITEM_PADDING_X)
            .gap(MENU_ICON_LABEL_GAP)
            .items_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|row| row.bg(theme::popup_hover()))
            .child(symbol("cloud", 16., theme::text_soft()))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::text())
                    .child(ATTACH_CLOUD_LABEL),
            )
            .on_click(cx.listener(|this, _, window, cx| this.open_cloud_picker(window, cx)));
        let upload = h_flex()
            .id("composer-attach-upload")
            .w_full()
            .h(MENU_ITEM_HEIGHT)
            .px(MENU_ITEM_PADDING_X)
            .gap(MENU_ICON_LABEL_GAP)
            .items_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|row| row.bg(theme::popup_hover()))
            .child(symbol("upload", 16., theme::text_soft()))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::text())
                    .child(UPLOAD_LABEL),
            )
            .on_click(cx.listener(|this, _, window, cx| {
                this.close_attach_menu(window, cx);
                this.open_picker(window, cx);
            }));
        Some(
            v_flex()
                .id(ATTACH_MENU_TAG)
                .debug_selector(|| ATTACH_MENU_TAG.into())
                .absolute()
                .bottom(relative(1.))
                .right_0()
                .mb(px(6.))
                .w(px(MENU_WIDTH))
                .max_w(relative(1.))
                .py(MENU_LIST_PADDING)
                .gap(MENU_ITEM_GAP)
                .rounded(px(10.))
                .flyout_surface()
                .occlude()
                .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                    this.close_attach_menu(window, cx);
                }))
                .children(search)
                .child(
                    div()
                        .px(px(SIDE_INSET))
                        .pt(px(6.))
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::text())
                        .child(title),
                )
                .child(
                    v_flex()
                        .id("composer-attach-list")
                        .max_h(px(LIST_MAX_HEIGHT))
                        .px(MENU_LIST_PADDING)
                        .overflow_y_scrollbar()
                        .children(body),
                )
                .child(div().w_full().h(px(1.)).bg(theme::border()))
                .child(div().px(MENU_LIST_PADDING).child(browse).child(upload))
                .into_any_element(),
        )
    }

    fn menu_note(&self, text: &'static str) -> AnyElement {
        div()
            .px(MENU_ITEM_PADDING_X)
            .py(px(10.))
            .text_size(px(12.))
            .text_color(theme::text_soft())
            .child(text)
            .into_any_element()
    }

    fn cloud_file_row(
        &self,
        index: usize,
        file: &CloudFile,
        now: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let picked = file.clone();
        h_flex()
            .id(("composer-attach-file", index))
            .w_full()
            .h(px(ROW_HEIGHT))
            .flex_none()
            .px(MENU_ITEM_PADDING_X)
            .gap(px(10.))
            .items_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|row| row.bg(theme::popup_hover()))
            .child(file_badge(FileKind::from_name(&file.name), BADGE_SIZE))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_size(px(13.))
                            .text_color(theme::text())
                            .truncate()
                            .child(file.name.clone()),
                    )
                    .children(file.last_opened.map(|opened| {
                        div()
                            .text_size(px(12.))
                            .text_color(theme::text_soft())
                            .child(opened_label(opened, now))
                    })),
            )
            .on_click(cx.listener(move |this, _, window, cx| {
                this.attach_cloud_file(picked.clone(), window, cx)
            }))
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone as _;

    use super::opened_label;

    #[test]
    fn opened_reads_like_teams() {
        let now = chrono::Utc.with_ymd_and_hms(2026, 11, 9, 20, 0, 0).unwrap();
        let ago = |seconds: i64| opened_label(now - chrono::Duration::seconds(seconds), now);
        assert_eq!(ago(20), "Opened just now");
        assert_eq!(ago(60), "Opened 1 minute ago");
        assert_eq!(ago(4 * 3600 + 100), "Opened 4 hours ago");
        assert_eq!(ago(6 * 86_400), "Opened 6 days ago");
    }
}
