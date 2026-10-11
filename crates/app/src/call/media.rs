use gpui_kit::*;

pub struct CallMedia;

impl CallMedia {
    pub fn changed(media: &Option<Entity<CallMedia>>, cx: &mut App) {
        if let Some(media) = media {
            media.update(cx, |_, cx| cx.notify());
        }
    }
}
