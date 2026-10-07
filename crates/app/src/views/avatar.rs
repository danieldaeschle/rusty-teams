use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::data::{AvatarState, Directory, PresenceKind};
use crate::format;
use crate::sidebar_model::{AvatarSpec, Face};
use crate::theme;

const PAIR_RATIO: f32 = 24. / 36.;
const PRESENCE_MIN: f32 = 10.;
const PRESENCE_MAX: f32 = 14.;
const PRESENCE_RATIO: f32 = 0.38;
const PRESENCE_RING: f32 = 2.;
const INITIALS_RATIO: f32 = 0.36;
const PAIR_INITIALS_RATIO: f32 = 0.42;

fn initials_circle(name: &str, key: &str, size: f32, pending: bool, text_ratio: f32) -> Div {
    let (background, foreground) = if pending {
        (theme::surface_raised(), theme::text_muted())
    } else {
        (
            theme::avatar_color(format::palette_index(key)),
            theme::text(),
        )
    };
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(background)
        .text_color(foreground)
        .text_size(px((size * text_ratio).round()))
        .font_weight(FontWeight::SEMIBOLD)
        .child(format::initials(name))
}

fn face_circle(directory: &Directory, face: &Face, size: f32, text_ratio: f32) -> AnyElement {
    let key = face.user_id.as_deref().unwrap_or(&face.name);
    let state = face
        .user_id
        .as_deref()
        .and_then(|user_id| directory.avatar(user_id));
    match state {
        Some(AvatarState::Ready(image)) => {
            let texture_size = size / crate::avatar_image::visible_fraction();
            let offset = (size - texture_size) / 2.;
            div()
                .relative()
                .size(px(size))
                .flex_none()
                .child(
                    img(ImageSource::Image(image.clone()))
                        .absolute()
                        .left(px(offset))
                        .top(px(offset))
                        .size(px(texture_size)),
                )
                .into_any_element()
        }
        Some(AvatarState::Pending) => {
            initials_circle(&face.name, key, size, true, text_ratio).into_any_element()
        }
        _ => initials_circle(&face.name, key, size, false, text_ratio).into_any_element(),
    }
}

pub fn person_avatar(
    directory: &Directory,
    user_id: Option<&str>,
    name: &str,
    size: f32,
) -> AnyElement {
    let face = Face {
        user_id: user_id.map(str::to_owned),
        name: name.to_owned(),
    };
    face_circle(directory, &face, size, INITIALS_RATIO)
}

pub fn presence_size(avatar_size: f32) -> f32 {
    (avatar_size * PRESENCE_RATIO)
        .round()
        .clamp(PRESENCE_MIN, PRESENCE_MAX)
}

pub fn presence_dot(kind: PresenceKind, ring: Hsla, avatar_size: f32) -> Option<Div> {
    let outer = presence_size(avatar_size);
    let inner = outer - 2. * PRESENCE_RING;
    let fill = match kind {
        PresenceKind::Available => theme::green(),
        PresenceKind::Busy | PresenceKind::DoNotDisturb => theme::red(),
        PresenceKind::Away => theme::amber(),
        PresenceKind::Offline => theme::background(),
        PresenceKind::Unknown => return None,
    };
    let mut center = div()
        .size(px(inner))
        .rounded_full()
        .bg(fill)
        .flex()
        .items_center()
        .justify_center();
    center = match kind {
        PresenceKind::DoNotDisturb => {
            center.child(div().w(px(inner - 4.)).h(px(2.)).rounded(px(1.)).bg(ring))
        }
        PresenceKind::Offline => center.border(px(1.5)).border_color(theme::text_muted()),
        _ => center,
    };
    Some(
        div()
            .absolute()
            .right(px(-2.))
            .bottom(px(-2.))
            .size(px(outer))
            .rounded_full()
            .bg(ring)
            .flex()
            .items_center()
            .justify_center()
            .child(center),
    )
}

pub fn with_presence(avatar: AnyElement, kind: PresenceKind, size: f32, ring: Hsla) -> Div {
    let mut wrapper = div().relative().size(px(size)).flex_none().child(avatar);
    if let Some(dot) = presence_dot(kind, ring, size) {
        wrapper = wrapper.child(dot);
    }
    wrapper
}

pub fn spec_avatar(directory: &Directory, spec: &AvatarSpec, size: f32, ring: Hsla) -> AnyElement {
    match spec {
        AvatarSpec::Single(face) => face_circle(directory, face, size, INITIALS_RATIO),
        AvatarSpec::Pair(first, second) => {
            let small = (size * PAIR_RATIO).round();
            div()
                .relative()
                .size(px(size))
                .flex_none()
                .child(div().absolute().left_0().top_0().child(face_circle(
                    directory,
                    first,
                    small,
                    PAIR_INITIALS_RATIO,
                )))
                .child(
                    div()
                        .absolute()
                        .right_0()
                        .bottom_0()
                        .size(px(small))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(ring)
                        .child(face_circle(
                            directory,
                            second,
                            small - 2. * PRESENCE_RING,
                            PAIR_INITIALS_RATIO,
                        )),
                )
                .into_any_element()
        }
    }
}

pub fn square_avatar(name: &str, key: &str, size: f32, radius: f32) -> Div {
    div()
        .size(px(size))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(radius))
        .bg(theme::avatar_color(format::palette_index(key)))
        .text_color(theme::text())
        .text_size(px((size * 0.4).round()))
        .font_weight(FontWeight::SEMIBOLD)
        .child(format::initials(name))
}

pub fn member_stack(directory: &Directory, faces: &[Face], total: usize) -> Div {
    const LIMIT: usize = 3;
    const SIZE: f32 = 22.;
    const OVERLAP: f32 = 6.;
    let visible = faces.len().min(LIMIT) + usize::from(total > LIMIT);
    let width = SIZE * visible as f32 - OVERLAP * visible.saturating_sub(1) as f32;
    let mut stack = div().flex().flex_none().w(px(width)).items_center();
    for (index, face) in faces.iter().take(LIMIT).enumerate() {
        stack = stack.child(
            div()
                .when(index > 0, |element| element.ml(px(-OVERLAP)))
                .size(px(SIZE))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(theme::background())
                .child(face_circle(
                    directory,
                    face,
                    SIZE - 2. * PRESENCE_RING,
                    0.45,
                )),
        );
    }
    if total > LIMIT {
        stack = stack.child(
            div()
                .ml(px(-OVERLAP))
                .size(px(SIZE))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .border(px(PRESENCE_RING))
                .border_color(theme::background())
                .bg(theme::surface_raised())
                .text_color(theme::text_soft())
                .text_size(px(10.))
                .font_weight(FontWeight::SEMIBOLD)
                .child(format!("+{}", total - LIMIT)),
        );
    }
    stack
}

#[cfg(test)]
mod tests {
    use super::presence_size;

    #[test]
    fn presence_dot_scales_with_the_avatar() {
        assert_eq!(presence_size(26.), 10.);
        assert_eq!(presence_size(36.), 14.);
        assert_eq!(presence_size(20.), 10.);
    }
}
