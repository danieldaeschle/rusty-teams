use std::rc::Rc;

use chrono::{Local, Offset as _};
use gpui_kit::assets::IconName;
use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex,
    input::{Input, InputState},
    menu::{ContextMenuExt as _, PopupMenuItem},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{DriveEntry, FileKind};

use super::attachments::file_badge;
use super::widgets::icon;
use crate::channel_files::{
    LibraryPath, MessageFilter, SharedContent, SharedItem, grouped_items, size_label,
};
use crate::downloads::{DownloadState, percent_label};
use crate::format;
use crate::theme;

const SIDE_PADDING: f32 = 24.;
const ROW_HEIGHT: f32 = 40.;
const BADGE_SIZE: f32 = 26.;
const MODIFIED_WIDTH: f32 = 110.;
const MODIFIED_BY_WIDTH: f32 = 160.;
const SIZE_WIDTH: f32 = 90.;
const SKELETON_ROWS: usize = 6;
const NEW_FOLDER_WIDTH: f32 = 220.;
const MENU_ICON_SIZE: f32 = 14.;

pub const EMPTY_LIBRARY: &str = "No files yet. Upload or drop files here.";
pub const EMPTY_MESSAGES: &str = "No files or links shared in this channel yet.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedScope {
    Library,
    Messages,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SharedCommand {
    Scope(SharedScope),
    Filter(MessageFilter),
    Crumb(usize),
    Open(usize),
    OpenInBrowser(usize),
    Download(usize),
    CopyLink(usize),
    Retry,
    ToggleNewFolder,
    Upload,
    OpenSharePoint,
    OpenItem(usize),
    JumpToPost(usize),
}

pub type SharedRun = Rc<dyn Fn(SharedCommand, &mut Window, &mut App)>;

pub enum Listing {
    Loading,
    Failed(String),
    Ready(Vec<DriveEntry>),
}

pub struct LibraryView<'a> {
    pub path: Option<&'a LibraryPath>,
    pub listing: &'a Listing,
    pub new_folder: Option<&'a Entity<InputState>>,
    pub status: Option<&'a str>,
    pub download_states: Vec<Option<DownloadState>>,
}

pub struct MessagesView<'a> {
    pub items: &'a [SharedItem],
    pub filter: MessageFilter,
    pub download_states: Vec<Option<DownloadState>>,
}

pub fn download_label(state: Option<&DownloadState>) -> Option<String> {
    match state? {
        DownloadState::Saving(percent) => Some(percent_label(*percent)),
        DownloadState::Saved(_) => Some("Saved".to_owned()),
        DownloadState::Failed(_) => Some("Failed, click to retry".to_owned()),
    }
}

fn segment(
    id: &'static str,
    label: &'static str,
    active: bool,
    command: SharedCommand,
    run: &SharedRun,
) -> Stateful<Div> {
    let run = run.clone();
    div()
        .id(id)
        .px(px(14.))
        .py(px(5.))
        .rounded(px(6.))
        .cursor_pointer()
        .text_size(px(12.5))
        .font_weight(if active {
            FontWeight::SEMIBOLD
        } else {
            FontWeight::NORMAL
        })
        .text_color(if active {
            theme::text()
        } else {
            theme::text_muted()
        })
        .when(active, |segment| segment.bg(theme::border_strong()))
        .on_click(move |_, window, cx| run(command, window, cx))
        .child(label)
}

fn switch(scope: SharedScope, run: &SharedRun) -> Div {
    h_flex()
        .p(px(2.))
        .gap(px(2.))
        .rounded(px(8.))
        .bg(theme::surface_raised())
        .child(segment(
            "shared-in-library",
            "In library",
            scope == SharedScope::Library,
            SharedCommand::Scope(SharedScope::Library),
            run,
        ))
        .child(segment(
            "shared-in-messages",
            "In messages",
            scope == SharedScope::Messages,
            SharedCommand::Scope(SharedScope::Messages),
            run,
        ))
}

fn tool_button(
    id: &'static str,
    label: &'static str,
    glyph: IconName,
    command: SharedCommand,
    run: &SharedRun,
) -> impl IntoElement {
    let run = run.clone();
    Button::new(id)
        .ghost()
        .compact()
        .icon(glyph)
        .label(label)
        .on_click(move |_, window, cx| run(command, window, cx))
}

fn header(scope: SharedScope, trailing: Option<AnyElement>, run: &SharedRun) -> Div {
    h_flex()
        .w_full()
        .flex_none()
        .px(px(SIDE_PADDING))
        .py(px(10.))
        .gap(px(12.))
        .items_center()
        .child(switch(scope, run))
        .children(trailing)
}

