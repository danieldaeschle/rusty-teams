use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{FileCard, FileKind, ImageRef};

use super::widgets::icon;
use crate::data::Directory;
use crate::format;
use crate::theme;

pub const IMAGE_MAX_WIDTH: f32 = 360.;
const IMAGE_MAX_HEIGHT: f32 = 300.;
const IMAGE_MIN_SIDE: f32 = 48.;
const IMAGE_FALLBACK: (f32, f32) = (240., 160.);
const IMAGE_RADIUS: f32 = 10.;
const FILE_CARD_WIDTH: f32 = 300.;
const FILE_BADGE_SIZE: f32 = 34.;

pub fn fit_image(size: Option<(u32, u32)>) -> (f32, f32) {
    let Some((width, height)) = size.filter(|(width, height)| *width > 0 && *height > 0) else {
        return IMAGE_FALLBACK;
    };
    let (width, height) = (width as f32, height as f32);
    let scale = (IMAGE_MAX_WIDTH / width)
        .min(IMAGE_MAX_HEIGHT / height)
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
    fit_image(declared_size(image).or(loaded))
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
                    .child("Bild wird geladen"),
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
        FileKind::Word => ("DOC", "Word-Dokument", 0x2563EB),
        FileKind::Excel => ("XLS", "Excel-Tabelle", 0x15803D),
        FileKind::PowerPoint => ("PPT", "PowerPoint", 0xC2410C),
        FileKind::Pdf => ("PDF", "PDF-Dokument", 0xB91C1C),
        FileKind::Image => ("IMG", "Bild", 0x7C3AED),
        FileKind::Archive => ("ZIP", "Archiv", 0xB45309),
        FileKind::Other => ("", "Datei", 0x525252),
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

fn file_view(card: &FileCard, id: String) -> AnyElement {
    let style = kind_style(card.kind);
    let badge = div()
        .size(px(FILE_BADGE_SIZE))
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
        .when(!style.label.is_empty(), |badge| badge.child(style.label));
    let url = card.open_url.clone();
    h_flex()
        .id(ElementId::Name(id.into()))
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
        .on_click(move |_, _, cx| cx.open_url(&url))
        .into_any_element()
}

pub fn attachments_view(
    images: &[ImageRef],
    files: &[FileCard],
    id: &str,
    directory: &Directory,
) -> Option<Div> {
    if images.is_empty() && files.is_empty() {
        return None;
    }
    Some(
        v_flex()
            .gap(px(6.))
            .items_start()
            .children(
                images.iter().enumerate().map(|(index, image)| {
                    image_view(image, format!("{id}-image-{index}"), directory)
                }),
            )
            .children(
                files
                    .iter()
                    .enumerate()
                    .map(|(index, card)| file_view(card, format!("{id}-file-{index}"))),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::{IMAGE_FALLBACK, file_subtitle, fit_image};
    use teams_core::{FileCard, FileKind};

    #[test]
    fn wide_images_shrink_to_the_max_width_keeping_aspect() {
        assert_eq!(fit_image(Some((720, 360))), (360., 180.));
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
        assert_eq!(file_subtitle(&card), "PDF-Dokument \u{b7} 2 KB");
        let unknown = FileCard { size: None, ..card };
        assert_eq!(file_subtitle(&unknown), "PDF-Dokument");
    }
}
