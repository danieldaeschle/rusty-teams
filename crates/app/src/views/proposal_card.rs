use std::rc::Rc;

use chrono::{DateTime, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{Draft, MentionInput, OBJECT_MARK};

#[cfg(test)]
use crate::agent_api::body::body_blocks;
use crate::agent_api::{ProposalEntry, ProposalPicture, ProposalStatus, agent_label};
use crate::render::render_blocks;
use crate::theme;
use crate::views::attachment_tray::{Classification, OutgoingImage, classify, inline_image};
use crate::views::composer::{Outgoing, ReplyPreview};
use crate::views::widgets::icon;

const CARD_MARGIN_X: f32 = 20.;
const CARD_MARGIN_BOTTOM: f32 = 8.;
const CARD_PADDING: f32 = 12.;
const CARD_RADIUS: f32 = 10.;
const CARD_GAP: f32 = 8.;
const BUTTON_HEIGHT: f32 = 28.;
const BUTTON_RADIUS: f32 = 7.;
const BODY_LINE_HEIGHT: f32 = 20.;
const BODY_MAX_LINES: f32 = 4.;
const QUOTE_BAR_WIDTH: f32 = 2.;
const THUMBNAIL_HEIGHT: f32 = 48.;
const THUMBNAIL_MAX_WIDTH: f32 = 120.;
const THUMBNAIL_MIN_WIDTH: f32 = 16.;
const THUMBNAIL_RADIUS: f32 = 6.;
pub const PROPOSAL_CARD: &str = "shell/islands/main/proposal-card";
pub const PROPOSAL_THUMBNAILS: &str = "shell/islands/main/proposal-card/thumbnails";
pub const PROPOSAL_BUTTONS: &str = "shell/islands/main/proposal-card/buttons";

pub type CardAction = Rc<dyn Fn(&mut Window, &mut App)>;

#[derive(Clone)]
pub struct CardActions {
    pub send: CardAction,
    pub edit: CardAction,
    pub discard: CardAction,
}

fn open_session_available(session_id: &str, cx: &App) -> bool {
    crate::agents::has_session(session_id, cx)
}

fn open_session(session_id: &str, _window: &mut Window, cx: &mut App) {
    crate::agents::open_session(session_id, cx);
}

fn inline_flags(pictures: &[ProposalPicture]) -> Vec<bool> {
    let mut inline_used = 0;
    pictures
        .iter()
        .map(|picture| {
            let inline = matches!(
                classify(&picture.name, &picture.bytes, inline_used),
                Classification::Inline(_)
            );
            if inline {
                inline_used += picture.bytes.len() as u64;
            }
            inline
        })
        .collect()
}

pub fn needs_upload(entry: &ProposalEntry) -> bool {
    inline_flags(&entry.images).contains(&false)
}

pub fn picture_files(entry: &ProposalEntry) -> Vec<(String, Vec<u8>)> {
    entry
        .images
        .iter()
        .map(|picture| (picture.name.clone(), picture.bytes.to_vec()))
        .collect()
}

/// Carries only the images the composer would send inline; `needs_upload` tells when others are left.
pub fn outgoing_for(entry: &ProposalEntry, reply: Option<ReplyPreview>) -> Outgoing {
    let flags = inline_flags(&entry.images);
    let images: Vec<OutgoingImage> = entry
        .images
        .iter()
        .zip(&flags)
        .filter(|(_, inline)| **inline)
        .filter_map(|(picture, _)| inline_image(picture.name.clone(), picture.bytes.to_vec()))
        .map(OutgoingImage::Inline)
        .collect();
    let mut markdown = entry.proposal.text.trim().to_owned();
    for _ in &images {
        markdown.push_str("\n\n");
        markdown.push(OBJECT_MARK);
    }
    Outgoing {
        draft: Draft::from_markdown(&markdown),
        subject: None,
        mentions: entry
            .proposal
            .mentions
            .iter()
            .map(|mention| MentionInput::user(&mention.user_id, &mention.name))
            .collect(),
        reply,
        edit: None,
        images,
        files: Vec::new(),
        link_preview: None,
        importance: Default::default(),
    }
}

fn thumbnail_size(dimensions: Option<(u32, u32)>) -> (f32, f32) {
    let width = dimensions
        .filter(|(width, height)| *width > 0 && *height > 0)
        .map_or(THUMBNAIL_HEIGHT, |(width, height)| {
            THUMBNAIL_HEIGHT * width as f32 / height as f32
        });
    (
        width
            .clamp(THUMBNAIL_HEIGHT / 2., THUMBNAIL_MAX_WIDTH)
            .round(),
        THUMBNAIL_HEIGHT,
    )
}

fn thumbnails(pictures: &[ProposalPicture]) -> Div {
    h_flex()
        .w_full()
        .overflow_hidden()
        .gap(px(CARD_GAP))
        .debug_selector(|| PROPOSAL_THUMBNAILS.into())
        .children(pictures.iter().map(|picture| {
            let thumbnail = picture.thumbnail.as_ref();
            let (width, height) = thumbnail_size(thumbnail.map(|thumbnail| thumbnail.dimensions));
            div()
                .w(px(width))
                .h(px(height))
                .min_w(px(THUMBNAIL_MIN_WIDTH))
                .overflow_hidden()
                .rounded(px(THUMBNAIL_RADIUS))
                .bg(theme::surface_raised())
                .children(thumbnail.map(|thumbnail| {
                    img(ImageSource::from(thumbnail.image.clone()))
                        .size_full()
                        .object_fit(ObjectFit::Cover)
                }))
        }))
}

pub fn age_label(created_at: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let minutes = (now - created_at).num_minutes().max(0);
    match minutes {
        0 => "just now".to_owned(),
        1..=59 => format!("{minutes} min ago"),
        60..=1439 => format!("{} h ago", minutes / 60),
        _ => format!("{} d ago", minutes / 1440),
    }
}

fn card_button(
    id: &'static str,
    label: &'static str,
    background: Hsla,
    color: Hsla,
    action: CardAction,
) -> Stateful<Div> {
    div()
        .id(id)
        .h(px(BUTTON_HEIGHT))
        .px(px(12.))
        .flex()
        .items_center()
        .rounded(px(BUTTON_RADIUS))
        .cursor_pointer()
        .text_size(px(12.))
        .font_weight(FontWeight::SEMIBOLD)
        .bg(background)
        .text_color(color)
        .hover(|button| button.opacity(0.85))
        .child(label)
        .on_click(move |_, window, cx| action(window, cx))
}

fn header_text(entry: &ProposalEntry, now: DateTime<Utc>) -> (String, Hsla, Option<String>) {
    let agent = agent_label(entry.proposal.agent.as_deref());
    match &entry.status {
        ProposalStatus::Pending => (
            format!("{agent} proposes a reply"),
            theme::accent_text(),
            Some(format!(" - {}", age_label(entry.proposal.created_at, now))),
        ),
        ProposalStatus::Stale { answered_at } => (
            format!(
                "{agent}'s reply is outdated - you answered at {}",
                answered_at.format("%H:%M")
            ),
            theme::text_muted(),
            None,
        ),
        ProposalStatus::Failed { reason } => (
            format!("Could not send - {reason}"),
            theme::red_soft(),
            None,
        ),
    }
}

pub fn render(
    entry: &ProposalEntry,
    quote: Option<String>,
    actions: CardActions,
    now: DateTime<Utc>,
    cx: &App,
) -> Div {
    let stale = matches!(entry.status, ProposalStatus::Stale { .. });
    let failed = matches!(entry.status, ProposalStatus::Failed { .. });
    let (title, title_color, age) = header_text(entry, now);
    let close = actions.discard.clone();
    let header = h_flex()
        .w_full()
        .items_center()
        .gap(px(4.))
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .items_baseline()
                .overflow_hidden()
                .text_size(px(12.))
                .child(
                    div()
                        .truncate()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(title_color)
                        .child(title),
                )
                .children(
                    age.map(|age| div().flex_none().text_color(theme::text_faint()).child(age)),
                ),
        )
        .child(
            div()
                .id("proposal-close")
                .size(px(BUTTON_HEIGHT))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(BUTTON_RADIUS))
                .cursor_pointer()
                .hover(|button| button.bg(theme::surface_raised()))
                .child(icon(IconName::Close, 14., theme::text_muted()))
                .on_click(move |_, window, cx| close(window, cx)),
        );
    let quote = quote.map(|quote| {
        div()
            .w_full()
            .truncate()
            .pl(px(8.))
            .border_l(px(QUOTE_BAR_WIDTH))
            .border_color(theme::border_strong())
            .text_size(px(12.))
            .text_color(theme::text_muted())
            .child(quote)
    });
    let labels = entry.mention_labels.clone();
    let body = div()
        .id("proposal-text")
        .w_full()
        .max_h(px(BODY_LINE_HEIGHT * BODY_MAX_LINES))
        .overflow_hidden()
        .text_size(px(14.))
        .line_height(px(BODY_LINE_HEIGHT))
        .when(stale, |body| body.opacity(0.6))
        .child(render_blocks(&entry.body, "proposal-body", false, cx))
        .when(!labels.is_empty(), |body| {
            body.tooltip(move |window, cx| {
                Tooltip::new(format!("Mentions: {}", labels.join(", "))).build(window, cx)
            })
        });
    let images = (!entry.images.is_empty()).then(|| thumbnails(&entry.images));
    let session = entry
        .proposal
        .session_id
        .clone()
        .filter(|session_id| open_session_available(session_id, cx));
    let buttons = h_flex()
        .w_full()
        .items_center()
        .gap(px(8.))
        .debug_selector(|| PROPOSAL_BUTTONS.into())
        .when(!stale, |row| {
            row.child(card_button(
                "proposal-send",
                if failed { "Retry" } else { "Send" },
                theme::accent(),
                theme::on_accent(),
                actions.send.clone(),
            ))
        })
        .child(card_button(
            "proposal-edit",
            "Edit",
            theme::surface_raised(),
            theme::text(),
            actions.edit.clone(),
        ))
        .child(card_button(
            "proposal-discard",
            "Discard",
            theme::surface_raised(),
            theme::text(),
            actions.discard.clone(),
        ))
        .children(session.map(|session_id| {
            div()
                .id("proposal-open-session")
                .ml_auto()
                .cursor_pointer()
                .text_size(px(12.))
                .text_color(theme::accent_text())
                .child("Open session")
                .on_click(move |_, window, cx| open_session(&session_id, window, cx))
        }));
    v_flex()
        .flex_none()
        .mx(px(CARD_MARGIN_X))
        .mb(px(CARD_MARGIN_BOTTOM))
        .p(px(CARD_PADDING))
        .gap(px(CARD_GAP))
        .rounded(px(CARD_RADIUS))
        .bg(theme::row_hover())
        .debug_selector(|| PROPOSAL_CARD.into())
        .child(header)
        .children(quote)
        .child(body)
        .children(images)
        .child(buttons)
}