fn crumb_trail(path: &LibraryPath, run: &SharedRun) -> Div {
    let last = path.crumbs().len() - 1;
    let mut trail = h_flex().flex_1().min_w_0().gap(px(4.)).items_center();
    for (index, crumb) in path.crumbs().iter().enumerate() {
        if index > 0 {
            trail = trail.child(icon(IconName::ChevronRight, 12., theme::text_faint()));
        }
        let run = run.clone();
        let current = index == last;
        trail = trail.child(
            div()
                .id(ElementId::NamedInteger(
                    "library-crumb".into(),
                    index as u64,
                ))
                .px(px(4.))
                .py(px(2.))
                .rounded(px(4.))
                .truncate()
                .text_size(px(13.))
                .font_weight(if current {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if current {
                    theme::text()
                } else {
                    theme::text_muted()
                })
                .when(!current, |crumb| {
                    crumb
                        .cursor_pointer()
                        .hover(|crumb| crumb.bg(theme::row_hover()))
                        .on_click(move |_, window, cx| run(SharedCommand::Crumb(index), window, cx))
                })
                .child(crumb.name.clone()),
        );
    }
    trail
}

fn column(width: Option<f32>) -> Div {
    match width {
        Some(width) => div().w(px(width)).flex_none().truncate(),
        None => div().flex_1().min_w_0().truncate(),
    }
}

fn column_header() -> Div {
    h_flex()
        .w_full()
        .flex_none()
        .h(px(28.))
        .px(px(SIDE_PADDING))
        .gap(px(12.))
        .items_center()
        .border_b_1()
        .border_color(theme::border())
        .text_size(px(11.5))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::text_faint())
        .child(column(None).pl(px(BADGE_SIZE + 10.)).child("Name"))
        .child(column(Some(MODIFIED_WIDTH)).child("Modified"))
        .child(column(Some(MODIFIED_BY_WIDTH)).child("Modified by"))
        .child(column(Some(SIZE_WIDTH)).child("Size"))
}

fn entry_badge(entry: &DriveEntry) -> Div {
    if entry.is_folder() {
        return div()
            .size(px(BADGE_SIZE))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .child(icon(IconName::Folder, 22., theme::accent_text()));
    }
    file_badge(FileKind::from_name(&entry.name), BADGE_SIZE)
}

fn modified_text(entry: &DriveEntry) -> String {
    entry.modified_at.map_or_else(String::new, |time| {
        let now = Local::now();
        format::list_time_label(time, now.date_naive(), now.offset().fix())
    })
}

fn menu_icon(glyph: IconName) -> gpui_kit::component::Icon {
    icon(glyph, MENU_ICON_SIZE, theme::text_muted())
}

fn entry_row(
    index: usize,
    entry: &DriveEntry,
    download: Option<String>,
    run: &SharedRun,
) -> impl IntoElement {
    let folder = entry.is_folder();
    let (open_run, menu_run) = (run.clone(), run.clone());
    let name = h_flex()
        .flex_1()
        .min_w_0()
        .gap(px(10.))
        .items_center()
        .child(entry_badge(entry))
        .child(
            div()
                .truncate()
                .text_size(px(13.))
                .text_color(theme::text())
                .child(entry.name.clone()),
        )
        .children(download.map(|label| {
            div()
                .flex_none()
                .text_size(px(11.5))
                .text_color(theme::accent_text())
                .child(label)
        }));
    h_flex()
        .id(ElementId::NamedInteger("library-row".into(), index as u64))
        .w_full()
        .flex_none()
        .h(px(ROW_HEIGHT))
        .px(px(SIDE_PADDING))
        .gap(px(12.))
        .items_center()
        .cursor_pointer()
        .text_size(px(12.5))
        .text_color(theme::text_muted())
        .hover(|row| row.bg(theme::row_hover()))
        .on_click(move |_, window, cx| open_run(SharedCommand::Open(index), window, cx))
        .child(name)
        .child(column(Some(MODIFIED_WIDTH)).child(modified_text(entry)))
        .child(column(Some(MODIFIED_BY_WIDTH)).child(entry.modified_by.clone().unwrap_or_default()))
        .child(column(Some(SIZE_WIDTH)).child(size_label(entry)))
        .context_menu(move |popup, _, _| {
            let item = |label: &'static str, glyph: IconName, command: SharedCommand| {
                let run = menu_run.clone();
                PopupMenuItem::new(label)
                    .icon(menu_icon(glyph))
                    .on_click(move |_, window, cx| run(command, window, cx))
            };
            let popup = popup.item(item(
                "Open in browser",
                IconName::ExternalLink,
                SharedCommand::OpenInBrowser(index),
            ));
            let popup = if folder {
                popup
            } else {
                popup.item(item(
                    "Download",
                    IconName::Download,
                    SharedCommand::Download(index),
                ))
            };
            popup.item(item(
                "Copy link",
                IconName::Link,
                SharedCommand::CopyLink(index),
            ))
        })
}

