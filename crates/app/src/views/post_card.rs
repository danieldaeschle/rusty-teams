use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::adaptive_card::cards_view;
use super::attachments::{attachments_view, message_body};
use super::avatar::{bot_avatar, person_avatar};
use super::profile_card::opens_profile;
use super::link_preview::{link_preview_card, meeting_join_chip};
use super::message_actions::message_toolbar;
use super::message_row::{BODY_SIZE, RowActions, delivery_note, forwarded_header, has_text};
use super::reaction_pills::reaction_pills;
use super::scheduled_toolbar::scheduled_toolbar;
use super::translation_line::translation_line;
use super::widgets::{icon, symbol};
use crate::data::Directory;
use crate::rows::{Delivery, MessageRow, PostRow};
use crate::sidebar_model::DELETED_PREVIEW;
use crate::theme;

const ROOT_AVATAR: f32 = 32.;
const REPLY_AVATAR: f32 = 24.;
const AVATAR_GAP: f32 = 10.;
const CARD_PADDING: f32 = 12.;
const CARD_RADIUS: f32 = 8.;
const CARD_GAP: f32 = 12.;
const SLOTS_PER_POST: usize = 64;
const TOOLBAR_LIFT: f32 = 22.;
const SUBJECT_SIZE: f32 = 14.;

pub type PostAction = Box<dyn Fn(&mut Window, &mut App)>;

pub struct PostActions {
    pub root: RowActions,
    pub replies: Vec<RowActions>,
    pub open_thread: Option<PostAction>,
    pub open_reply: Option<PostAction>,
    pub reply_editor: Option<AnyElement>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Role {
    Root,
    Reply,
}

fn message_index(post_index: usize, slot: usize) -> usize {
    post_index * SLOTS_PER_POST + slot.min(SLOTS_PER_POST - 1)
}

fn edited_marker() -> Div {
    div()
        .text_size(px(11.))
        .italic()
        .text_color(theme::text_muted())
        .child("Edited")
}

fn message_content(
    row: &MessageRow,
    index: usize,
    directory: &Directory,
    actions: &RowActions,
    cx: &App,
) -> Div {
    let mut content = v_flex().gap(px(6.));
    if row.deleted {
        return content.child(
            div()
                .text_size(px(13.))
                .italic()
                .text_color(theme::text_muted())
                .child(DELETED_PREVIEW),
        );
    }
    if row.forwarded {
        content = content.child(forwarded_header());
    }
    if has_text(row) {
        content = content.child(
            v_flex()
                .gap(px(6.))
                .text_size(px(BODY_SIZE))
                .line_height(relative(1.45))
                .text_color(theme::text_strong())
                .children(message_body(
                    &row.blocks,
                    &row.images,
                    &row.local_images,
                    &format!("message-{index}"),
                    false,
                    directory,
                    cx,
                )),
        );
    }
    content = content.children(attachments_view(
        &row.blocks,
        &row.images,
        &row.local_images,
        &row.files,
        &format!("message-{index}"),
        directory,
        actions.files.as_ref(),
    ));
    content = content.children(
        row.meeting_link
            .clone()
            .map(|url| meeting_join_chip(format!("message-{index}-join"), url)),
    );
    content = content.children(row.link_preview.as_ref().map(|preview| {
        link_preview_card(
            preview,
            format!("message-{index}-link"),
            directory,
            false,
            None,
        )
    }));
    content = content.children(cards_view(
        &row.adaptive_cards,
        &row.conversation_id,
        &row.key,
        cx,
    ));
    if !row.reactions.is_empty() {
        content = content.child(reaction_pills(
            &row.reactions,
            index,
            false,
            directory,
            actions.react.clone(),
            actions.reaction_controls.as_ref(),
        ));
    }
    content
}

fn message_header(
    row: &MessageRow,
    index: usize,
    author: String,
    profile_user: Option<&str>,
    saved: bool,
) -> Div {
    h_flex()
        .gap(px(6.))
        .items_center()
        .child(
            opens_profile(
                div().id(ElementId::Name(format!("post-author-{index}").into())),
                profile_user,
            )
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child(author),
        )
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme::text_muted())
                .child(row.time.clone()),
        )
        .when(row.edited, |line| line.child(edited_marker()))
        .when(saved, |line| {
            line.child(icon(IconName::Bookmark, 12., theme::accent_text()))
        })
        .when(row.delivery == Delivery::Sending, |line| {
            line.child(symbol("schedule", 13., theme::text_muted()))
        })
}

