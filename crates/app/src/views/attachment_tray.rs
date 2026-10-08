use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{FileKind, FileReference, UploadedFile};

use super::attachments::file_badge;
use super::widgets::icon;
use crate::format;
use crate::theme;

pub const MAX_ATTACHMENTS: usize = 10;
pub const INLINE_IMAGE_MAX_BYTES: u64 = 1024 * 1024;
pub const INLINE_TOTAL_MAX_BYTES: u64 = 5 * 512 * 1024;
pub const MAX_FILE_BYTES: u64 = 250 * 1024 * 1024;
pub const FILES_LATER_NOTICE: &str = "Files can be added once the chat exists.";
const PASTED_IMAGE_STEM: &str = "pasted-image";
const PREVIEW_HEIGHT: f32 = 120.;
const PREVIEW_GAP: f32 = 4.;
const PREVIEW_RADIUS: f32 = 10.;
const CHIP_WIDTH: f32 = 220.;
const CHIP_HEIGHT: f32 = 48.;
const CHIP_BADGE_SIZE: f32 = 30.;
const REMOVE_SIZE: f32 = 18.;
const TRAY_GAP: f32 = 8.;
const PROGRESS_HEIGHT: f32 = 2.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InlineFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

impl InlineFormat {
    fn image_format(self) -> ImageFormat {
        match self {
            InlineFormat::Png => ImageFormat::Png,
            InlineFormat::Jpeg => ImageFormat::Jpeg,
            InlineFormat::Gif => ImageFormat::Gif,
            InlineFormat::Webp => ImageFormat::Webp,
        }
    }

    fn decoder_format(self) -> Option<image::ImageFormat> {
        match self {
            InlineFormat::Png => Some(image::ImageFormat::Png),
            InlineFormat::Jpeg => Some(image::ImageFormat::Jpeg),
            InlineFormat::Webp => Some(image::ImageFormat::WebP),
            InlineFormat::Gif => None,
        }
    }
}

pub fn sniff_inline_format(bytes: &[u8]) -> Option<InlineFormat> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some(InlineFormat::Png),
        [0xFF, 0xD8, 0xFF, ..] => Some(InlineFormat::Jpeg),
        [b'G', b'I', b'F', b'8', ..] => Some(InlineFormat::Gif),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some(InlineFormat::Webp),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Classification {
    Inline(InlineFormat),
    File { sent_as_file: bool },
}

/// `inline_used` is the raw bytes of the images already inline in the tray.
pub fn classify(name: &str, bytes: &[u8], inline_used: u64) -> Classification {
    let size = bytes.len() as u64;
    match sniff_inline_format(bytes) {
        Some(format)
            if size <= INLINE_IMAGE_MAX_BYTES && inline_used + size <= INLINE_TOTAL_MAX_BYTES =>
        {
            Classification::Inline(format)
        }
        Some(_) => Classification::File { sent_as_file: true },
        None => Classification::File {
            sent_as_file: FileKind::from_name(name) == FileKind::Image,
        },
    }
}

fn image_dimensions(format: InlineFormat, bytes: &[u8]) -> Option<(u32, u32)> {
    match format.decoder_format() {
        Some(decoder) => image::ImageReader::with_format(Cursor::new(bytes), decoder)
            .into_dimensions()
            .ok(),
        None => {
            let header = bytes.get(6..10)?;
            Some((
                u32::from(u16::from_le_bytes([header[0], header[1]])),
                u32::from(u16::from_le_bytes([header[2], header[3]])),
            ))
        }
    }
}