fn skeleton_rows() -> impl IntoElement {
    v_flex().w_full().children((0..SKELETON_ROWS).map(|index| {
        let bar = |width: f32| {
            div()
                .h(px(10.))
                .w(px(width))
                .rounded(px(5.))
                .bg(theme::surface_raised())
        };
        h_flex()
            .id(ElementId::NamedInteger(
                "library-skeleton".into(),
                index as u64,
            ))
            .w_full()
            .h(px(ROW_HEIGHT))
            .px(px(SIDE_PADDING))
            .gap(px(12.))
            .items_center()
            .child(
                h_flex()
                    .flex_1()
                    .gap(px(10.))
                    .items_center()
                    .child(
                        div()
                            .size(px(BADGE_SIZE))
                            .rounded(px(7.))
                            .bg(theme::surface_raised()),
                    )
                    .child(bar(140. + 30. * (index % 3) as f32)),
            )
            .child(column(Some(MODIFIED_WIDTH)).child(bar(50.)))
            .child(column(Some(MODIFIED_BY_WIDTH)).child(bar(90.)))
            .child(column(Some(SIZE_WIDTH)).child(bar(40.)))
    }))
}

fn centered() -> Div {
    div()
        .w_full()
        .flex_1()
        .flex()
        .items_center()
        .justify_center()
        .text_size(px(13.))
        .text_color(theme::text_muted())
}

fn centered_note(text: &'static str) -> Div {
    centered().child(text)
}

fn new_folder_row(input: &Entity<InputState>) -> Div {
    h_flex()
        .w_full()
        .flex_none()
        .px(px(SIDE_PADDING))
        .pb(px(8.))
        .gap(px(8.))
        .items_center()
        .child(icon(IconName::Folder, 18., theme::accent_text()))
        .child(
            div()
                .w(px(NEW_FOLDER_WIDTH))
                .h(px(30.))
                .px(px(2.))
                .flex()
                .items_center()
                .rounded(px(8.))
                .bg(theme::surface())
                .border_1()
                .border_color(theme::accent())
                .child(Input::new(input).appearance(false).bordered(false)),
        )
        .child(
            div()
                .text_size(px(11.5))
                .text_color(theme::text_faint())
                .child("Enter to create, Esc to cancel"),
        )
}

pub fn render_library(view: LibraryView, run: &SharedRun) -> AnyElement {
    let toolbar = view.path.map(|path| {
        h_flex()
            .flex_1()
            .min_w_0()
            .gap(px(4.))
            .items_center()
            .child(crumb_trail(path, run))
            .child(tool_button(
                "library-new-folder",
                "New folder",
                IconName::Plus,
                SharedCommand::ToggleNewFolder,
                run,
            ))
            .child(tool_button(
                "library-upload",
                "Upload",
                IconName::Upload,
                SharedCommand::Upload,
                run,
            ))
            .child(tool_button(
                "library-open-sharepoint",
                "Open in SharePoint \u{2197}",
                IconName::ExternalLink,
                SharedCommand::OpenSharePoint,
                run,
            ))
            .into_any_element()
    });
    let body = match view.listing {
        Listing::Loading => skeleton_rows().into_any_element(),
        Listing::Failed(reason) => {
            let retry = run.clone();
            centered()
                .flex_col()
                .gap(px(8.))
                .child(
                    div()
                        .text_color(theme::red_soft())
                        .child(format!("Could not load the files: {reason}")),
                )
                .child(
                    Button::new("library-retry")
                        .ghost()
                        .compact()
                        .label("Retry")
                        .on_click(move |_, window, cx| retry(SharedCommand::Retry, window, cx)),
                )
                .into_any_element()
        }
        Listing::Ready(entries) if entries.is_empty() => {
            centered_note(EMPTY_LIBRARY).into_any_element()
        }
        Listing::Ready(entries) => v_flex()
            .id("library-rows")
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(entries.iter().enumerate().map(|(index, entry)| {
                let download = view
                    .download_states
                    .get(index)
                    .and_then(|state| download_label(state.as_ref()));
                entry_row(index, entry, download, run)
            }))
            .into_any_element(),
    };
    v_flex()
        .size_full()
        .child(header(SharedScope::Library, toolbar, run))
        .children(view.new_folder.map(new_folder_row))
        .children(view.status.map(|status| {
            div()
                .px(px(SIDE_PADDING))
                .pb(px(6.))
                .text_size(px(12.))
                .text_color(theme::accent_text())
                .child(status.to_owned())
        }))
        .child(column_header())
        .child(body)
        .into_any_element()
}