fn message_block(
    row: &MessageRow,
    index: usize,
    role: Role,
    actions: RowActions,
    directory: &Directory,
    cx: &App,
) -> Stateful<Div> {
    let size = if role == Role::Root {
        ROOT_AVATAR
    } else {
        REPLY_AVATAR
    };
    let author = actions
        .bot
        .as_ref()
        .map_or_else(|| row.author.clone(), |bot| bot.name.clone());
    let avatar = match &actions.bot {
        Some(bot) => bot_avatar(bot, size),
        None => person_avatar(directory, row.sender_id.as_deref(), &row.author, size),
    };
    let profile_user = row.sender_id.as_deref().filter(|_| actions.bot.is_none());
    let content = message_content(row, index, directory, &actions, cx);
    let saved = actions.saved;
    let RowActions {
        delivery,
        reply,
        hovered,
        menu,
        scheduled_menu,
        highlighted,
        translation: translation_actions,
        ..
    } = actions;
    let note = delivery_note(row, delivery, index);
    let translation = translation_line(row, index, translation_actions.as_ref());
    let body = v_flex()
        .flex_1()
        .min_w_0()
        .gap(px(6.))
        .child(message_header(row, index, author, profile_user, saved))
        .when_some(
            row.subject.clone().filter(|_| role == Role::Root),
            |column, subject| {
                column.child(
                    div()
                        .text_size(px(SUBJECT_SIZE))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme::text())
                        .child(subject),
                )
            },
        )
        .child(content)
        .children(translation)
        .children(note);
    div()
        .id(ElementId::Name(format!("post-message-{index}").into()))
        .relative()
        .w_full()
        .p(px(CARD_PADDING))
        .when(role == Role::Reply, |block| {
            block
                .pl(px(CARD_PADDING + ROOT_AVATAR + AVATAR_GAP))
                .border_t_1()
                .border_color(theme::border())
        })
        .when(highlighted, |block| block.bg(theme::accent().opacity(0.2)))
        .when_some(hovered, |block, hovered| {
            block.on_hover(move |is_hovered, _, cx| hovered(*is_hovered, cx))
        })
        .when_some(reply, |block, reply| {
            block.on_mouse_down(MouseButton::Right, move |_, _, cx| reply(cx))
        })
        .child(
            h_flex()
                .items_start()
                .gap(px(if role == Role::Root { AVATAR_GAP } else { 8. }))
                .child(
                    opens_profile(
                        div().id(ElementId::Name(format!("post-avatar-{index}").into())),
                        profile_user,
                    )
                    .w(px(size))
                    .flex_none()
                    .child(avatar),
                )
                .child(body),
        )
        .when_some(menu, |block, menu| {
            block.child(
                div()
                    .absolute()
                    .top(px(-TOOLBAR_LIFT))
                    .right(px(8.))
                    .child(deferred(message_toolbar(menu)).with_priority(1)),
            )
        })
        .when_some(scheduled_menu, |block, menu| {
            block.child(
                div()
                    .absolute()
                    .top(px(-TOOLBAR_LIFT))
                    .right(px(8.))
                    .child(deferred(scheduled_toolbar(menu)).with_priority(1)),
            )
        })
}

fn replies_link(label: String, post_index: usize, open: Option<PostAction>) -> Stateful<Div> {
    div()
        .id(ElementId::Name(format!("post-replies-{post_index}").into()))
        .pl(px(CARD_PADDING + ROOT_AVATAR + AVATAR_GAP))
        .pr(px(CARD_PADDING))
        .pb(px(8.))
        .text_size(px(13.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(theme::accent_text())
        .when_some(open, |link, open| {
            link.cursor_pointer()
                .hover(|link| link.text_color(theme::accent_soft()))
                .on_click(move |_, window, cx| open(window, cx))
        })
        .child(label)
}

fn reply_footer(
    post_index: usize,
    open: Option<PostAction>,
    editor: Option<AnyElement>,
) -> Option<Div> {
    let footer = v_flex()
        .w_full()
        .px(px(CARD_PADDING))
        .py(px(8.))
        .border_t_1()
        .border_color(theme::border());
    if let Some(editor) = editor {
        return Some(footer.child(editor));
    }
    let open = open?;
    Some(
        footer.child(
            h_flex().child(
                h_flex()
                    .id(ElementId::Name(format!("post-reply-{post_index}").into()))
                    .gap(px(6.))
                    .px(px(6.))
                    .py(px(4.))
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_size(px(12.5))
                    .text_color(theme::text_muted())
                    .hover(|button| {
                        button
                            .bg(theme::row_hover())
                            .text_color(theme::text_strong())
                    })
                    .on_click(move |_, window, cx| open(window, cx))
                    .child(icon(IconName::Reply, 14., theme::text_muted()))
                    .child("Reply"),
            ),
        ),
    )
}

pub fn render_post(
    post: &PostRow,
    index: usize,
    actions: PostActions,
    directory: &Directory,
    cx: &App,
) -> AnyElement {
    let PostActions {
        root,
        replies,
        open_thread,
        open_reply,
        reply_editor,
    } = actions;
    let mut card = v_flex()
        .w_full()
        .rounded(px(CARD_RADIUS))
        .bg(theme::surface())
        .border_1()
        .border_color(theme::border())
        .child(message_block(
            &post.root,
            message_index(index, 0),
            Role::Root,
            root,
            directory,
            cx,
        ));
    if let Some(label) = post.hidden_replies_label() {
        card = card.child(replies_link(label, index, open_thread));
    }
    for (position, (reply, actions)) in post.replies.iter().zip(replies).enumerate() {
        card = card.child(message_block(
            reply,
            message_index(index, position + 1),
            Role::Reply,
            actions,
            directory,
            cx,
        ));
    }
    card = card.children(reply_footer(index, open_reply, reply_editor));
    div()
        .w_full()
        .px(px(24.))
        .pt(px(CARD_GAP))
        .child(card)
        .into_any_element()
}
