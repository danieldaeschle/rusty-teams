use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::team_avatar_key;

use super::avatar::{person_avatar, team_avatar};
use super::widgets::icon;
use crate::app_state::AppState;
use crate::data::Directory;
use crate::manage_team::{MembersLoad, TeamMember, member_groups};
use crate::theme;
use app_ui_kit::scroll_edge::with_top_edge;

const MANAGE_TEAM_TAG: &str = "shell/islands/main/manage-team";
const MANAGE_TEAM_SCROLL_DIVIDER: &str = "shell/islands/main/manage-team-divider-scroll";
const HEADER_HEIGHT: f32 = 60.;
const PAGE_PADDING: f32 = 20.;
const TEAM_AVATAR_SIZE: f32 = 32.;
const TEAM_AVATAR_RADIUS: f32 = 8.;
const MEMBER_AVATAR_SIZE: f32 = 32.;
const ROW_HEIGHT: f32 = 52.;
const SEARCH_WIDTH: f32 = 260.;
const SEARCH_HEIGHT: f32 = 32.;
const ROLE_WIDTH: f32 = 96.;
const LIST_MAX_WIDTH: f32 = 960.;
const SEARCH_PLACEHOLDER: &str = "Search for members";

pub struct ManageTeamPage {
    app: Entity<AppState>,
    search: Entity<InputState>,
    owners_open: bool,
    others_open: bool,
    scroll: ScrollHandle,
    _subscriptions: [Subscription; 2],
}

impl ManageTeamPage {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(SEARCH_PLACEHOLDER));
        let subscriptions = [
            cx.observe(&app, |_, _, cx| cx.notify()),
            cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        ManageTeamPage {
            app,
            search,
            owners_open: true,
            others_open: true,
            scroll: ScrollHandle::new(),
            _subscriptions: subscriptions,
        }
    }

    fn header(&self, team_id: &str, team_name: &str, directory: &Directory, cx: &Context<Self>) -> Div {
        let photo = directory.avatar(&team_avatar_key(team_id));
        h_flex()
            .h(px(HEADER_HEIGHT))
            .flex_none()
            .px(px(PAGE_PADDING))
            .gap(px(12.))
            .items_center()
            .child(
                div()
                    .id("manage-team-back")
                    .size(px(28.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|button| button.bg(theme::row_hover()))
                    .child(icon(IconName::ArrowLeft, 16., theme::text_muted()))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.app.update(cx, |state, cx| state.close_manage_team(cx));
                    })),
            )
            .child(team_avatar(photo, team_name, team_id, TEAM_AVATAR_SIZE, TEAM_AVATAR_RADIUS))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .text_size(px(16.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme::text())
                    .child(team_name.to_owned()),
            )
            .child(
                div()
                    .ml(px(12.))
                    .h_full()
                    .flex()
                    .items_center()
                    .border_b_2()
                    .border_color(theme::accent())
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text())
                    .child("Members"),
            )
            .child(div().flex_1())
            .child(
                h_flex()
                    .id("manage-team-in-browser")
                    .h(px(28.))
                    .px(px(10.))
                    .gap(px(6.))
                    .items_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .text_size(px(13.))
                    .text_color(theme::text_soft())
                    .hover(|button| button.bg(theme::row_hover()))
                    .child(icon(IconName::ExternalLink, 14., theme::text_muted()))
                    .child("Open in browser")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.app.update(cx, |state, cx| {
                            if let Some(team_id) = state.manage_team.as_ref().map(|open| open.team_id.clone()) {
                                state.manage_team_in_browser(&team_id, cx);
                            }
                        });
                    })),
            )
    }

    fn search_box(&self) -> Div {
        h_flex()
            .w(px(SEARCH_WIDTH))
            .max_w_full()
            .h(px(SEARCH_HEIGHT))
            .px(px(10.))
            .gap(px(8.))
            .items_center()
            .rounded(px(6.))
            .bg(theme::popup())
            .child(icon(IconName::Search, 14., theme::text_muted()))
            .child(div().flex_1().min_w_0().child(Input::new(&self.search).appearance(false).bordered(false)))
    }

    fn section(
        &self,
        id: &'static str,
        title: &str,
        members: &[&TeamMember],
        open: bool,
        directory: &Directory,
        cx: &Context<Self>,
    ) -> Div {
        let toggle = h_flex()
            .id(id)
            .h(px(32.))
            .flex_none()
            .gap(px(6.))
            .items_center()
            .cursor_pointer()
            .text_size(px(14.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::text())
            .child(icon(if open { IconName::ChevronDown } else { IconName::ChevronRight }, 14., theme::text_muted()))
            .child(format!("{title} ({})", members.len()))
            .on_click(cx.listener(move |this, _, _, cx| {
                if id == "manage-team-owners" {
                    this.owners_open = !this.owners_open;
                } else {
                    this.others_open = !this.others_open;
                }
                cx.notify();
            }));
        v_flex()
            .w_full()
            .child(toggle)
            .when(open, |section| {
                section.children(members.iter().map(|member| member_row(member, directory)))
            })
    }
}