pub fn limit_notice(dropped: usize) -> String {
    let files = if dropped == 1 {
        "1 file was".to_owned()
    } else {
        format!("{dropped} files were")
    };
    format!("Up to {MAX_ATTACHMENTS} attachments per message. {files} not added.")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadState {
    Uploading(u8),
    Done(DoneFile),
    Failed,
    Sharing(UploadedFile),
    ShareFailed(UploadedFile),
}

/// `uploaded` is known for files the app uploaded, so an unsent one can be deleted again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoneFile {
    pub reference: FileReference,
    pub uploaded: Option<UploadedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UploadResult {
    Done(DoneFile),
    ShareFailed(UploadedFile),
    Failed,
}

impl UploadState {
    fn uploaded(&self) -> Option<UploadedFile> {
        match self {
            UploadState::Done(done) => done.uploaded.clone(),
            UploadState::Sharing(uploaded) | UploadState::ShareFailed(uploaded) => {
                Some(uploaded.clone())
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)]
pub enum ItemKind {
    Reading,
    Image {
        image: Arc<Image>,
        dimensions: Option<(u32, u32)>,
    },
    File {
        bytes: Arc<Vec<u8>>,
        upload: UploadState,
        sent_as_file: bool,
    },
}

#[derive(Debug, Clone)]
pub struct TrayItem {
    pub id: u64,
    pub name: String,
    pub size: u64,
    pub kind: ItemKind,
}

impl TrayItem {
    fn blocks_send(&self) -> bool {
        match &self.kind {
            ItemKind::Reading => true,
            ItemKind::Image { .. } => false,
            ItemKind::File { upload, .. } => !matches!(upload, UploadState::Done(_)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PreviewImage {
    pub image: Arc<Image>,
    pub dimensions: Option<(u32, u32)>,
}

pub type RemoveImage = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingImage {
    pub name: String,
    pub image: Arc<Image>,
    pub dimensions: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutgoingFile {
    pub name: String,
    pub size: u64,
    pub kind: FileKind,
    pub reference: FileReference,
    pub uploaded: Option<UploadedFile>,
}

#[derive(Debug, Clone)]
pub enum JobKind {
    Upload,
    Share(UploadedFile),
}

#[derive(Debug, Clone)]
pub struct UploadJob {
    pub id: u64,
    pub name: String,
    pub bytes: Arc<Vec<u8>>,
    pub kind: JobKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadedFile {
    pub bytes: Vec<u8>,
    pub dimensions: Option<(u32, u32)>,
    pub file_name: Option<String>,
}

impl LoadedFile {
    pub fn new(bytes: Vec<u8>) -> Self {
        let dimensions =
            sniff_inline_format(&bytes).and_then(|format| image_dimensions(format, &bytes));
        LoadedFile {
            bytes,
            dimensions,
            file_name: None,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct AttachmentTray {
    items: Vec<TrayItem>,
    next_id: u64,
    notice: Option<String>,
}

impl AttachmentTray {
    pub fn items(&self) -> &[TrayItem] {
        &self.items
    }

    pub fn has_chips(&self) -> bool {
        self.items
            .iter()
            .any(|item| !matches!(item.kind, ItemKind::Image { .. }))
    }

    pub fn image(&self, id: u64) -> Option<PreviewImage> {
        self.items.iter().find_map(|item| match &item.kind {
            ItemKind::Image { image, dimensions } if item.id == id => Some(PreviewImage {
                image: image.clone(),
                dimensions: *dimensions,
            }),
            _ => None,
        })
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn blocks_send(&self) -> bool {
        self.items.iter().any(TrayItem::blocks_send)
    }

    fn push(&mut self, name: String, size: u64, kind: ItemKind) -> u64 {
        self.next_id += 1;
        self.items.push(TrayItem {
            id: self.next_id,
            name,
            size,
            kind,
        });
        self.next_id
    }

    /// Items beyond the limit are dropped; the ids returned belong to the first names.
    pub fn add_pending(&mut self, names: &[String]) -> Vec<u64> {
        self.notice = None;
        let accepted = names
            .len()
            .min(MAX_ATTACHMENTS.saturating_sub(self.items.len()));
        let ids = names[..accepted]
            .iter()
            .map(|name| self.push(name.clone(), 0, ItemKind::Reading))
            .collect();
        if accepted < names.len() {
            self.notice = Some(limit_notice(names.len() - accepted));
        }
        ids
    }

    /// Returns the upload to start when the item turned out to be a file.
    pub fn finish_reading(
        &mut self,
        id: u64,
        loaded: LoadedFile,
        files_allowed: bool,
    ) -> Option<UploadJob> {
        let position = self
            .items
            .iter()
            .position(|item| item.id == id && matches!(item.kind, ItemKind::Reading))?;
        let name = loaded
            .file_name
            .clone()
            .unwrap_or_else(|| self.items[position].name.clone());
        let size = loaded.bytes.len() as u64;
        match classify(&name, &loaded.bytes, self.inline_bytes()) {
            Classification::Inline(format) => {
                let item = &mut self.items[position];
                item.name = name;
                item.size = size;
                item.kind = ItemKind::Image {
                    image: Arc::new(Image::from_bytes(format.image_format(), loaded.bytes)),
                    dimensions: loaded.dimensions,
                };
                None
            }
            Classification::File { .. } if !files_allowed => {
                self.items.remove(position);
                self.notice = Some(FILES_LATER_NOTICE.to_owned());
                None
            }
            Classification::File { sent_as_file } => {
                let bytes = Arc::new(loaded.bytes);
                let item = &mut self.items[position];
                item.name = name.clone();
                item.size = size;
                item.kind = ItemKind::File {
                    bytes: bytes.clone(),
                    upload: UploadState::Uploading(0),
                    sent_as_file,
                };
                Some(UploadJob {
                    id,
                    name,
                    bytes,
                    kind: JobKind::Upload,
                })
            }
        }
    }

    pub fn fail_reading(&mut self, id: u64, message: String) {
        let before = self.items.len();
        self.items.retain(|item| item.id != id);
        if self.items.len() != before {
            self.notice = Some(message);
        }
    }

    pub fn set_progress(&mut self, id: u64, percent: u8) {
        if let Some(ItemKind::File { upload, .. }) = self.kind_of_mut(id)
            && matches!(upload, UploadState::Uploading(_))
        {
            *upload = UploadState::Uploading(percent.min(100));
        }
    }

    /// The bytes are on the server; from here a remove or switch must delete the item.
    /// Returns the file back when its item is gone, so the caller can delete it.
    pub fn mark_uploaded(&mut self, id: u64, uploaded: UploadedFile) -> Option<UploadedFile> {
        if !self.items.iter().any(|item| item.id == id) {
            return Some(uploaded);
        }
        if let Some(ItemKind::File { upload, .. }) = self.kind_of_mut(id)
            && matches!(upload, UploadState::Uploading(_))
        {
            *upload = UploadState::Sharing(uploaded);
        }
        None
    }

    /// Returns the uploaded file of a result whose item is gone, so the caller can delete it.
    pub fn finish_upload(&mut self, id: u64, result: UploadResult) -> Option<UploadedFile> {
        if !self.items.iter().any(|item| item.id == id) {
            return match result {
                UploadResult::Done(done) => done.uploaded,
                UploadResult::ShareFailed(uploaded) => Some(uploaded),
                UploadResult::Failed => None,
            };
        }
        if let Some(ItemKind::File { upload, .. }) = self.kind_of_mut(id)
            && matches!(upload, UploadState::Uploading(_) | UploadState::Sharing(_))
        {
            *upload = match result {
                UploadResult::Done(done) => UploadState::Done(done),
                UploadResult::ShareFailed(uploaded) => UploadState::ShareFailed(uploaded),
                UploadResult::Failed => UploadState::Failed,
            };
        }
        None
    }

    pub fn retry(&mut self, id: u64) -> Option<UploadJob> {
        self.notice = None;
        let item = self.items.iter_mut().find(|item| item.id == id)?;
        let ItemKind::File { bytes, upload, .. } = &mut item.kind else {
            return None;
        };
        match upload.clone() {
            UploadState::Failed => *upload = UploadState::Uploading(0),
            UploadState::ShareFailed(uploaded) => *upload = UploadState::Sharing(uploaded),
            _ => return None,
        }
        let kind = match upload {
            UploadState::Sharing(uploaded) => JobKind::Share(uploaded.clone()),
            _ => JobKind::Upload,
        };
        Some(UploadJob {
            id,
            name: item.name.clone(),
            bytes: bytes.clone(),
            kind,
        })
    }

    /// Returns the uploaded file of the removed item, which was never sent.
    pub fn remove(&mut self, id: u64) -> Option<UploadedFile> {
        self.notice = None;
        let position = self.items.iter().position(|item| item.id == id)?;
        match self.items.remove(position).kind {
            ItemKind::File { upload, .. } => upload.uploaded(),
            _ => None,
        }
    }

    /// Returns the uploaded files that were dropped unsent.
    pub fn clear(&mut self) -> Vec<UploadedFile> {
        self.notice = None;
        self.items
            .drain(..)
            .filter_map(|item| match item.kind {
                ItemKind::File { upload, .. } => upload.uploaded(),
                _ => None,
            })
            .collect()
    }

    pub fn keep_images_only(&mut self) -> Vec<UploadedFile> {
        self.notice = None;
        let (images, others): (Vec<_>, Vec<_>) = self
            .items
            .drain(..)
            .partition(|item| matches!(item.kind, ItemKind::Image { .. }));
        self.items = images;
        others
            .into_iter()
            .filter_map(|item| match item.kind {
                ItemKind::File { upload, .. } => upload.uploaded(),
                _ => None,
            })
            .collect()
    }

    fn inline_bytes(&self) -> u64 {
        self.items
            .iter()
            .filter(|item| matches!(item.kind, ItemKind::Image { .. }))
            .map(|item| item.size)
            .sum()
    }

    pub fn restore(&mut self, images: &[OutgoingImage], files: &[OutgoingFile]) -> Vec<u64> {
        self.clear();
        let image_ids = images
            .iter()
            .map(|image| {
                self.push(
                    image.name.clone(),
                    image.image.bytes.len() as u64,
                    ItemKind::Image {
                        image: image.image.clone(),
                        dimensions: image.dimensions,
                    },
                )
            })
            .collect();
        for file in files {
            self.push(
                file.name.clone(),
                file.size,
                ItemKind::File {
                    bytes: Arc::new(Vec::new()),
                    upload: UploadState::Done(DoneFile {
                        reference: file.reference.clone(),
                        uploaded: file.uploaded.clone(),
                    }),
                    sent_as_file: false,
                },
            );
        }
        image_ids
    }

    pub fn images_in(&self, ids: &[u64]) -> Vec<OutgoingImage> {
        ids.iter()
            .filter_map(|id| {
                let item = self.items.iter().find(|item| item.id == *id)?;
                match &item.kind {
                    ItemKind::Image { image, dimensions } => Some(OutgoingImage {
                        name: item.name.clone(),
                        image: image.clone(),
                        dimensions: *dimensions,
                    }),
                    _ => None,
                }
            })
            .collect()
    }

    pub fn discard_images_except(&mut self, kept: &[u64]) {
        self.items
            .retain(|item| !matches!(item.kind, ItemKind::Image { .. }) || kept.contains(&item.id));
    }

    pub fn outgoing_files(&self) -> Vec<OutgoingFile> {
        self.items
            .iter()
            .filter_map(|item| match &item.kind {
                ItemKind::File {
                    upload: UploadState::Done(done),
                    ..
                } => Some(OutgoingFile {
                    name: item.name.clone(),
                    size: item.size,
                    kind: FileKind::from_name(&item.name),
                    reference: done.reference.clone(),
                    uploaded: done.uploaded.clone(),
                }),
                _ => None,
            })
            .collect()
    }

    fn kind_of_mut(&mut self, id: u64) -> Option<&mut ItemKind> {
        self.items
            .iter_mut()
            .find(|item| item.id == id)
            .map(|item| &mut item.kind)
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

pub fn discard_uploaded(engine: Option<Arc<crate::backend::Engine>>, files: Vec<UploadedFile>) {
    let Some(engine) = engine else {
        return;
    };
    for file in files {
        let engine = engine.clone();
        drop(crate::runtime::spawn(async move {
            let _ = engine.discard_attachment(&file).await;
        }));
    }
}

pub fn names_of(paths: &[PathBuf]) -> Vec<String> {
    paths.iter().map(|path| display_name(path)).collect()
}

pub fn read_attachment(path: &Path) -> Result<LoadedFile, String> {
    let name = display_name(path);
    let unreadable = || format!("Could not read {name}.");
    let metadata = std::fs::metadata(path).map_err(|_| unreadable())?;
    if !metadata.is_file() {
        return Err(unreadable());
    }
    if metadata.len() > MAX_FILE_BYTES {
        return Err(format!(
            "{name} is larger than {} MB.",
            MAX_FILE_BYTES / (1024 * 1024)
        ));
    }
    let bytes = std::fs::read(path).map_err(|_| unreadable())?;
    if bytes.is_empty() {
        return Err(format!("{name} is empty."));
    }
    Ok(LoadedFile::new(bytes))
}

pub fn pasted_image_name() -> String {
    format!("{PASTED_IMAGE_STEM}.png")
}

/// Formats the app cannot send inline (bitmaps from the Windows clipboard) become PNG.
pub fn prepare_pasted_image(pasted: Image) -> Result<LoadedFile, String> {
    let unreadable = || format!("Could not read {}.", pasted_image_name());
    let extension = match pasted.format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::Gif => "gif",
        ImageFormat::Webp => "webp",
        ImageFormat::Bmp => {
            let decoded =
                image::load_from_memory_with_format(&pasted.bytes, image::ImageFormat::Bmp)
                    .map_err(|_| unreadable())?;
            let mut png = Vec::new();
            decoded
                .write_to(&mut Cursor::new(&mut png), image::ImageFormat::Png)
                .map_err(|_| unreadable())?;
            return Ok(LoadedFile {
                file_name: Some(pasted_image_name()),
                ..LoadedFile::new(png)
            });
        }
        _ => return Err(unreadable()),
    };
    Ok(LoadedFile {
        file_name: Some(format!("{PASTED_IMAGE_STEM}.{extension}")),
        ..LoadedFile::new(pasted.bytes)
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum PasteAction {
    Text,
    Files(Vec<PathBuf>),
    Image(Image),
}

/// Office and browser copies carry text next to a bitmap; the text is what was meant.
pub fn paste_action(item: &ClipboardItem) -> PasteAction {
    let mut pasted = None;
    for entry in item.entries() {
        match entry {
            ClipboardEntry::ExternalPaths(paths) if !paths.paths().is_empty() => {
                return PasteAction::Files(paths.paths().to_vec());
            }
            ClipboardEntry::Image(found)
                if pasted.is_none() && found.format != ImageFormat::Svg =>
            {
                pasted = Some(found.clone());
            }
            _ => {}
        }
    }
    let has_text = item.text().is_some_and(|text| !text.trim().is_empty());
    match pasted {
        Some(image) if !has_text => PasteAction::Image(image),
        _ => PasteAction::Text,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Muted,
    Danger,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChipState {
    pub subtitle: String,
    pub tone: Tone,
    pub progress: Option<u8>,
    pub failed: bool,
}

pub fn chip_state(item: &TrayItem) -> ChipState {
    let size = format::file_size_label(item.size);
    let muted = |subtitle: String| ChipState {
        subtitle,
        tone: Tone::Muted,
        progress: None,
        failed: false,
    };
    match &item.kind {
        ItemKind::Reading => muted("Reading ...".to_owned()),
        ItemKind::Image { .. } => muted(size),
        ItemKind::File {
            upload,
            sent_as_file,
            ..
        } => match upload {
            UploadState::Uploading(percent) => ChipState {
                progress: Some(*percent),
                ..muted(format!("Uploading {percent} %"))
            },
            UploadState::Sharing(_) => ChipState {
                progress: Some(100),
                ..muted("Uploading 100 %".to_owned())
            },
            UploadState::Done(_) if *sent_as_file => muted(format!("{size}, sent as file")),
            UploadState::Done(_) => muted(size),
            UploadState::Failed => ChipState {
                tone: Tone::Danger,
                failed: true,
                ..muted("Upload failed.".to_owned())
            },
            UploadState::ShareFailed(_) => ChipState {
                tone: Tone::Danger,
                failed: true,
                ..muted("Sharing failed.".to_owned())
            },
        },
    }
}

type ItemAction<T> = fn(&mut T, u64, &mut Window, &mut Context<T>);

fn remove_button<T: 'static>(id: u64, remove: ItemAction<T>, cx: &mut Context<T>) -> Stateful<Div> {
    div()
        .id(ElementId::Name(format!("attachment-remove-{id}").into()))
        .size(px(REMOVE_SIZE))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .hover(|button| button.bg(theme::row_hover()))
        .child(icon(IconName::Close, 10., theme::text_muted()))
        .on_click(cx.listener(move |this, _, window, cx| remove(this, id, window, cx)))
}

pub fn preview_size(dimensions: Option<(u32, u32)>, available_width: f32) -> (f32, f32) {
    let (width, height) = dimensions
        .filter(|(width, height)| *width > 0 && *height > 0)
        .map_or((PREVIEW_HEIGHT, PREVIEW_HEIGHT), |(width, height)| {
            (width as f32, height as f32)
        });
    let scale = (PREVIEW_HEIGHT / height)
        .min(available_width / width)
        .clamp(0., 1.);
    (
        (width * scale).round().max(1.),
        (height * scale).round().max(1.),
    )
}

pub fn preview_row_height(height: f32) -> f32 {
    height + 2. * PREVIEW_GAP
}

pub fn inline_preview(
    id: u64,
    preview: Option<PreviewImage>,
    available_width: Pixels,
    selected: bool,
    remove: RemoveImage,
) -> AnyElement {
    let dimensions = preview.as_ref().and_then(|preview| preview.dimensions);
    let (width, height) = preview_size(dimensions, f32::from(available_width));
    let group = SharedString::from(format!("image-preview-{id}"));
    let frame = div()
        .relative()
        .size_full()
        .rounded(px(PREVIEW_RADIUS))
        .overflow_hidden()
        .border_1()
        .border_color(theme::border_strong())
        .bg(theme::surface_raised());
    let frame = match preview {
        Some(preview) => frame.child(
            img(preview.image)
                .size_full()
                .rounded(px(PREVIEW_RADIUS))
                .object_fit(ObjectFit::Cover),
        ),
        None => frame,
    };
    div()
        .group(group.clone())
        .w(px(width))
        .h(px(preview_row_height(height)))
        .py(px(PREVIEW_GAP))
        .child(
            frame
                .when(selected, |frame| {
                    frame.child(
                        div()
                            .absolute()
                            .inset_0()
                            .rounded(px(PREVIEW_RADIUS))
                            .border_2()
                            .border_color(theme::accent()),
                    )
                })
                .child(
                    div()
                        .id(ElementId::Name(format!("image-remove-{id}").into()))
                        .absolute()
                        .top(px(6.))
                        .right(px(6.))
                        .size(px(REMOVE_SIZE))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .cursor_pointer()
                        .bg(black().opacity(0.6))
                        .opacity(0.)
                        .group_hover(group, |button| button.opacity(1.))
                        .hover(|button| button.bg(black().opacity(0.85)))
                        .child(icon(IconName::Close, 10., white()))
                        .on_mouse_down(MouseButton::Left, |_, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                        })
                        .on_click(move |_, window, cx| remove(window, cx)),
                ),
        )
        .into_any_element()
}

fn chip<T: 'static>(
    item: &TrayItem,
    remove: ItemAction<T>,
    retry: ItemAction<T>,
    cx: &mut Context<T>,
) -> AnyElement {
    let state = chip_state(item);
    let id = item.id;
    let subtitle_color = match state.tone {
        Tone::Muted => theme::text_muted(),
        Tone::Danger => theme::red(),
    };
    h_flex()
        .id(ElementId::Name(format!("attachment-{id}").into()))
        .relative()
        .w(px(CHIP_WIDTH))
        .h(px(CHIP_HEIGHT))
        .flex_none()
        .gap(px(9.))
        .px(px(9.))
        .items_center()
        .rounded(px(8.))
        .overflow_hidden()
        .bg(theme::surface_raised())
        .border_1()
        .border_color(if state.failed {
            theme::red()
        } else {
            theme::border_strong()
        })
        .child(file_badge(FileKind::from_name(&item.name), CHIP_BADGE_SIZE))
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(
                    div()
                        .truncate()
                        .text_size(px(12.5))
                        .text_color(theme::text())
                        .child(item.name.clone()),
                )
                .child(
                    div()
                        .truncate()
                        .text_size(px(11.))
                        .text_color(subtitle_color)
                        .child(state.subtitle),
                ),
        )
        .when(state.failed, |chip| {
            chip.child(
                div()
                    .id(ElementId::Name(format!("attachment-retry-{id}").into()))
                    .flex_none()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::accent_text())
                    .cursor_pointer()
                    .hover(|button| button.underline())
                    .child("Retry")
                    .on_click(cx.listener(move |this, _, window, cx| retry(this, id, window, cx))),
            )
        })
        .child(remove_button(id, remove, cx))
        .when_some(state.progress, |chip, percent| {
            chip.child(
                div()
                    .absolute()
                    .left_0()
                    .bottom_0()
                    .h(px(PROGRESS_HEIGHT))
                    .w(relative(f32::from(percent) / 100.))
                    .bg(theme::accent()),
            )
        })
        .into_any_element()
}

pub fn render_tray<T: 'static>(
    tray: &AttachmentTray,
    remove: ItemAction<T>,
    retry: ItemAction<T>,
    cx: &mut Context<T>,
) -> Option<Div> {
    if !tray.has_chips() {
        return None;
    }
    let elements: Vec<AnyElement> = tray
        .items()
        .iter()
        .filter(|item| !matches!(item.kind, ItemKind::Image { .. }))
        .map(|item| chip(item, remove, retry, cx))
        .collect();
    Some(
        h_flex()
            .w_full()
            .flex_wrap()
            .gap(px(TRAY_GAP))
            .px(px(12.))
            .pt(px(10.))
            .pb(px(2.))
            .children(elements),
    )
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::path::PathBuf;

    use super::JobKind;
    use gpui_kit::{ClipboardEntry, ClipboardItem, ExternalPaths, Image, ImageFormat};
    use teams_core::{FileKind, FileReference, UploadedFile};

    use super::{
        AttachmentTray, Classification, DoneFile, FILES_LATER_NOTICE, INLINE_IMAGE_MAX_BYTES,
        INLINE_TOTAL_MAX_BYTES, InlineFormat, ItemKind, LoadedFile, MAX_ATTACHMENTS, PasteAction,
        Tone, UploadResult, chip_state, classify, limit_notice, paste_action, pasted_image_name,
        prepare_pasted_image, preview_size, read_attachment, sniff_inline_format,
    };

    const PNG_HEADER: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = Vec::new();
        image::RgbaImage::new(width, height)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .unwrap();
        bytes
    }

    fn padded(mut bytes: Vec<u8>, length: u64) -> Vec<u8> {
        bytes.resize(length as usize, 0);
        bytes
    }

    fn reference(name: &str) -> FileReference {
        FileReference {
            attachment_id: format!("id-{name}"),
            content_url: format!("https://files.example/{name}"),
            name: name.to_owned(),
        }
    }

    fn done(name: &str) -> UploadResult {
        UploadResult::Done(DoneFile {
            reference: reference(name),
            uploaded: None,
        })
    }

    fn sample_upload() -> UploadedFile {
        UploadedFile {
            drive_id: "d".into(),
            item_id: "i".into(),
            name: "plan.pdf".into(),
            web_url: "https://x".into(),
            web_dav_url: None,
            etag: "\"{G},1\"".into(),
        }
    }

    fn names(count: usize) -> Vec<String> {
        (0..count)
            .map(|index| format!("file-{index}.pdf"))
            .collect()
    }

    fn loaded_file(name: &str) -> LoadedFile {
        LoadedFile {
            file_name: Some(name.to_owned()),
            ..LoadedFile::new(b"%PDF-1.7".to_vec())
        }
    }

    fn tray_with_file() -> (AttachmentTray, u64) {
        let mut tray = AttachmentTray::default();
        let id = tray.add_pending(&["plan.pdf".to_owned()])[0];
        tray.finish_reading(id, LoadedFile::new(b"%PDF-1.7".to_vec()), true);
        (tray, id)
    }

    #[test]
    fn magic_bytes_decide_the_inline_format() {
        assert_eq!(sniff_inline_format(&png(1, 1)), Some(InlineFormat::Png));
        assert_eq!(
            sniff_inline_format(&[0xFF, 0xD8, 0xFF, 0xE0, 0]),
            Some(InlineFormat::Jpeg)
        );
        assert_eq!(sniff_inline_format(b"GIF89a...."), Some(InlineFormat::Gif));
        assert_eq!(
            sniff_inline_format(b"RIFF\x10\0\0\0WEBPVP8 "),
            Some(InlineFormat::Webp)
        );
        assert_eq!(sniff_inline_format(b"BM......"), None);
        assert_eq!(sniff_inline_format(b"%PDF-1.7"), None);
        assert_eq!(sniff_inline_format(&[]), None);
    }

    #[test]
    fn small_supported_images_are_inline() {
        assert_eq!(
            classify("shot.png", &png(2, 2), 0),
            Classification::Inline(InlineFormat::Png)
        );
    }

    #[test]
    fn images_over_one_megabyte_become_files() {
        let at_limit = padded(PNG_HEADER.to_vec(), INLINE_IMAGE_MAX_BYTES);
        let over_limit = padded(PNG_HEADER.to_vec(), INLINE_IMAGE_MAX_BYTES + 1);
        assert_eq!(
            classify("a.png", &at_limit, 0),
            Classification::Inline(InlineFormat::Png)
        );
        assert_eq!(
            classify("a.png", &over_limit, 0),
            Classification::File { sent_as_file: true }
        );
    }

    #[test]
    fn the_inline_total_is_capped_and_later_images_become_files() {
        let image = padded(PNG_HEADER.to_vec(), INLINE_IMAGE_MAX_BYTES);
        assert_eq!(
            classify(
                "a.png",
                &image,
                INLINE_TOTAL_MAX_BYTES - INLINE_IMAGE_MAX_BYTES
            ),
            Classification::Inline(InlineFormat::Png)
        );
        assert_eq!(
            classify(
                "a.png",
                &image,
                INLINE_TOTAL_MAX_BYTES - INLINE_IMAGE_MAX_BYTES + 1
            ),
            Classification::File { sent_as_file: true }
        );
    }

    #[test]
    fn removing_an_inline_image_does_not_reclassify_the_others() {
        let big = || LoadedFile::new(padded(PNG_HEADER.to_vec(), INLINE_IMAGE_MAX_BYTES));
        let mut tray = AttachmentTray::default();
        let ids = tray.add_pending(&names(4));
        for id in &ids[..3] {
            tray.finish_reading(*id, big(), true);
        }
        let job = tray.finish_reading(ids[3], big(), true);
        assert!(
            job.is_some(),
            "the fourth 1 MiB image exceeds 2.5 MiB inline"
        );
        tray.remove(ids[0]);
        assert!(matches!(tray.items()[2].kind, ItemKind::File { .. }));
    }

    #[test]
    fn a_failed_share_retries_only_the_share_and_keeps_the_upload() {
        let (mut tray, id) = tray_with_file();
        let uploaded = sample_upload();
        tray.finish_upload(id, UploadResult::ShareFailed(uploaded.clone()));
        let state = chip_state(&tray.items()[0]);
        assert_eq!(state.subtitle, "Sharing failed.");
        assert!(state.failed);
        assert!(tray.blocks_send());
        let job = tray.retry(id).unwrap();
        assert!(matches!(job.kind, JobKind::Share(ref file) if *file == uploaded));
        assert_eq!(tray.clear(), vec![uploaded.clone()]);
        let (mut tray, id) = tray_with_file();
        tray.finish_upload(id, UploadResult::ShareFailed(uploaded.clone()));
        tray.retry(id);
        tray.finish_upload(id, UploadResult::ShareFailed(uploaded.clone()));
        assert_eq!(tray.remove(id), Some(uploaded));
    }

    #[test]
    fn an_item_being_shared_can_be_deleted_by_remove_or_clear() {
        let (mut tray, id) = tray_with_file();
        tray.mark_uploaded(id, sample_upload());
        assert_eq!(chip_state(&tray.items()[0]).subtitle, "Uploading 100 %");
        assert!(tray.blocks_send());
        tray.set_progress(id, 10);
        assert_eq!(chip_state(&tray.items()[0]).progress, Some(100));
        assert_eq!(tray.remove(id), Some(sample_upload()));

        let (mut tray, id) = tray_with_file();
        tray.mark_uploaded(id, sample_upload());
        tray.finish_upload(id, UploadResult::ShareFailed(sample_upload()));
        assert!(chip_state(&tray.items()[0]).failed);
        assert_eq!(tray.keep_images_only(), vec![sample_upload()]);
    }

    #[test]
    fn events_for_a_removed_item_hand_the_upload_back_for_deletion() {
        let (mut tray, id) = tray_with_file();
        tray.remove(id);
        assert_eq!(
            tray.mark_uploaded(id, sample_upload()),
            Some(sample_upload())
        );
        let done = UploadResult::Done(DoneFile {
            reference: reference("plan.pdf"),
            uploaded: Some(sample_upload()),
        });
        assert_eq!(tray.finish_upload(id, done), Some(sample_upload()));
        assert_eq!(
            tray.finish_upload(id, UploadResult::ShareFailed(sample_upload())),
            Some(sample_upload())
        );
        assert_eq!(tray.finish_upload(id, UploadResult::Failed), None);
    }

    #[test]
    fn events_for_a_live_item_are_never_handed_back() {
        let (mut tray, id) = tray_with_file();
        assert_eq!(tray.mark_uploaded(id, sample_upload()), None);
        assert_eq!(tray.finish_upload(id, done("plan.pdf")), None);
        assert_eq!(tray.finish_upload(id, done("plan.pdf")), None);
    }

    #[test]
    fn removing_or_clearing_reports_the_uploads_that_were_never_sent() {
        let (mut tray, id) = tray_with_file();
        tray.finish_upload(
            id,
            UploadResult::Done(DoneFile {
                reference: reference("plan.pdf"),
                uploaded: Some(sample_upload()),
            }),
        );
        assert_eq!(tray.clear(), vec![sample_upload()]);
        let (mut other, other_id) = tray_with_file();
        assert_eq!(other.remove(other_id), None);
    }

    #[test]
    fn other_image_formats_become_files_with_the_note() {
        assert_eq!(
            classify("scan.bmp", b"BM......", 0),
            Classification::File { sent_as_file: true }
        );
        assert_eq!(
            classify("photo.HEIC", b"....ftypheic", 0),
            Classification::File { sent_as_file: true }
        );
        assert_eq!(
            classify("plan.pdf", b"%PDF-1.7", 0),
            Classification::File {
                sent_as_file: false
            }
        );
    }

    #[test]
    fn dimensions_come_from_the_headers() {
        assert_eq!(LoadedFile::new(png(7, 3)).dimensions, Some((7, 3)));
        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&[10, 0, 20, 0]);
        assert_eq!(LoadedFile::new(gif).dimensions, Some((10, 20)));
        assert_eq!(LoadedFile::new(b"%PDF".to_vec()).dimensions, None);
    }

    #[test]
    fn the_tenth_attachment_fits_and_the_rest_are_dropped() {
        let mut tray = AttachmentTray::default();
        assert_eq!(tray.add_pending(&names(4)).len(), 4);
        let ids = tray.add_pending(&names(9));
        assert_eq!(ids.len(), 6);
        assert_eq!(tray.items().len(), MAX_ATTACHMENTS);
        assert_eq!(
            tray.notice(),
            Some("Up to 10 attachments per message. 3 files were not added.")
        );
    }

    #[test]
    fn one_dropped_file_reads_in_the_singular() {
        assert_eq!(
            limit_notice(1),
            "Up to 10 attachments per message. 1 file was not added."
        );
    }

    #[test]
    fn the_limit_notice_clears_on_the_next_change() {
        let mut tray = AttachmentTray::default();
        let ids = tray.add_pending(&names(12));
        assert!(tray.notice().is_some());
        tray.remove(ids[0]);
        assert_eq!(tray.notice(), None);
        tray.add_pending(&names(3));
        assert!(tray.notice().is_some());
        tray.clear();
        assert_eq!(tray.notice(), None);
    }

    #[test]
    fn a_file_starts_uploading_and_an_image_does_not() {
        let mut tray = AttachmentTray::default();
        let ids = tray.add_pending(&["a.pdf".to_owned(), "b.png".to_owned()]);
        let job = tray.finish_reading(ids[0], LoadedFile::new(b"%PDF".to_vec()), true);
        assert_eq!(job.as_ref().map(|job| job.name.as_str()), Some("a.pdf"));
        assert!(
            tray.finish_reading(ids[1], LoadedFile::new(png(2, 2)), true)
                .is_none()
        );
        assert!(matches!(tray.items()[1].kind, ItemKind::Image { .. }));
        assert!(tray.blocks_send());
    }

    #[test]
    fn a_pasted_name_replaces_the_placeholder() {
        let mut tray = AttachmentTray::default();
        let id = tray.add_pending(&[pasted_image_name()])[0];
        let loaded = LoadedFile {
            file_name: Some("pasted-image.jpg".into()),
            ..LoadedFile::new(vec![0xFF, 0xD8, 0xFF, 0xE0])
        };
        tray.finish_reading(id, loaded, true);
        assert_eq!(tray.items()[0].name, "pasted-image.jpg");
    }

    #[test]
    fn files_are_refused_while_the_chat_does_not_exist() {
        let mut tray = AttachmentTray::default();
        let ids = tray.add_pending(&["a.pdf".to_owned(), "b.png".to_owned()]);
        assert!(
            tray.finish_reading(ids[0], loaded_file("a.pdf"), false)
                .is_none()
        );
        tray.finish_reading(ids[1], LoadedFile::new(png(2, 2)), false);
        assert_eq!(tray.items().len(), 1);
        assert_eq!(tray.notice(), Some(FILES_LATER_NOTICE));
        assert!(!tray.blocks_send());
    }

    #[test]
    fn upload_progress_then_done_unblocks_sending() {
        let (mut tray, id) = tray_with_file();
        tray.set_progress(id, 42);
        assert_eq!(chip_state(&tray.items()[0]).subtitle, "Uploading 42 %");
        assert_eq!(chip_state(&tray.items()[0]).progress, Some(42));
        assert!(tray.blocks_send());
        tray.finish_upload(id, done("plan.pdf"));
        assert!(!tray.blocks_send());
        assert_eq!(tray.outgoing_files().len(), 1);
        assert_eq!(
            tray.outgoing_files()[0].kind,
            FileKind::from_name("plan.pdf")
        );
    }

    #[test]
    fn progress_after_the_upload_ended_is_ignored() {
        let (mut tray, id) = tray_with_file();
        tray.finish_upload(id, done("plan.pdf"));
        tray.set_progress(id, 10);
        tray.finish_upload(id, UploadResult::Failed);
        assert!(!tray.blocks_send());
    }

    #[test]
    fn a_failed_upload_blocks_sending_until_it_is_retried_or_removed() {
        let (mut tray, id) = tray_with_file();
        tray.finish_upload(id, UploadResult::Failed);
        let state = chip_state(&tray.items()[0]);
        assert_eq!(state.subtitle, "Upload failed.");
        assert_eq!(state.tone, Tone::Danger);
        assert!(state.failed);
        assert!(tray.blocks_send());
        assert!(tray.outgoing_files().is_empty());

        let job = tray.retry(id).expect("failed uploads can be retried");
        assert_eq!(job.name, "plan.pdf");
        assert_eq!(chip_state(&tray.items()[0]).subtitle, "Uploading 0 %");
        assert!(tray.retry(id).is_none());

        tray.finish_upload(id, UploadResult::Failed);
        tray.remove(id);
        assert!(!tray.blocks_send());
    }

    #[test]
    fn a_result_for_a_removed_item_is_dropped() {
        let (mut tray, id) = tray_with_file();
        tray.remove(id);
        tray.finish_upload(id, done("plan.pdf"));
        assert!(tray.items().is_empty());
    }

    #[test]
    fn done_chips_show_the_size_and_the_file_note() {
        let mut tray = AttachmentTray::default();
        let id = tray.add_pending(&["scan.bmp".to_owned()])[0];
        tray.finish_reading(id, LoadedFile::new(b"BM....".to_vec()), true);
        tray.finish_upload(id, done("scan.bmp"));
        assert_eq!(chip_state(&tray.items()[0]).subtitle, "6 B, sent as file");
        let (mut other, other_id) = tray_with_file();
        other.finish_upload(other_id, done("plan.pdf"));
        assert_eq!(chip_state(&other.items()[0]).subtitle, "8 B");
    }

    #[test]
    fn restore_brings_back_images_and_uploaded_files() {
        let mut source = AttachmentTray::default();
        let ids = source.add_pending(&["a.png".to_owned(), "b.pdf".to_owned()]);
        source.finish_reading(ids[0], LoadedFile::new(png(4, 2)), true);
        source.finish_reading(ids[1], LoadedFile::new(b"%PDF".to_vec()), true);
        source.finish_upload(ids[1], done("b.pdf"));
        let (images, files) = (source.images_in(&ids[..1]), source.outgoing_files());

        let mut restored = AttachmentTray::default();
        let image_ids = restored.restore(&images, &files);
        assert_eq!(image_ids.len(), 1);
        assert_eq!(restored.images_in(&image_ids), images);
        assert_eq!(restored.outgoing_files(), files);
        assert!(!restored.blocks_send());
    }

    #[test]
    fn images_come_back_in_the_order_asked_for_and_only_when_known() {
        let mut tray = AttachmentTray::default();
        let ids = tray.add_pending(&["a.png".to_owned(), "b.png".to_owned()]);
        tray.finish_reading(ids[0], LoadedFile::new(png(2, 2)), true);
        tray.finish_reading(ids[1], LoadedFile::new(png(4, 4)), true);
        let images = tray.images_in(&[ids[1], 99, ids[0]]);
        assert_eq!(images.len(), 2);
        assert_eq!(images[0].dimensions, Some((4, 4)));
        assert_eq!(images[1].dimensions, Some((2, 2)));
        assert!(!tray.has_chips());
        tray.discard_images_except(&[ids[1]]);
        assert_eq!(tray.items().len(), 1);
        assert!(tray.image(ids[1]).is_some());
        assert!(tray.image(ids[0]).is_none());
    }

    #[test]
    fn previews_are_at_most_120_high_and_fit_the_row() {
        assert_eq!(preview_size(Some((400, 300)), 800.), (160., 120.));
        assert_eq!(preview_size(Some((40, 30)), 800.), (40., 30.));
        assert_eq!(preview_size(Some((1000, 100)), 250.), (250., 25.));
        assert_eq!(preview_size(None, 800.), (120., 120.));
        assert_eq!(preview_size(Some((400, 300)), 0.), (1., 1.));
    }

    #[test]
    fn switching_chats_for_a_draft_keeps_only_images() {
        let mut tray = AttachmentTray::default();
        let ids = tray.add_pending(&["a.png".to_owned(), "b.pdf".to_owned()]);
        tray.finish_reading(ids[0], LoadedFile::new(png(2, 2)), true);
        tray.finish_reading(ids[1], LoadedFile::new(b"%PDF".to_vec()), true);
        tray.keep_images_only();
        assert_eq!(tray.items().len(), 1);
        assert!(matches!(tray.items()[0].kind, ItemKind::Image { .. }));
    }

    #[test]
    fn a_failed_read_removes_the_item_and_says_why() {
        let mut tray = AttachmentTray::default();
        let id = tray.add_pending(&["gone.txt".to_owned()])[0];
        tray.fail_reading(id, "Could not read gone.txt.".to_owned());
        assert!(tray.items().is_empty());
        assert_eq!(tray.notice(), Some("Could not read gone.txt."));
    }

    #[test]
    fn reading_items_block_sending() {
        let mut tray = AttachmentTray::default();
        tray.add_pending(&["a.pdf".to_owned()]);
        assert!(tray.blocks_send());
        assert_eq!(chip_state(&tray.items()[0]).subtitle, "Reading ...");
    }

    #[test]
    fn files_are_read_from_disk_with_their_limits() {
        let directory = tempfile::tempdir().unwrap();
        let file = directory.path().join("note.txt");
        std::fs::write(&file, b"hello").unwrap();
        let empty = directory.path().join("empty.txt");
        std::fs::write(&empty, b"").unwrap();

        assert_eq!(read_attachment(&file).unwrap().bytes, b"hello");
        assert_eq!(read_attachment(&empty).unwrap_err(), "empty.txt is empty.");
        assert!(
            read_attachment(directory.path())
                .unwrap_err()
                .ends_with('.')
        );
        assert_eq!(
            read_attachment(&directory.path().join("missing.txt")).unwrap_err(),
            "Could not read missing.txt."
        );
    }

    fn clipboard(entries: Vec<ClipboardEntry>) -> ClipboardItem {
        ClipboardItem { entries }
    }

    fn pasted_png() -> Image {
        Image::from_bytes(ImageFormat::Png, png(2, 2))
    }

    #[test]
    fn pasting_an_image_alone_adds_an_image() {
        let item = clipboard(vec![ClipboardEntry::Image(pasted_png())]);
        assert_eq!(paste_action(&item), PasteAction::Image(pasted_png()));
    }

    #[test]
    fn pasting_copied_files_adds_those_files() {
        let paths = ExternalPaths(vec![PathBuf::from("/a/b.pdf")].into_iter().collect());
        let item = clipboard(vec![ClipboardEntry::ExternalPaths(paths)]);
        assert_eq!(
            paste_action(&item),
            PasteAction::Files(vec![PathBuf::from("/a/b.pdf")])
        );
    }

    #[test]
    fn pasting_text_or_text_with_a_bitmap_stays_text() {
        assert_eq!(
            paste_action(&ClipboardItem::new_string("hi".into())),
            PasteAction::Text
        );
        let both = clipboard(vec![
            ClipboardEntry::from("cells".to_owned()),
            ClipboardEntry::Image(pasted_png()),
        ]);
        assert_eq!(paste_action(&both), PasteAction::Text);
        assert_eq!(paste_action(&clipboard(Vec::new())), PasteAction::Text);
    }

    #[test]
    fn a_pasted_bitmap_becomes_a_png() {
        let mut bmp = Vec::new();
        image::RgbaImage::new(3, 2)
            .write_to(&mut Cursor::new(&mut bmp), image::ImageFormat::Bmp)
            .unwrap();
        let loaded = prepare_pasted_image(Image::from_bytes(ImageFormat::Bmp, bmp)).unwrap();
        assert_eq!(sniff_inline_format(&loaded.bytes), Some(InlineFormat::Png));
        assert_eq!(loaded.dimensions, Some((3, 2)));
        assert_eq!(loaded.file_name.as_deref(), Some("pasted-image.png"));
    }

    #[test]
    fn a_pasted_png_keeps_its_bytes() {
        let loaded = prepare_pasted_image(pasted_png()).unwrap();
        assert_eq!(loaded.bytes, png(2, 2));
        assert_eq!(loaded.file_name.as_deref(), Some("pasted-image.png"));
    }
}
