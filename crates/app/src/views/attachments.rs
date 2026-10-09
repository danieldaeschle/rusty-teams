use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{FileCard, FileKind, ImageRef, external_image_url};

use super::widgets::{icon, symbol};
use crate::data::Directory;
use crate::downloads::{self, DownloadState};
use crate::format;
use crate::remote_image;
use crate::render::{Block, render_blocks};
use crate::rows::LocalImage;
use crate::theme;

pub const IMAGE_MAX_WIDTH: f32 = 360.;
const IMAGE_MAX_HEIGHT: f32 = 300.;
const IMAGE_MIN_SIDE: f32 = 48.;
const IMAGE_FALLBACK: (f32, f32) = (240., 160.);
const IMAGE_RADIUS: f32 = 10.;
const FILE_CARD_WIDTH: f32 = 300.;
const FILE_BADGE_SIZE: f32 = 34.;
const SAVE_BUTTON_SIZE: f32 = 28.;
const SAVE_BUTTON_RADIUS: f32 = 6.;
const SAVE_PROGRESS_HEIGHT: f32 = 2.;
const LOCAL_IMAGE_PREFIX: &str = "../hostedContents/";
const LOCAL_IMAGE_SUFFIX: &str = "/$value";

pub type FileActivate = Rc<dyn Fn(usize, &mut App)>;

#[derive(Clone)]
pub struct FileActions {
    pub states: Vec<Option<DownloadState>>,
    pub activate: FileActivate,
}

pub fn fit_image(size: Option<(u32, u32)>) -> (f32, f32) {
    fit_image_within(size, IMAGE_MAX_HEIGHT)
}

fn fit_image_within(size: Option<(u32, u32)>, max_height: f32) -> (f32, f32) {
    let Some((width, height)) = size.filter(|(width, height)| *width > 0 && *height > 0) else {
        return IMAGE_FALLBACK;
    };
    let (width, height) = (width as f32, height as f32);
    let scale = (IMAGE_MAX_WIDTH / width)
        .min(max_height / height)
        .min(1.);
    (
        (width * scale).max(IMAGE_MIN_SIDE).round(),
        (height * scale).max(IMAGE_MIN_SIDE).round(),
    )
}

fn declared_size(image: &ImageRef) -> Option<(u32, u32)> {
    image.width.zip(image.height)
}

pub fn image_size(image: &ImageRef, directory: &Directory) -> (f32, f32) {
    let loaded = directory.image(&image.url).and_then(|entry| entry.size);
    let max_height = if external_image_url(&image.url).is_some() {
        remote_image::MAX_HEIGHT as f32
    } else {
        IMAGE_MAX_HEIGHT
    };
    fit_image_within(declared_size(image).or(loaded), max_height)
}

fn image_view(image: &ImageRef, id: String, directory: &Directory) -> AnyElement {
    let (width, height) = image_size(image, directory);
    let frame = div()
        .id(ElementId::Name(id.into()))
        .relative()
        .w(px(width))
        .h(px(height))
        .max_w(relative(1.))
        .flex_none()
        .rounded(px(IMAGE_RADIUS))
        .overflow_hidden();
    let Some(entry) = directory.image(&image.url) else {
        return frame
            .flex()
            .flex_col()
            .gap(px(6.))
            .items_center()
            .justify_center()
            .bg(theme::background())
            .border_1()
            .border_color(theme::border_strong())
            .child(icon(IconName::Image, 24., theme::text_muted()))
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::text_muted())
                    .child("Loading image"),
            )
            .into_any_element();
    };
    let path = entry.path.clone();
    frame
        .cursor_pointer()
        .border_1()
        .border_color(theme::border())
        .hover(|frame| frame.border_color(theme::accent()))
        .child(
            img(entry.path.clone())
                .size_full()
                .rounded(px(IMAGE_RADIUS))
                .object_fit(ObjectFit::Cover),
        )
        .on_click(move |_, _, cx| cx.open_with_system(&path))
        .into_any_element()
}

struct KindStyle {
    label: &'static str,
    description: &'static str,
    color: u32,
}

fn kind_style(kind: FileKind) -> KindStyle {
    let (label, description, color) = match kind {
        FileKind::Word => ("DOC", "Word document", 0x2563EB),
        FileKind::Excel => ("XLS", "Excel spreadsheet", 0x15803D),
        FileKind::PowerPoint => ("PPT", "PowerPoint", 0xC2410C),
        FileKind::Pdf => ("PDF", "PDF document", 0xB91C1C),
        FileKind::Image => ("IMG", "Image", 0x7C3AED),
        FileKind::Archive => ("ZIP", "Archive", 0xB45309),
        FileKind::Other => ("", "File", 0x525252),
    };
    KindStyle {
        label,
        description,
        color,
    }
}

pub fn file_subtitle(card: &FileCard) -> String {
    let description = kind_style(card.kind).description;
    match card.size {
        Some(size) => format!("{description} \u{b7} {}", format::file_size_label(size)),
        None => description.to_owned(),
    }
}