fn member_row(member: &TeamMember, directory: &Directory) -> Div {
    h_flex()
        .w_full()
        .h(px(ROW_HEIGHT))
        .flex_none()
        .px(px(8.))
        .gap(px(12.))
        .items_center()
        .rounded(px(8.))
        .hover(|row| row.bg(theme::row_hover()))
        .child(
            div()
                .flex_none()
                .size(px(MEMBER_AVATAR_SIZE))
                .child(person_avatar(directory, member.user_id.as_deref(), &member.name, MEMBER_AVATAR_SIZE)),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w_0()
                .child(div().truncate().text_size(px(14.)).text_color(theme::text()).child(member.name.clone()))
                .children(member.email.clone().map(|email| {
                    div().truncate().text_size(px(12.)).text_color(theme::text_muted()).child(email)
                })),
        )
        .child(
            div()
                .flex_none()
                .w(px(ROLE_WIDTH))
                .text_size(px(13.))
                .text_color(theme::text_soft())
                .child(if member.owner { "Owner" } else { "Member" }),
        )
}

fn notice(text: String) -> Div {
    div()
        .py(px(32.))
        .flex()
        .justify_center()
        .text_size(px(13.))
        .text_color(theme::text_muted())
        .child(text)
}

impl Render for ManageTeamPage {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).value().to_string();
        let state = self.app.read(cx);
        let Some(manage_team) = state.manage_team.as_ref() else {
            return div().into_any_element();
        };
        let directory = &state.directory;
        let body: Vec<AnyElement> = match &manage_team.members {
            MembersLoad::Loading => vec![notice("Loading members...".to_owned()).into_any_element()],
            MembersLoad::Failed(message) => vec![notice(message.clone()).into_any_element()],
            MembersLoad::Loaded(members) => {
                let groups = member_groups(members, &query);
                if groups.owners.is_empty() && groups.others.is_empty() {
                    vec![notice(format!("No members match \"{}\"", query.trim())).into_any_element()]
                } else {
                    vec![
                        self.section("manage-team-owners", "Owners", &groups.owners, self.owners_open, directory, cx)
                            .into_any_element(),
                        self.section("manage-team-others", "Members and guests", &groups.others, self.others_open, directory, cx)
                            .into_any_element(),
                    ]
                }
            }
        };
        v_flex()
            .debug_selector(|| MANAGE_TEAM_TAG.into())
            .flex_1()
            .w_full()
            .min_w_0()
            .h_full()
            .rounded(px(10.))
            .bg(theme::surface())
            .overflow_hidden()
            .child(self.header(&manage_team.team_id, &manage_team.team_name, directory, cx))
            .child(div().w_full().h(px(1.)).flex_none().bg(theme::border()))
            .child(with_top_edge(
                v_flex()
                    .id("manage-team-list")
                    .flex_1()
                    .min_h_0()
                    .px(px(PAGE_PADDING - 8.))
                    .py(px(12.))
                    .overflow_y_scrollbar()
                    .track_scroll(&self.scroll)
                    .child(
                        v_flex()
                            .w_full()
                            .max_w(px(LIST_MAX_WIDTH))
                            .gap(px(12.))
                            .child(h_flex().w_full().px(px(8.)).justify_end().child(self.search_box()))
                            .children(body),
                    ),
                &self.scroll,
                MANAGE_TEAM_SCROLL_DIVIDER,
            ))
            .into_any_element()
    }
}