#[cfg(test)]
pub fn sample_png(side: u32, padded_to: usize) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::RgbaImage::new(side, side)
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .unwrap();
    if bytes.len() < padded_to {
        bytes.resize(padded_to, 0);
    }
    bytes
}

#[cfg(test)]
pub fn sample_entry(text: &str, mentions: &[(&str, &str)], images: &[Vec<u8>]) -> ProposalEntry {
    ProposalEntry {
        body: std::sync::Arc::new(body_blocks(text, &[])),
        mention_labels: Vec::new(),
        proposal: mcp::Proposal {
            id: "p1".to_owned(),
            conversation_id: "c1".to_owned(),
            reply_to_message_id: None,
            text: text.to_owned(),
            mentions: mentions
                .iter()
                .map(|(user_id, name)| mcp::ProposalMention {
                    user_id: (*user_id).to_owned(),
                    name: (*name).to_owned(),
                })
                .collect(),
            images: Vec::new(),
            agent: None,
            session_id: None,
            created_at: Utc::now(),
        },
        images: images
            .iter()
            .enumerate()
            .map(|(index, bytes)| ProposalPicture {
                name: format!("image-{index}.png"),
                bytes: std::sync::Arc::new(bytes.clone()),
                thumbnail: None,
            })
            .collect(),
        status: crate::agent_api::ProposalStatus::Pending,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Duration, Utc};
    use teams_core::MentionInput;

    use super::{
        age_label, needs_upload, outgoing_for, picture_files, sample_entry,
        sample_png, thumbnail_size,
    };
    use crate::views::attachment_tray::{AttachmentTray, INLINE_IMAGE_MAX_BYTES, OutgoingImage};

    const TWO_MEGABYTES: usize = 2 * 1024 * 1024;

    #[test]
    fn age_labels_follow_the_elapsed_time() {
        let now = Utc::now();
        let label = |minutes: i64| age_label(now - Duration::minutes(minutes), now);
        assert_eq!(label(0), "just now");
        assert_eq!(label(1), "1 min ago");
        assert_eq!(label(59), "59 min ago");
        assert_eq!(label(120), "2 h ago");
        assert_eq!(label(3000), "2 d ago");
        assert_eq!(age_label(now + Duration::minutes(5), now), "just now");
    }

    #[test]
    fn thumbnails_keep_the_height_and_clamp_the_width() {
        assert_eq!(thumbnail_size(Some((100, 100))), (48., 48.));
        assert_eq!(thumbnail_size(Some((1000, 100))), (120., 48.));
        assert_eq!(thumbnail_size(Some((10, 100))), (24., 48.));
        assert_eq!(thumbnail_size(None), (48., 48.));
    }

    #[test]
    fn send_hands_images_and_mentions_to_the_outgoing_message() {
        let entry = sample_entry(
            "Thanks @Ada Example and @Bo",
            &[("u1", "Ada Example"), ("u2", "Bo")],
            &[sample_png(2, 0), sample_png(3, 0)],
        );
        assert!(!needs_upload(&entry));
        let outgoing = outgoing_for(&entry, None);
        assert_eq!(
            outgoing.mentions,
            [
                MentionInput::user("u1", "Ada Example"),
                MentionInput::user("u2", "Bo")
            ]
        );
        assert_eq!(outgoing.images.len(), 2);
        assert_eq!(outgoing.extras().images.len(), 2);
        assert!(outgoing.html().starts_with("Thanks @Ada Example and @Bo"));
        assert_eq!(
            outgoing
                .html()
                .matches("<img src=\"../hostedContents/")
                .count(),
            2
        );
        assert_eq!(outgoing.text().trim(), "Thanks @Ada Example and @Bo");
    }

    #[test]
    fn a_proposal_without_extras_stays_plain_text() {
        let outgoing = outgoing_for(&sample_entry("hello", &[], &[]), None);
        assert!(outgoing.mentions.is_empty());
        assert!(outgoing.images.is_empty());
        assert_eq!(outgoing.html(), "hello");
    }

    #[test]
    fn images_over_the_inline_limits_are_left_for_upload() {
        let big = || sample_png(2, TWO_MEGABYTES);
        let entry = sample_entry("two big ones", &[], &[big(), big()]);
        assert!(needs_upload(&entry));
        let outgoing = outgoing_for(&entry, None);
        assert!(outgoing.images.is_empty());
        assert_eq!(picture_files(&entry).len(), 2);

        let mixed = sample_entry("mixed", &[], &[sample_png(2, 0), big(), sample_png(3, 0)]);
        assert!(needs_upload(&mixed));
        assert_eq!(outgoing_for(&mixed, None).images.len(), 2);

        let over_one = sample_entry(
            "limit",
            &[],
            &[sample_png(2, INLINE_IMAGE_MAX_BYTES as usize + 1)],
        );
        assert!(needs_upload(&over_one));
        let at_limit = sample_entry(
            "limit",
            &[],
            &[sample_png(2, INLINE_IMAGE_MAX_BYTES as usize)],
        );
        assert!(!needs_upload(&at_limit));
    }

    #[test]
    fn the_total_inline_budget_counts_across_images() {
        let almost_one_megabyte = || sample_png(2, 1024 * 1024);
        let entry = sample_entry(
            "three",
            &[],
            &[
                almost_one_megabyte(),
                almost_one_megabyte(),
                almost_one_megabyte(),
            ],
        );
        assert!(needs_upload(&entry));
        assert_eq!(outgoing_for(&entry, None).images.len(), 2);
    }

    #[test]
    fn inline_images_go_back_into_the_tray_in_order() {
        let entry = sample_entry("look", &[], &[sample_png(2, 0), sample_png(3, 0)]);
        let outgoing = outgoing_for(&entry, None);
        let mut tray = AttachmentTray::default();
        let ids = tray.restore(&outgoing.images, &outgoing.files);
        assert_eq!(ids.len(), 2);
        let restored = tray.images_in(&ids);
        assert_eq!(restored, outgoing.images);
        assert!(matches!(restored[0], OutgoingImage::Inline(_)));
        assert_eq!(
            outgoing
                .draft
                .text()
                .matches(teams_core::OBJECT_MARK)
                .count(),
            2
        );
    }
}
