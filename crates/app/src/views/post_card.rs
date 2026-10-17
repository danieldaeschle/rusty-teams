use gpui_kit::assets::IconName;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::adaptive_card::cards_view;
use super::attachments::{attachments_view, message_body};
use super::avatar::{bot_avatar, person_avatar, with_presence};
use super::call_card::{CallActions, post_call_card};
use super::call_notice::notice_line;
use super::link_preview::{link_preview_card, meeting_join_chip};
use super::message_actions::{add_reaction_pill, message_toolbar};
use super::message_row::{
    BODY_SIZE, RowActions, deleted_tombstone, delivery_note, forwarded_header, has_text,
};
use super::profile_card::opens_profile;
use super::reaction_pills::{PILL_HEIGHT, PillLook, reaction_pills};
use super::reaction_proposal_pill::reaction_pill;
use super::scheduled_toolbar::scheduled_toolbar;
use super::toolbar_placement::placed_toolbar;
use super::translation_line::translation_line;
use super::widgets::{icon, symbol};
use crate::agents::run_chip;
use crate::data::Directory;
use crate::rows::{Delivery, MessageRow, PostRow};
use crate::theme;

const ROOT_AVATAR: f32 = 32.;
const REPLY_AVATAR: f32 = 24.;
const AVATAR_GAP: f32 = 10.;
const CARD_PADDING: f32 = 12.;
const CARD_RADIUS: f32 = 10.;
const CARD_GAP: f32 = 12.;
const PINNED_LABEL: &str = "Pinned";
const SLOTS_PER_POST: usize = 64;
const TOOLBAR_RIGHT_INSET: f32 = 8.;
const SUBJECT_SIZE: f32 = 14.;
const FOOTER_AVATAR: f32 = 20.;
const FOOTER_BUTTON_PADDING: f32 = 6.;
const FOOTER_BUTTON_VERTICAL: f32 = 4.;
const REPLY_IN_THREAD: &str = "Reply in thread";
pub const POST_CARD_TAG: &str = "post-card";
pub const POST_AVATAR_TAG: &str = "post-card/avatar";
pub const POST_BODY_TAG: &str = "post-card/body";
pub const POST_FOOTER_AVATAR_TAG: &str = "post-card/footer-avatar";

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

fn pinned_marker() -> Div {
    h_flex()
        .gap(px(3.))
        .items_center()
        .text_size(px(11.))
        .text_color(theme::text_muted())
        .child(icon(IconName::Pin, 11., theme::text_muted()))
        .child(PINNED_LABEL)
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
        return content.child(deleted_tombstone(index, actions.undo_delete.clone()));
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
                    actions.open_image.as_ref(),
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
        actions.open_image.as_ref(),
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
        content = content.child(
            h_flex()
                .gap(px(4.))
                .flex_wrap()
                .items_center()
                .child(reaction_pills(
                    &row.reactions,
                    index,
                    false,
                    PillLook::Count,
                    directory,
                    actions.react.clone(),
                    actions.reaction_controls.as_ref(),
                ))
                .children(
                    actions
                        .add_reaction
                        .clone()
                        .map(|adder| add_reaction_pill(adder, PILL_HEIGHT, cx)),
                ),
        );
    }
    content
}

fn message_header(
    row: &MessageRow,
    index: usize,
    author: String,
    profile_user: Option<&str>,
    saved: bool,
    pinned: bool,
) -> Div {
    h_flex()
        .gap(px(6.))
        .items_center()
        .child(author_name(index, author, profile_user))
        .child(
            div()
                .text_size(px(11.))
                .text_color(theme::text_muted())
                .child(row.time.clone()),
        )
        .when(row.edited, |line| line.child(edited_marker()))
        .when(pinned, |line| line.child(pinned_marker()))
        .when(saved, |line| {
            line.child(icon(IconName::Bookmark, 12., theme::accent_text()))
        })
        .when(row.delivery == Delivery::Sending, |line| {
            line.child(symbol("schedule", 13., theme::text_muted()))
        })
}

