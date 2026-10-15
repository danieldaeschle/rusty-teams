use std::path::PathBuf;
use std::rc::Rc;

use gpui_kit::assets::IconName;
use gpui_kit::component::{
    Icon,
    menu::{ContextMenuExt as _, PopupMenuItem},
};
use gpui_kit::*;

use super::image_viewer::{suggested_name, write_image};
use super::widgets::icon;
use crate::downloads;
use crate::image_clipboard::ClipboardImage;
use crate::theme;

const MENU_ICON_SIZE: f32 = 16.;

pub type ImageShare = Rc<dyn Fn(&mut App)>;

fn menu_icon(glyph: IconName) -> Icon {
    icon(glyph, MENU_ICON_SIZE, theme::text_muted())
}

pub fn copy_image_file(path: PathBuf, cx: &mut App) {
    cx.spawn(async move |cx| {
        let image = cx
            .background_spawn(async move {
                std::fs::read(&path)
                    .ok()
                    .and_then(|bytes| ClipboardImage::from_bytes(&bytes))
            })
            .await;
        if let Some(image) = image {
            cx.update(|cx| image.write(cx));
        }
    })
    .detach();
}

fn download_image_file(source: PathBuf, cx: &mut App) {
    let name = suggested_name(&source, false);
    let receiver = cx.prompt_for_new_path(&downloads::downloads_directory(), Some(&name));
    cx.background_spawn(async move {
        if let Ok(Ok(Some(target))) = receiver.await {
            if let Err(error) = write_image(&source, &target, 0) {
                log::warn!("image download to {} failed: {error}", target.display());
            }
        }
    })
    .detach();
}

pub fn with_image_menu(
    frame: Stateful<Div>,
    path: PathBuf,
    share: Option<ImageShare>,
) -> impl IntoElement {
    frame
        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
        .context_menu(move |popup, _, _| {
            let (copy_path, download_path) = (path.clone(), path.clone());
            let popup = popup
                .item(
                    PopupMenuItem::new("Copy image")
                        .icon(menu_icon(IconName::Copy))
                        .on_click(move |_, _, cx| copy_image_file(copy_path.clone(), cx)),
                )
                .item(
                    PopupMenuItem::new("Download")
                        .icon(menu_icon(IconName::Download))
                        .on_click(move |_, _, cx| download_image_file(download_path.clone(), cx)),
                );
            match share.clone() {
                Some(share) => popup.item(
                    PopupMenuItem::new("Share")
                        .icon(menu_icon(IconName::Share))
                        .on_click(move |_, _, cx| share(cx)),
                ),
                None => popup,
            }
        })
}
