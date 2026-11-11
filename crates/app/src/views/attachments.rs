use crate::views::image_memory::{bounded, preview};
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{FileCard, FileKind, ImageRef, external_image_url};

use super::image_menu::{ImageShare, with_image_menu};
use super::widgets::{icon, symbol};
use crate::data::Directory;
use crate::downloads::{self, DownloadState};
use crate::format;
use crate::inline_video;
use crate::remote_image;
use crate::render::{Block, render_blocks, render_table, table_images};
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
const FILE_PREVIEW_HEIGHT: f32 = FILE_CARD_WIDTH * 9. / 16.;
const PORTRAIT_PAGE_RATIO: f32 = 1.414;
const PLAY_BUTTON_SIZE: f32 = 40.;
const LOCAL_IMAGE_PREFIX: &str = "../hostedContents/";
const LOCAL_IMAGE_SUFFIX: &str = "/$value";

pub type FileActivate = Rc<dyn Fn(usize, &mut App)>;
#[derive(Clone)]
pub struct ImageActions {
    pub open: Rc<dyn Fn(&str, &mut App)>,
    pub share: Option<ImageShare>,
}

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
    let scale = (IMAGE_MAX_WIDTH / width).min(max_height / height).min(1.);
    (
        (width * scale).max(IMAGE_MIN_SIDE).round(),
        (height * scale).max(IMAGE_MIN_SIDE).round(),
    )
}

fn declared_size(image: &ImageRef) -> Option<(u32, u32)> {
    image.width.zip(image.height)
}

fn image_size(image: &ImageRef, directory: &Directory, sent_preview: Option<&LocalImage>) -> (f32, f32) {
    let loaded = directory
        .image(&image.url)
        .and_then(|entry| entry.size)
        .or_else(|| sent_preview.and_then(|preview| preview.size));
    let max_height = if external_image_url(&image.url).is_some() {
        remote_image::MAX_HEIGHT as f32
    } else {
        IMAGE_MAX_HEIGHT
    };
    fit_image_within(declared_size(image).or(loaded), max_height)
}

fn sent_preview<'a>(images: &[ImageRef], position: usize, sent_previews: &'a [LocalImage]) -> Option<&'a LocalImage> {
    if external_image_url(&images[position].url).is_some() {
        return None;
    }
    let uploaded_before = images[..position]
        .iter()
        .filter(|image| external_image_url(&image.url).is_none())
        .count();
    sent_previews.get(uploaded_before)
}

fn local_image(image: &LocalImage, id: ElementId) -> Stateful<Img> {
    img(bounded(image.image.clone()))
        .id(id)
        .size_full()
        .rounded(px(IMAGE_RADIUS))
        .object_fit(ObjectFit::Cover)
}

fn image_view(
    image: &ImageRef,
    id: String,
    directory: &Directory,
    open_image: Option<&ImageActions>,
    sent_preview: Option<&LocalImage>,
) -> AnyElement {
    let (width, height) = image_size(image, directory, sent_preview);
    let image_id = ElementId::Name(format!("{id}-image").into());
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
        if let Some(sent) = sent_preview {
            return frame.child(local_image(sent, image_id)).into_any_element();
        }
        return frame
            .flex()
            .flex_col()
            .gap(px(6.))
            .items_center()
            .justify_center()
            .bg(theme::background())
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
    let share = open_image.and_then(|actions| actions.share.clone());
    let (url, open_image) = (image.url.clone(), open_image.cloned());
    let frame = frame
        .cursor_pointer()
        .child(
            img(preview(entry.path.clone()))
                .id(image_id.clone())
                .size_full()
                .rounded(px(IMAGE_RADIUS))
                .object_fit(ObjectFit::Cover)
                .when_some(sent_preview.cloned(), |remote, sent| {
                    remote.with_loading(move || local_image(&sent, image_id.clone()).into_any_element())
                }),
        )
        .on_click({
            let path = path.clone();
            move |_, _, cx| match &open_image {
                Some(actions) => (actions.open)(&url, cx),
                None => cx.open_with_system(&path),
            }
        });
    with_image_menu(frame, path, share).into_any_element()
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
        FileKind::Video => ("VID", "Video", 0xBE185D),
        FileKind::Archive => ("ZIP", "Archive", 0xB45309),
        FileKind::Loop => ("", "Loop component", 0x7C3AED),
        FileKind::Other => ("", "File", 0x525252),
    };
    KindStyle {
        label,
        description,
        color,
    }
}