fn author_name(index: usize, author: String, profile_user: Option<&str>) -> Stateful<Div> {
    opens_profile(
        div().id(ElementId::Name(format!("post-author-{index}").into())),
        profile_user,
    )
    .text_size(px(13.))
    .font_weight(FontWeight::SEMIBOLD)
    .text_color(theme::text())
    .child(author)
}

fn root_header(
    row: &MessageRow,
    index: usize,
    author: String,
    profile_user: Option<&str>,
    saved: bool,
    pinned: bool,
) -> Div {
    v_flex()
        .min_w_0()
        .child(
            h_flex()
                .gap(px(6.))
                .items_center()
                .child(author_name(index, author, profile_user))
                .when(saved, |line| {
                    line.child(icon(IconName::Bookmark, 12., theme::accent_text()))
                })
                .when(row.delivery == Delivery::Sending, |line| {
                    line.child(symbol("schedule", 13., theme::text_muted()))
                }),
        )
        .child(
            h_flex()
                .gap(px(6.))
                .items_center()
                .text_size(px(11.))
                .text_color(theme::text_muted())
                .child(row.time.clone())
                .when(row.edited, |line| line.child(edited_marker()))
                .when(pinned, |line| line.child(pinned_marker())),
        )
}

fn message_block(
    row: &MessageRow,
    index: usize,
    role: Role,
    mut actions: RowActions,
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
    let avatar = match (&actions.bot, row.sender_id.as_deref()) {
        (Some(bot), _) => bot_avatar(bot, size),
        (None, Some(sender_id)) if role == Role::Root => with_presence(
            person_avatar(directory, Some(sender_id), &row.author, size),
            directory.presence_of(sender_id),
            size,
            theme::row_hover(),
        )
        .into_any_element(),
        (None, sender_id) => person_avatar(directory, sender_id, &row.author, size),
    };
    if let (Role::Reply, Some(call)) = (role, &row.call)
        && let Some(notice) = &call.notice
    {
        let recap = std::mem::replace(&mut actions.call, CallActions::none()).recap;
        return div()
            .id(ElementId::Name(format!("post-message-{index}").into()))
            .w_full()
            .px(px(CARD_PADDING))
            .pb(px(CARD_PADDING))
            .bg(theme::reply_surface())
            .child(notice_line(call, notice, index, directory, recap));
    }
    let profile_user = row.sender_id.as_deref().filter(|_| actions.bot.is_none());
    let content = match &row.call {
        Some(call) => v_flex().child(post_call_card(
            call,
            index,
            std::mem::replace(&mut actions.call, CallActions::none()),
            directory,
        )),
        None => message_content(row, index, directory, &actions, cx),
    };
    let saved = actions.saved;
    let pinned = actions.pinned && role == Role::Root;
    let RowActions {
        delivery,
        reply,
        hovered,
        menu,
        scheduled_menu,
        highlighted,
        translation: translation_actions,
        timeline,
        agent_run,
        reaction_proposal,
        ..
    } = actions;
    let note = delivery_note(row, delivery, index);
    let translation = translation_line(row, index, translation_actions.as_ref());
    let subject = row
        .subject
        .clone()
        .filter(|_| role == Role::Root)
        .map(|subject| {
            div()
                .text_size(px(SUBJECT_SIZE))
                .font_weight(FontWeight::BOLD)
                .text_color(theme::text())
                .child(subject)
        });
    let details = v_flex()
        .w_full()
        .min_w_0()
        .gap(px(6.))
        .children(subject)
        .child(content)
        .children(translation)
        .children(note)
        .children(agent_run.map(|run| run_chip(&run, theme::surface_raised())))
        .children(reaction_proposal.map(|pill| reaction_pill(pill, &row.key)));
    let avatar = opens_profile(
        div().id(ElementId::Name(format!("post-avatar-{index}").into())),
        profile_user,
    )
    .w(px(size))
    .flex_none()
    .child(avatar);
    let layout = match role {
        Role::Root => v_flex()
            .w_full()
            .gap(px(8.))
            .child(
                h_flex()
                    .items_center()
                    .gap(px(AVATAR_GAP))
                    .child(avatar.debug_selector(|| POST_AVATAR_TAG.into()))
                    .child(root_header(row, index, author, profile_user, saved, pinned)),
            )
            .child(details.debug_selector(|| POST_BODY_TAG.into())),
        Role::Reply => h_flex().items_start().gap(px(8.)).child(avatar).child(
            v_flex()
                .flex_1()
                .min_w_0()
                .gap(px(6.))
                .child(message_header(
                    row,
                    index,
                    author,
                    profile_user,
                    saved,
                    pinned,
                ))
                .child(details),
        ),
    };
    div()
        .id(ElementId::Name(format!("post-message-{index}").into()))
        .relative()
        .w_full()
        .p(px(CARD_PADDING))
        .when(role == Role::Reply, |block| {
            block
                .pl(px(CARD_PADDING + ROOT_AVATAR + AVATAR_GAP))
                .bg(theme::reply_surface())
        })
        .when(highlighted, |block| block.bg(theme::accent().opacity(0.2)))
        .when_some(hovered, |block, hovered| {
            block.on_hover(move |is_hovered, _, cx| hovered(*is_hovered, cx))
        })
        .when_some(reply, |block, reply| {
            block.on_mouse_down(MouseButton::Right, move |_, _, cx| reply(cx))
        })
        .child(layout)
        .when_some(menu, |block, menu| {
            block.child(placed_toolbar(
                message_toolbar(menu, cx),
                timeline.clone(),
                TOOLBAR_RIGHT_INSET,
            ))
        })
        .when_some(scheduled_menu, |block, menu| {
            block.child(placed_toolbar(
                scheduled_toolbar(menu),
                timeline.clone(),
                TOOLBAR_RIGHT_INSET,
            ))
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
    directory: &Directory,
) -> Option<Div> {
    let footer = v_flex()
        .w_full()
        .px(px(CARD_PADDING - FOOTER_BUTTON_PADDING))
        .rounded_b(px(CARD_RADIUS))
        .bg(theme::reply_surface());
    if let Some(editor) = editor {
        return Some(footer.px(px(CARD_PADDING)).py(px(8.)).child(editor));
    }
    let open = open?;
    let own_avatar = directory.me.as_ref().map(|me| {
        div()
            .debug_selector(|| POST_FOOTER_AVATAR_TAG.into())
            .flex_none()
            .child(person_avatar(
                directory,
                Some(&me.user_id),
                &me.display_name,
                FOOTER_AVATAR,
            ))
    });
    Some(
        footer.py(px(CARD_PADDING - FOOTER_BUTTON_VERTICAL)).child(
            h_flex().child(
                h_flex()
                    .id(ElementId::Name(format!("post-reply-{post_index}").into()))
                    .gap(px(8.))
                    .px(px(FOOTER_BUTTON_PADDING))
                    .py(px(FOOTER_BUTTON_VERTICAL))
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_soft())
                    .hover(|button| {
                        button
                            .bg(theme::row_hover())
                            .text_color(theme::text_strong())
                    })
                    .on_click(move |_, window, cx| open(window, cx))
                    .children(own_avatar)
                    .child(REPLY_IN_THREAD),
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
        .debug_selector(|| POST_CARD_TAG.into())
        .w_full()
        .rounded(px(CARD_RADIUS))
        .bg(theme::row_hover())
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
    let has_footer = open_reply.is_some() || reply_editor.is_some();
    let reply_count = post.replies.len();
    for (position, (reply, actions)) in post.replies.iter().zip(replies).enumerate() {
        let closes_card = !has_footer && position + 1 == reply_count;
        card = card.child(
            message_block(
                reply,
                message_index(index, position + 1),
                Role::Reply,
                actions,
                directory,
                cx,
            )
            .when(closes_card, |block| block.rounded_b(px(CARD_RADIUS))),
        );
    }
    card = card.children(reply_footer(index, open_reply, reply_editor, directory));
    div()
        .w_full()
        .px(px(20.))
        .pt(px(CARD_GAP))
        .child(card)
        .into_any_element()
}