pub fn file_badge(kind: FileKind, size: f32) -> Div {
    let style = kind_style(kind);
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(7.))
        .bg(theme::color(style.color))
        .text_color(gpui_kit::white())
        .text_size(px(10.5))
        .font_weight(FontWeight::BOLD)
        .when(style.label.is_empty(), |badge| {
            badge.child(icon(IconName::File, 16., gpui_kit::white()))
        })
        .when(!style.label.is_empty(), |badge| badge.child(style.label))
}

fn local_image_view(image: &LocalImage, id: String) -> AnyElement {
    let (width, height) = fit_image(image.size);
    div()
        .id(ElementId::Name(id.into()))
        .w(px(width))
        .h(px(height))
        .max_w(relative(1.))
        .flex_none()
        .rounded(px(IMAGE_RADIUS))
        .overflow_hidden()
        .border_1()
        .border_color(theme::border())
        .child(
            img(image.image.clone())
                .size_full()
                .rounded(px(IMAGE_RADIUS))
                .object_fit(ObjectFit::Cover),
        )
        .into_any_element()
}

fn save_button(
    state: Option<&DownloadState>,
    id: String,
    index: usize,
    activate: FileActivate,
) -> Stateful<Div> {
    let look = downloads::button_look(state);
    let tooltip = look.tooltip.clone();
    let clickable = look.clickable;
    let button = div()
        .id(ElementId::Name(id.into()))
        .size(px(SAVE_BUTTON_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(SAVE_BUTTON_RADIUS))
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .when(look.clickable, |button| {
            button
                .cursor_pointer()
                .hover(|button| button.bg(theme::row_hover()))
        })
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            if clickable {
                activate(index, cx);
            }
        });
    match look.percent {
        Some(percent) => button.child(
            div()
                .text_size(px(11.))
                .text_color(theme::text_muted())
                .child(downloads::percent_label(percent)),
        ),
        None => button.child(symbol(look.symbol, 16., theme::text_muted())),
    }
}

fn file_view(
    card: &FileCard,
    id: String,
    index: usize,
    actions: Option<&FileActions>,
) -> AnyElement {
    let badge = file_badge(card.kind, FILE_BADGE_SIZE);
    let url = card.open_url.clone();
    let state = actions
        .and_then(|actions| actions.states.get(index))
        .and_then(Option::as_ref);
    let progress = downloads::progress_fraction(state);
    let button = actions
        .filter(|_| !card.open_url.is_empty())
        .map(|actions| save_button(state, format!("{id}-save"), index, actions.activate.clone()));
    h_flex()
        .id(ElementId::Name(id.into()))
        .relative()
        .overflow_hidden()
        .w(px(FILE_CARD_WIDTH))
        .max_w(relative(1.))
        .gap(px(10.))
        .px(px(8.))
        .py(px(7.))
        .items_center()
        .rounded(px(9.))
        .bg(theme::surface())
        .border_1()
        .border_color(theme::border_strong())
        .cursor_pointer()
        .hover(|card| card.bg(theme::row_hover()).border_color(theme::accent()))
        .child(badge)
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(px(13.))
                        .text_color(theme::text())
                        .child(card.name.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(theme::text_muted())
                        .child(file_subtitle(card)),
                ),
        )
        .child(icon(IconName::ExternalLink, 14., theme::text_muted()))
        .children(button)
        .when_some(progress, |card, fraction| {
            card.child(
                div()
                    .absolute()
                    .left_0()
                    .bottom_0()
                    .h(px(SAVE_PROGRESS_HEIGHT))
                    .w(relative(fraction))
                    .bg(theme::accent()),
            )
        })
        .on_click(move |_, _, cx| cx.open_url(&url))
        .into_any_element()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Placement {
    Remote(usize),
    Local(usize),
}

fn local_image_index(url: &str) -> Option<usize> {
    url.strip_prefix(LOCAL_IMAGE_PREFIX)?
        .strip_suffix(LOCAL_IMAGE_SUFFIX)?
        .parse::<usize>()
        .ok()?
        .checked_sub(1)
}

fn placement(url: &str, images: &[ImageRef], local_count: usize) -> Option<Placement> {
    if let Some(index) = images.iter().position(|image| image.url == url) {
        return Some(Placement::Remote(index));
    }
    local_image_index(url)
        .filter(|index| *index < local_count)
        .map(Placement::Local)
}

fn placements(blocks: &[Block], images: &[ImageRef], local_count: usize) -> Vec<Placement> {
    blocks
        .iter()
        .filter_map(|block| match block {
            Block::Image { url } => placement(url, images, local_count),
            _ => None,
        })
        .collect()
}