pub fn kind_description(kind: FileKind) -> &'static str {
    kind_style(kind).description
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
    let image_id = ElementId::Name(format!("{id}-image").into());
    div()
        .id(ElementId::Name(id.into()))
        .w(px(width))
        .h(px(height))
        .max_w(relative(1.))
        .flex_none()
        .rounded(px(IMAGE_RADIUS))
        .overflow_hidden()
        .child(local_image(image, image_id))
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

fn file_preview(card: &FileCard, id: &str, directory: &Directory) -> Option<Stateful<Div>> {
    let image = card.preview().and_then(|preview| {
        directory
            .image(&preview.url)
            .map(|entry| entry.path.clone())
    });
    if image.is_none() && card.kind != FileKind::Video {
        return None;
    }
    let frame = div()
        .id(SharedString::from(format!("{id}-preview")))
        .relative()
        .w_full()
        .h(px(FILE_PREVIEW_HEIGHT))
        .overflow_hidden()
        .bg(theme::code_surface());
    let frame = match image {
        Some(path) if card.kind.is_document() && card.kind != FileKind::PowerPoint => {
            frame.bg(theme::white()).child(
                img(bounded(path))
                    .w_full()
                    .h(px(FILE_CARD_WIDTH * PORTRAIT_PAGE_RATIO))
                    .object_fit(ObjectFit::Cover),
            )
        }
        Some(path) => frame.child(img(bounded(path)).size_full().object_fit(match card.kind {
            FileKind::Video | FileKind::PowerPoint => ObjectFit::Cover,
            _ => ObjectFit::Contain,
        })),
        None => frame,
    };
    let player_key = SharedString::from(format!("{id}-player"));
    let content_url = card.open_url.clone();
    Some(frame.when(card.kind == FileKind::Video, |frame| {
        let play_key = player_key.clone();
        frame
            .child(inline_video::slot(player_key))
            .on_click(move |_, window, cx| {
                if inline_video::play(play_key.clone(), &content_url, window, cx) {
                    cx.stop_propagation();
                }
            })
            .child(
                h_flex()
                    .absolute()
                    .inset_0()
                    .items_center()
                    .justify_center()
                    .child(
                        h_flex()
                            .size(px(PLAY_BUTTON_SIZE))
                            .rounded_full()
                            .items_center()
                            .justify_center()
                            .bg(black().opacity(0.5))
                            .child(icon(IconName::Play, 18., theme::white())),
                    ),
            )
    }))
}

fn file_view(
    card: &FileCard,
    id: String,
    index: usize,
    actions: Option<&FileActions>,
    directory: &Directory,
) -> AnyElement {
    let badge = file_badge(card.kind, FILE_BADGE_SIZE);
    let url = card.open_url.clone();
    let state = actions
        .and_then(|actions| actions.states.get(index))
        .and_then(Option::as_ref);
    let progress = downloads::progress_fraction(state);
    let button = actions
        .filter(|_| !card.open_url.is_empty() && card.kind != FileKind::Loop)
        .map(|actions| save_button(state, format!("{id}-save"), index, actions.activate.clone()));
    let preview = file_preview(card, &id, directory);
    let row = h_flex()
        .w_full()
        .gap(px(12.))
        .px(px(8.))
        .py(px(8.))
        .items_center()
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
        .children(button);
    v_flex()
        .id(ElementId::Name(id.into()))
        .relative()
        .overflow_hidden()
        .w_full()
        .max_w(px(FILE_CARD_WIDTH))
        .rounded(px(10.))
        .bg(theme::surface())
        .cursor_pointer()
        .hover(|card| card.bg(theme::row_hover()))
        .map(|card_view| {
            if card.kind.is_document() {
                card_view.child(row).children(preview)
            } else {
                card_view.children(preview).child(row)
            }
        })
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
        .flat_map(|block| match block {
            Block::Image { url } => vec![url.as_str()],
            table => table_images(table),
        })
        .filter_map(|url| placement(url, images, local_count))
        .collect()
}