fn chip(
    id: &'static str,
    label: &'static str,
    active: bool,
    command: SharedCommand,
    run: &SharedRun,
) -> Stateful<Div> {
    let run = run.clone();
    div()
        .id(id)
        .px(px(12.))
        .py(px(4.))
        .rounded_full()
        .cursor_pointer()
        .text_size(px(12.))
        .border_1()
        .border_color(if active {
            theme::accent()
        } else {
            theme::border_strong()
        })
        .text_color(if active {
            theme::accent_text()
        } else {
            theme::text_muted()
        })
        .when(active, |chip| chip.bg(theme::accent_soft().opacity(0.12)))
        .on_click(move |_, window, cx| run(command, window, cx))
        .child(label)
}

fn chips(filter: MessageFilter, run: &SharedRun) -> AnyElement {
    h_flex()
        .gap(px(8.))
        .child(chip(
            "shared-filter-all",
            "All",
            filter == MessageFilter::All,
            SharedCommand::Filter(MessageFilter::All),
            run,
        ))
        .child(chip(
            "shared-filter-files",
            "Files",
            filter == MessageFilter::Files,
            SharedCommand::Filter(MessageFilter::Files),
            run,
        ))
        .child(chip(
            "shared-filter-links",
            "Links",
            filter == MessageFilter::Links,
            SharedCommand::Filter(MessageFilter::Links),
            run,
        ))
        .into_any_element()
}

fn item_badge(item: &SharedItem) -> Div {
    match &item.content {
        SharedContent::File(card) => file_badge(card.kind, BADGE_SIZE),
        SharedContent::Link(_) => div()
            .size(px(BADGE_SIZE))
            .flex_none()
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(7.))
            .bg(theme::surface_raised())
            .child(icon(IconName::Link, 14., theme::text_muted())),
    }
}

fn item_row(
    index: usize,
    item: &SharedItem,
    download: Option<String>,
    run: &SharedRun,
) -> Stateful<Div> {
    let (open_run, post_run) = (run.clone(), run.clone());
    let post =
        (!item.post_label.is_empty()).then(|| format!("\u{201c}{}\u{201d}", item.post_label));
    let source = h_flex()
        .gap(px(4.))
        .text_size(px(12.))
        .text_color(theme::text_muted())
        .child(format!("{} in", item.author))
        .children(post.map(|label| {
            div()
                .id(ElementId::NamedInteger("shared-post".into(), index as u64))
                .truncate()
                .cursor_pointer()
                .hover(|post| post.text_color(theme::accent_text()))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    post_run(SharedCommand::JumpToPost(index), window, cx)
                })
                .child(label)
        }));
    h_flex()
        .id(ElementId::NamedInteger("shared-item".into(), index as u64))
        .w_full()
        .flex_none()
        .px(px(SIDE_PADDING))
        .py(px(6.))
        .gap(px(10.))
        .items_center()
        .cursor_pointer()
        .hover(|row| row.bg(theme::row_hover()))
        .on_click(move |_, window, cx| open_run(SharedCommand::OpenItem(index), window, cx))
        .child(item_badge(item))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(px(13.))
                        .text_color(theme::text())
                        .child(item.name().to_owned()),
                )
                .child(source),
        )
        .children(download.map(|label| {
            div()
                .flex_none()
                .text_size(px(11.5))
                .text_color(theme::accent_text())
                .child(label)
        }))
}

pub fn render_messages(view: MessagesView, run: &SharedRun) -> AnyElement {
    let now = Local::now();
    let groups = grouped_items(
        view.items,
        view.filter,
        now.date_naive(),
        now.offset().fix(),
    );
    let body = if groups.is_empty() {
        centered_note(EMPTY_MESSAGES).into_any_element()
    } else {
        v_flex()
            .id("shared-messages")
            .w_full()
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(groups.into_iter().map(|(group, indices)| {
                v_flex()
                    .w_full()
                    .child(
                        div()
                            .px(px(SIDE_PADDING))
                            .pt(px(10.))
                            .pb(px(4.))
                            .text_size(px(11.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text_faint())
                            .child(group.label()),
                    )
                    .children(indices.into_iter().map(|index| {
                        let download = view
                            .download_states
                            .get(index)
                            .and_then(|state| download_label(state.as_ref()));
                        item_row(index, &view.items[index], download, run)
                    }))
            }))
            .into_any_element()
    };
    v_flex()
        .size_full()
        .child(header(
            SharedScope::Messages,
            Some(chips(view.filter, run)),
            run,
        ))
        .child(body)
        .into_any_element()
}