pub fn message_body(
    blocks: &[Block],
    images: &[ImageRef],
    local_images: &[LocalImage],
    id: &str,
    own: bool,
    directory: &Directory,
    cx: &App,
) -> Vec<AnyElement> {
    if !blocks
        .iter()
        .any(|block| matches!(block, Block::Image { .. }))
    {
        return vec![render_blocks(blocks, id, own, cx)];
    }
    let mut elements = Vec::new();
    let mut run_start = 0;
    let render_run = |start: usize, end: usize, elements: &mut Vec<AnyElement>| {
        if end > start {
            let part_id = format!("{id}-part-{start}");
            elements.push(render_blocks(&blocks[start..end], &part_id, own, cx));
        }
    };
    for (index, block) in blocks.iter().enumerate() {
        let Block::Image { url } = block else {
            continue;
        };
        render_run(run_start, index, &mut elements);
        run_start = index + 1;
        match placement(url, images, local_images.len()) {
            Some(Placement::Remote(position)) => elements.push(image_view(
                &images[position],
                format!("{id}-image-{position}"),
                directory,
            )),
            Some(Placement::Local(position)) => elements.push(local_image_view(
                &local_images[position],
                format!("{id}-local-image-{position}"),
            )),
            None => {}
        }
    }
    render_run(run_start, blocks.len(), &mut elements);
    elements
}

pub fn attachments_view(
    blocks: &[Block],
    images: &[ImageRef],
    local_images: &[LocalImage],
    files: &[FileCard],
    id: &str,
    directory: &Directory,
    file_actions: Option<&FileActions>,
) -> Option<Div> {
    let placed = placements(blocks, images, local_images.len());
    let unplaced_images: Vec<(usize, &ImageRef)> = images
        .iter()
        .enumerate()
        .filter(|(index, _)| !placed.contains(&Placement::Remote(*index)))
        .collect();
    let unplaced_local: Vec<(usize, &LocalImage)> = local_images
        .iter()
        .enumerate()
        .filter(|(index, _)| !placed.contains(&Placement::Local(*index)))
        .collect();
    if unplaced_images.is_empty() && unplaced_local.is_empty() && files.is_empty() {
        return None;
    }
    Some(
        v_flex()
            .gap(px(6.))
            .items_start()
            .children(
                unplaced_images.into_iter().map(|(index, image)| {
                    image_view(image, format!("{id}-image-{index}"), directory)
                }),
            )
            .children(
                unplaced_local.into_iter().map(|(index, image)| {
                    local_image_view(image, format!("{id}-local-image-{index}"))
                }),
            )
            .children(files.iter().enumerate().map(|(index, card)| {
                file_view(card, format!("{id}-file-{index}"), index, file_actions)
            })),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        IMAGE_FALLBACK, Placement, file_subtitle, fit_image, fit_image_within, local_image_index,
        placement, placements,
    };
    use crate::render::Block;
    use teams_core::{FileCard, FileKind, ImageRef};

    fn image_ref(url: &str) -> ImageRef {
        ImageRef {
            id: url.into(),
            url: url.into(),
            width: None,
            height: None,
        }
    }

    fn image_block(url: &str) -> Block {
        Block::Image { url: url.into() }
    }

    #[test]
    fn hosted_content_urls_map_to_zero_based_local_indices() {
        assert_eq!(local_image_index("../hostedContents/2/$value"), Some(1));
        assert_eq!(local_image_index("../hostedContents/0/$value"), None);
        assert_eq!(local_image_index("https://x/y"), None);
    }

    #[test]
    fn placed_images_are_found_by_url_before_local_indices() {
        let images = [image_ref("a"), image_ref("b")];
        let blocks = [
            image_block("b"),
            image_block("../hostedContents/2/$value"),
            image_block("../hostedContents/9/$value"),
            image_block("missing"),
        ];
        assert_eq!(
            placements(&blocks, &images, 2),
            vec![Placement::Remote(1), Placement::Local(1)]
        );
        assert_eq!(placement("a", &images, 0), Some(Placement::Remote(0)));
    }

    #[test]
    fn wide_images_shrink_to_the_max_width_keeping_aspect() {
        assert_eq!(fit_image(Some((720, 360))), (360., 180.));
    }

    #[test]
    fn gifs_and_stickers_are_capped_at_250_high() {
        assert_eq!(fit_image_within(Some((300, 500)), 250.), (150., 250.));
    }

    #[test]
    fn tall_images_shrink_to_the_max_height() {
        assert_eq!(fit_image(Some((300, 600))), (150., 300.));
    }

    #[test]
    fn small_images_keep_their_size() {
        assert_eq!(fit_image(Some((200, 100))), (200., 100.));
    }

    #[test]
    fn unknown_or_empty_sizes_use_the_fallback() {
        assert_eq!(fit_image(None), IMAGE_FALLBACK);
        assert_eq!(fit_image(Some((0, 10))), IMAGE_FALLBACK);
    }

    #[test]
    fn thin_images_keep_a_clickable_side() {
        assert_eq!(fit_image(Some((3000, 10))), (360., 48.));
    }

    #[test]
    fn subtitle_adds_the_size_when_known() {
        let card = FileCard {
            name: "a.pdf".into(),
            kind: FileKind::Pdf,
            content_type: None,
            size: Some(2048),
            open_url: String::new(),
        };
        assert_eq!(file_subtitle(&card), "PDF document \u{b7} 2 KB");
        let unknown = FileCard { size: None, ..card };
        assert_eq!(file_subtitle(&unknown), "PDF document");
    }
}