fn draws_images(block: &Block) -> bool {
    matches!(block, Block::Image { .. }) || !table_images(block).is_empty()
}

pub fn message_body(
    blocks: &[Block],
    images: &[ImageRef],
    local_images: &[LocalImage],
    sent_previews: &[LocalImage],
    id: &str,
    own: bool,
    directory: &Directory,
    open_image: Option<&ImageActions>,
    cx: &App,
) -> Vec<AnyElement> {
    if !blocks.iter().any(draws_images) {
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
    let placed_view = |url: &str, key: &str| -> Option<AnyElement> {
        match placement(url, images, local_images.len())? {
            Placement::Remote(position) => Some(image_view(
                &images[position],
                format!("{key}-image-{position}"),
                directory,
                open_image,
                sent_preview(images, position, sent_previews),
            )),
            Placement::Local(position) => Some(local_image_view(
                &local_images[position],
                format!("{key}-local-image-{position}"),
            )),
        }
    };
    for (index, block) in blocks.iter().enumerate() {
        if !draws_images(block) {
            continue;
        }
        render_run(run_start, index, &mut elements);
        run_start = index + 1;
        match block {
            Block::Image { url } => elements.extend(placed_view(url, id)),
            Block::Table { header, rows } => {
                let table_id = format!("{id}-table-{index}");
                elements.push(render_table(*header, rows, &table_id, own, cx, &|url| {
                    placed_view(url, &table_id)
                }));
            }
            _ => {}
        }
    }
    render_run(run_start, blocks.len(), &mut elements);
    elements
}

pub fn attachments_view(
    blocks: &[Block],
    images: &[ImageRef],
    local_images: &[LocalImage],
    sent_previews: &[LocalImage],
    files: &[FileCard],
    id: &str,
    directory: &Directory,
    file_actions: Option<&FileActions>,
    open_image: Option<&ImageActions>,
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
            .children(unplaced_images.into_iter().map(|(index, image)| {
                image_view(
                    image,
                    format!("{id}-image-{index}"),
                    directory,
                    open_image,
                    sent_preview(images, index, sent_previews),
                )
            }))
            .children(
                unplaced_local.into_iter().map(|(index, image)| {
                    local_image_view(image, format!("{id}-local-image-{index}"))
                }),
            )
            .children(files.iter().enumerate().map(|(index, card)| {
                file_view(
                    card,
                    format!("{id}-file-{index}"),
                    index,
                    file_actions,
                    directory,
                )
            })),
    )
}

#[cfg(test)]
mod tests {
    use super::{
        IMAGE_FALLBACK, Placement, file_subtitle, fit_image, fit_image_within, local_image_index,
        placement, placements, sent_preview,
    };
    use crate::rows::LocalImage;
    use crate::render::{Block, TableCell};
    use teams_core::{FileCard, FileKind, ImageRef};

    fn image_ref(url: &str) -> ImageRef {
        ImageRef {
            id: url.into(),
            url: url.into(),
            width: None,
            height: None,
            alt: None,
        }
    }

    fn image_block(url: &str) -> Block {
        Block::Image { url: url.into() }
    }

    #[test]
    fn sent_previews_follow_the_uploaded_images_and_skip_gifs() {
        let gif = "https://media.giphy.com/media/x/giphy.gif";
        let images = [image_ref("hosted-a"), image_ref(gif), image_ref("hosted-b")];
        let previews: Vec<LocalImage> = [(1, 1), (2, 2)]
            .into_iter()
            .map(|size| LocalImage { image: std::sync::Arc::new(gpui_kit::Image::empty()), size: Some(size) })
            .collect();
        let size_at = |position| sent_preview(&images, position, &previews).and_then(|preview| preview.size);
        assert_eq!(size_at(0), Some((1, 1)));
        assert_eq!(size_at(1), None);
        assert_eq!(size_at(2), Some((2, 2)));
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
    fn an_image_in_a_table_cell_counts_as_placed() {
        let images = [image_ref("a"), image_ref("b")];
        let table = Block::Table {
            header: false,
            rows: vec![vec![TableCell {
                inline: Default::default(),
                images: vec!["b".into()],
            }]],
        };
        assert_eq!(placements(&[table], &images, 0), vec![Placement::Remote(1)]);
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
