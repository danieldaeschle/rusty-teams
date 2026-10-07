use std::collections::HashSet;

use chrono::{Local, Offset, Utc};
use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenuItem};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use store::{ChannelRecord, SidebarTeam, TeamRecord};

use super::avatar::{spec_avatar, square_avatar, with_presence};
use super::widgets::{count_badge, dot, icon, unread_marker};
use crate::app_state::{AppEvent, AppState, Selection};
use crate::data::{Directory, FolderKind};
use crate::format;
use crate::sidebar_model::{
    AvatarSpec, ChatItem, DELETED_PREVIEW, EMPTY_FOLDER_HINT, Preview, Section, SectionInput,
    SectionKind, any_unread_channel, build_sections, unread_chat_count,
};
use crate::theme;

pub const SIDEBAR_WIDTH: f32 = 312.;
const ROW_HEIGHT: f32 = 54.;
const PINNED_CHANNEL_HEIGHT: f32 = 48.;
const SECTION_HEIGHT: f32 = 30.;
const SECTION_GAP: f32 = 4.;
const HINT_HEIGHT: f32 = 26.;
const FALLBACK_VIEWPORT: f32 = 900.;
const VIEWPORT_MARGIN: f32 = 300.;
const AVATAR_SIZE: f32 = 36.;
const PINNED_LABEL: &str = "Pinned";
const TEAMS_LABEL: &str = "Teams";
const HIDDEN_TEAMS_LABEL: &str = "Hidden teams";
const TEAM_ROW_HEIGHT: f32 = 40.;
const REVEAL_ROW_HEIGHT: f32 = 32.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SidebarTab {
    Chats,
    Channels,
}

#[derive(Clone)]
struct DraggedChat {
    id: String,
    title: String,
}

struct DragGhost {
    title: String,
}

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .px(px(12.))
            .py(px(6.))
            .rounded(px(8.))
            .bg(theme::surface_raised())
            .border_1()
            .border_color(theme::accent())
            .text_size(px(13.))
            .text_color(theme::text())
            .child(self.title.clone())
    }
}

#[derive(Clone)]
struct MenuContext {
    state: Entity<AppState>,
    folders: Vec<(String, String)>,
    favorites_id: Option<String>,
}

pub struct SidebarView {
    state: Entity<AppState>,
    tab: SidebarTab,
    expanded_teams: HashSet<String>,
    revealed_channel_teams: HashSet<String>,
    hidden_teams_open: bool,
    chats_scroll: ScrollHandle,
    channels_scroll: ScrollHandle,
    _subscription: Subscription,
}

fn preview_text(preview: &Preview) -> Option<(String, bool)> {
    match preview {
        Preview::Empty => None,
        Preview::Deleted => Some((DELETED_PREVIEW.to_owned(), true)),
        Preview::Text {
            prefix: Some(prefix),
            text,
        } => Some((format!("{prefix}: {text}"), false)),
        Preview::Text { prefix: None, text } => Some((text.clone(), false)),
    }
}

fn preview_icon(text: &str) -> Option<IconName> {
    match text.rsplit(": ").next()? {
        "Image" => Some(IconName::Image),
        "File" => Some(IconName::File),
        _ => None,
    }
}

impl SidebarView {
    pub fn new(state: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let subscription = cx.subscribe(&state, |_, _, event: &AppEvent, cx| {
            if matches!(
                event,
                AppEvent::Sidebar | AppEvent::Selection | AppEvent::Directory
            ) {
                cx.notify();
            }
        });
        let tab = if state.read(cx).start_on_channels {
            SidebarTab::Channels
        } else {
            SidebarTab::Chats
        };
        SidebarView {
            state,
            tab,
            expanded_teams: HashSet::new(),
            revealed_channel_teams: HashSet::new(),
            hidden_teams_open: false,
            chats_scroll: ScrollHandle::new(),
            channels_scroll: ScrollHandle::new(),
            _subscription: subscription,
        }
    }

    fn tab_button(
        &self,
        tab: SidebarTab,
        label: &str,
        marker: Option<AnyElement>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let active = self.tab == tab;
        h_flex()
            .id(SharedString::from(format!("tab-{label}")))
            .flex_1()
            .h(px(28.))
            .rounded(px(6.))
            .items_center()
            .justify_center()
            .gap(px(6.))
            .cursor_pointer()
            .text_size(px(13.))
            .when(active, |tab| {
                tab.bg(theme::surface_raised())
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text())
            })
            .when(!active, |tab| {
                tab.text_color(theme::text_muted())
                    .hover(|tab| tab.text_color(theme::text()))
            })
            .child(label.to_owned())
            .children(marker)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.tab = tab;
                cx.notify();
            }))
    }

    fn tab_bar(
        &self,
        unread_chats: u32,
        channel_dot: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let chat_marker =
            (unread_chats > 0).then(|| count_badge(unread_chats, false).into_any_element());
        let channel_marker = channel_dot.then(|| dot(7.).into_any_element());
        div().w_full().px(px(12.)).pt(px(12.)).pb(px(8.)).child(
            h_flex()
                .w_full()
                .gap(px(4.))
                .p(px(3.))
                .rounded(px(8.))
                .bg(theme::background())
                .border_1()
                .border_color(theme::border())
                .child(self.tab_button(SidebarTab::Chats, "Chats", chat_marker, cx))
                .child(self.tab_button(SidebarTab::Channels, "Channels", channel_marker, cx)),
        )
    }

    fn section_header(&self, section: &Section, cx: &mut Context<Self>) -> impl IntoElement {
        let drop_target = (section.kind != SectionKind::Others).then(|| section.id.clone());
        let toggle_id = section.id.clone();
        h_flex()
            .id(SharedString::from(format!("section-{}", section.id)))
            .h(px(SECTION_HEIGHT))
            .mt(px(SECTION_GAP))
            .mx(px(4.))
            .pl(px(8.))
            .pr(px(10.))
            .gap(px(6.))
            .items_center()
            .rounded(px(6.))
            .cursor_pointer()
            .hover(|header| header.bg(theme::row_hover()))
            .drag_over::<DraggedChat>(|style, _, _, _| {
                style
                    .bg(theme::drop_background())
                    .border_1()
                    .border_dashed()
                    .border_color(theme::accent())
            })
            .on_drop(cx.listener(move |this, dragged: &DraggedChat, _, cx| {
                let target = drop_target.clone();
                this.state.update(cx, |state, cx| {
                    state.move_chat(&dragged.id, target.as_deref(), cx)
                });
            }))
            .child(icon(
                if section.collapsed {
                    IconName::ChevronRight
                } else {
                    IconName::ChevronDown
                },
                12.,
                theme::text_muted(),
            ))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_soft())
                    .child(section.name.clone()),
            )
            .when(section.collapsed && section.unread_chats > 0, |header| {
                header.child(count_badge(section.unread_chats, false))
            })
            .child(
                div()
                    .text_size(px(11.))
                    .text_color(theme::text_faint())
                    .child(section.count.to_string()),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                let id = toggle_id.clone();
                this.state
                    .update(cx, |state, cx| state.toggle_collapsed(&id, cx));
            }))
    }

    fn chat_row(
        &self,
        item: &ChatItem,
        selected: bool,
        directory: &Directory,
        menu: &MenuContext,
    ) -> AnyElement {
        let ring = if selected {
            theme::surface_raised()
        } else {
            theme::surface()
        };
        let unread = item.unread.is_unread();
        let presence = item
            .presence_user
            .as_deref()
            .map(|user_id| directory.presence_of(user_id));
        let avatar = spec_avatar(directory, &item.avatar, AVATAR_SIZE, ring);
        let avatar = match presence {
            Some(kind) => with_presence(avatar, kind, AVATAR_SIZE, ring).into_any_element(),
            None => avatar,
        };
        let name = div()
            .flex_1()
            .min_w_0()
            .truncate()
            .text_size(px(14.))
            .font_weight(if unread {
                FontWeight::BOLD
            } else {
                FontWeight::NORMAL
            })
            .text_color(theme::text())
            .child(item.title.clone());
        let time = div()
            .flex_none()
            .text_size(px(11.))
            .text_color(if unread {
                theme::accent_text()
            } else {
                theme::text_muted()
            })
            .child(item.time_label.clone());
        let preview = preview_text(&item.preview).map(|(text, italic)| {
            let glyph = preview_icon(&text);
            h_flex()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .gap(px(4.))
                .items_center()
                .text_size(px(12.5))
                .when(italic, |preview| preview.italic())
                .text_color(if unread {
                    theme::text_strong()
                } else {
                    theme::text_muted()
                })
                .children(glyph.map(|name| {
                    icon(
                        name,
                        12.,
                        if unread {
                            theme::text_strong()
                        } else {
                            theme::text_muted()
                        },
                    )
                }))
                .child(div().truncate().child(text))
        });
        let lines = v_flex()
            .flex_1()
            .min_w_0()
            .gap(px(2.))
            .child(
                h_flex()
                    .gap(px(8.))
                    .items_baseline()
                    .child(name)
                    .child(time),
            )
            .child(
                h_flex()
                    .h(px(18.))
                    .gap(px(8.))
                    .items_center()
                    .child(preview.unwrap_or_else(|| div().flex_1()))
                    .children(unread_marker(item.unread)),
            );
        let state = self.state.clone();
        let chat_id = item.id.clone();
        let dragged = DraggedChat {
            id: item.id.clone(),
            title: item.title.clone(),
        };
        let pinned = menu.favorites_id.is_some() && item.folder_id == menu.favorites_id;
        let menu_context = menu.clone();
        let menu_chat_id = item.id.clone();
        h_flex()
            .id(SharedString::from(format!("chat-{}", item.id)))
            .mx(px(8.))
            .h(px(ROW_HEIGHT))
            .px(px(8.))
            .gap(px(10.))
            .items_center()
            .rounded(px(8.))
            .cursor_pointer()
            .when(selected, |row| row.bg(theme::surface_raised()))
            .when(!selected, |row| row.hover(|row| row.bg(theme::row_hover())))
            .on_drag(dragged, |dragged, _, _, cx| {
                let title = dragged.title.clone();
                cx.new(|_| DragGhost { title })
            })
            .child(avatar)
            .child(lines)
            .on_click(move |_, _, cx| {
                let selection = Selection::Chat(chat_id.clone());
                state.update(cx, |state, cx| state.select(selection, cx));
            })
            .context_menu(move |popup, window, cx| {
                let context = menu_context.clone();
                let chat_id = menu_chat_id.clone();
                let pin_state = context.state.clone();
                let pin_id = chat_id.clone();
                let mut popup = popup.item(
                    PopupMenuItem::new(if pinned { "Unpin" } else { "Pin" }).on_click(
                        move |_, _, cx| {
                            pin_state.update(cx, |state, cx| {
                                if pinned {
                                    state.move_chat(&pin_id, None, cx);
                                } else {
                                    state.pin_chat(&pin_id, cx);
                                }
                            });
                        },
                    ),
                );
                if !context.folders.is_empty() {
                    let folders = context.folders.clone();
                    let move_state = context.state.clone();
                    let move_id = chat_id.clone();
                    popup = popup.submenu("Move to folder", window, cx, move |submenu, _, _| {
                        folders.iter().fold(submenu, |submenu, (folder_id, name)| {
                            let (state, chat_id, folder_id) =
                                (move_state.clone(), move_id.clone(), folder_id.clone());
                            submenu.item(PopupMenuItem::new(name.clone()).on_click(
                                move |_, _, cx| {
                                    state.update(cx, |state, cx| {
                                        state.move_chat(&chat_id, Some(&folder_id), cx)
                                    });
                                },
                            ))
                        })
                    });
                }
                let read_state = context.state.clone();
                popup.item(
                    PopupMenuItem::new("Mark as read")
                        .disabled(!unread)
                        .on_click(move |_, _, cx| {
                            read_state.update(cx, |state, cx| state.mark_chat_read(&chat_id, cx));
                        }),
                )
            })
            .into_any_element()
    }

    fn chats_body(&self, window_height: f32, cx: &mut Context<Self>) -> AnyElement {
        let (sections, menu) = {
            let state = self.state.read(cx);
            let now = Utc::now();
            let offset = Local::now().offset().fix();
            let input = SectionInput {
                chats: &state.sidebar.chats,
                directory: &state.directory,
                collapsed: &state.collapsed,
                now,
                offset,
            };
            let sections = build_sections(&input);
            let menu = MenuContext {
                state: self.state.clone(),
                folders: state
                    .directory
                    .folders
                    .iter()
                    .filter(|folder| folder.kind == FolderKind::UserCreated)
                    .map(|folder| (folder.id.clone(), folder.name.clone()))
                    .collect(),
                favorites_id: state.directory.favorites().map(|folder| folder.id.clone()),
            };
            (sections, menu)
        };

        let top = -f32::from(self.chats_scroll.offset().y);
        let measured: f32 = f32::from(self.chats_scroll.bounds().size.height);
        let height = if measured > 1. {
            measured
        } else {
            window_height.max(FALLBACK_VIEWPORT)
        };
        let (visible_from, visible_to) = (top - VIEWPORT_MARGIN, top + height + VIEWPORT_MARGIN);

        let selected = self.state.read(cx).selection.clone();
        let mut faces: Vec<String> = Vec::new();
        let mut presence_users: Vec<String> = Vec::new();
        let mut y = 0.;
        let mut list = v_flex().w_full().pb(px(12.));
        let mut skipped_height = 0.;
        let directory_state = self.state.clone();
        for section in &sections {
            list = with_spacer(list, &mut skipped_height);
            list = list.child(self.section_header(section, cx));
            y += SECTION_GAP + SECTION_HEIGHT;
            if section.collapsed {
                continue;
            }
            for item in &section.items {
                let on_screen = y + ROW_HEIGHT >= visible_from && y <= visible_to;
                if on_screen {
                    collect_faces(&item.avatar, &mut faces);
                    presence_users.extend(item.presence_user.clone());
                }
                if on_screen {
                    list = with_spacer(list, &mut skipped_height);
                    let is_selected = selected == Some(Selection::Chat(item.id.clone()));
                    let directory = &directory_state.read(cx).directory;
                    list = list.child(self.chat_row(item, is_selected, directory, &menu));
                } else {
                    skipped_height += ROW_HEIGHT;
                }
                y += ROW_HEIGHT;
            }
            if section.show_empty_hint {
                list = with_spacer(list, &mut skipped_height);
                list = list.child(
                    div()
                        .ml(px(44.))
                        .mr(px(20.))
                        .h(px(HINT_HEIGHT))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .italic()
                        .text_color(theme::text_faint())
                        .child(EMPTY_FOLDER_HINT),
                );
                y += HINT_HEIGHT;
            }
        }
        list = with_spacer(list, &mut skipped_height);
        self.request_lazy_data(faces, presence_users, cx);
        div()
            .id("chats-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.chats_scroll)
            .child(list)
            .into_any_element()
    }

    fn request_lazy_data(
        &self,
        faces: Vec<String>,
        presence_users: Vec<String>,
        cx: &mut Context<Self>,
    ) {
        if faces.is_empty() && presence_users.is_empty() {
            return;
        }
        let state = self.state.clone();
        cx.defer(move |cx| {
            state.update(cx, |state, cx| {
                state.request_avatars(faces, cx);
                state.request_presence(presence_users, cx);
            });
        });
    }

    fn channel_row(
        &self,
        channel_id: &str,
        team: &TeamRecord,
        channel: &ChannelRecord,
        selected: bool,
    ) -> AnyElement {
        let unread = channel.unread;
        let state = self.state.clone();
        let selection = Selection::Channel(channel_id.to_owned());
        let now = Utc::now();
        let offset = Local::now().offset().fix();
        let time = channel
            .last_message_at
            .map(|time| {
                format::list_time_label(time, now.with_timezone(&offset).date_naive(), offset)
            })
            .unwrap_or_default();
        h_flex()
            .id(SharedString::from(format!("pinned-{channel_id}")))
            .mx(px(8.))
            .h(px(PINNED_CHANNEL_HEIGHT))
            .px(px(8.))
            .gap(px(10.))
            .items_center()
            .rounded(px(8.))
            .cursor_pointer()
            .when(selected, |row| row.bg(theme::surface_raised()))
            .when(!selected, |row| row.hover(|row| row.bg(theme::row_hover())))
            .child(square_avatar(&team.name, &team.id, 28., 7.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(1.))
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_baseline()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(13.5))
                                    .font_weight(if unread {
                                        FontWeight::BOLD
                                    } else {
                                        FontWeight::NORMAL
                                    })
                                    .text_color(theme::text())
                                    .child(format!("{} / {}", team.name, channel.name)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(if unread {
                                        theme::accent_text()
                                    } else {
                                        theme::text_muted()
                                    })
                                    .child(time),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap(px(8.))
                            .items_center()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .text_size(px(12.))
                                    .text_color(theme::text_muted())
                                    .child(team.name.clone()),
                            )
                            .when(unread, |line| line.child(dot(7.))),
                    ),
            )
            .on_click(move |_, _, cx| {
                let selection = selection.clone();
                state.update(cx, |state, cx| state.select(selection, cx));
            })
            .into_any_element()
    }

    fn plain_header(&self, label: &str) -> impl IntoElement {
        div()
            .h(px(SECTION_HEIGHT))
            .mt(px(SECTION_GAP))
            .px(px(12.))
            .flex()
            .items_center()
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::text_soft())
            .child(label.to_owned())
    }

    fn team_block(
        &self,
        mut list: Div,
        entry: &SidebarTeam,
        selected: Option<&Selection>,
        cx: &mut Context<Self>,
    ) -> Div {
        let team_id = entry.team.id.clone();
        let expanded = team_holds(entry, selected) || self.expanded_teams.contains(&team_id);
        let unread = entry.channels.iter().any(|channel| channel.unread);
        let toggle_id = team_id.clone();
        list = list.child(
            h_flex()
                .id(SharedString::from(format!("team-{team_id}")))
                .mx(px(8.))
                .h(px(TEAM_ROW_HEIGHT))
                .px(px(8.))
                .gap(px(8.))
                .items_center()
                .rounded(px(8.))
                .cursor_pointer()
                .hover(|row| row.bg(theme::row_hover()))
                .child(icon(
                    if expanded {
                        IconName::ChevronDown
                    } else {
                        IconName::ChevronRight
                    },
                    14.,
                    theme::text_muted(),
                ))
                .child(square_avatar(&entry.team.name, &entry.team.id, 28., 7.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(13.5))
                        .font_weight(if unread || expanded {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::NORMAL
                        })
                        .text_color(theme::text())
                        .child(entry.team.name.clone()),
                )
                .when(unread && !expanded, |row| row.child(dot(7.)))
                .on_click(cx.listener(move |this, _, _, cx| {
                    if !this.expanded_teams.remove(&toggle_id) {
                        this.expanded_teams.insert(toggle_id.clone());
                    }
                    cx.notify();
                })),
        );
        if !expanded {
            return list;
        }
        let is_hidden = |channel: &ChannelRecord| entry.hidden_channel_ids.contains(&channel.id);
        let is_selected =
            |channel: &ChannelRecord| selected == Some(&Selection::Channel(channel.id.clone()));
        for channel in entry
            .channels
            .iter()
            .filter(|channel| !is_hidden(channel) || is_selected(channel))
        {
            list = list.child(self.team_channel_row(channel, is_selected(channel)));
        }
        let hidden: Vec<&ChannelRecord> = entry
            .channels
            .iter()
            .filter(|channel| is_hidden(channel) && !is_selected(channel))
            .collect();
        if hidden.is_empty() {
            return list;
        }
        let revealed = self.revealed_channel_teams.contains(&team_id);
        let toggle_id = team_id.clone();
        list = list.child(self.reveal_row(
            SharedString::from(format!("hidden-channels-{team_id}")),
            hidden_channels_label(hidden.len()),
            revealed,
            40.,
            cx.listener(move |this, _, _, cx| {
                if !this.revealed_channel_teams.remove(&toggle_id) {
                    this.revealed_channel_teams.insert(toggle_id.clone());
                }
                cx.notify();
            }),
        ));
        if revealed {
            for channel in hidden {
                list = list.child(self.team_channel_row(channel, false));
            }
        }
        list
    }

    fn reveal_row(
        &self,
        id: SharedString,
        label: String,
        open: bool,
        indent: f32,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> impl IntoElement {
        h_flex()
            .id(id)
            .mx(px(8.))
            .h(px(REVEAL_ROW_HEIGHT))
            .pl(px(indent))
            .pr(px(8.))
            .gap(px(8.))
            .items_center()
            .rounded(px(8.))
            .cursor_pointer()
            .hover(|row| row.bg(theme::row_hover()))
            .child(icon(
                if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                },
                12.,
                theme::text_faint(),
            ))
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(theme::text_muted())
                    .child(label),
            )
            .on_click(on_click)
    }

    fn team_channel_row(&self, channel: &ChannelRecord, is_selected: bool) -> impl IntoElement {
        let state = self.state.clone();
        let selection = Selection::Channel(channel.id.clone());
        h_flex()
            .id(SharedString::from(format!("channel-{}", channel.id)))
            .mx(px(8.))
            .h(px(TEAM_ROW_HEIGHT))
            .pl(px(40.))
            .pr(px(8.))
            .gap(px(8.))
            .items_center()
            .rounded(px(8.))
            .cursor_pointer()
            .when(is_selected, |row| row.bg(theme::surface_raised()))
            .when(!is_selected, |row| {
                row.hover(|row| row.bg(theme::row_hover()))
            })
            .child(
                div()
                    .w(px(14.))
                    .flex()
                    .justify_center()
                    .text_size(px(15.))
                    .text_color(theme::text_faint())
                    .child("#"),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_size(px(13.5))
                    .font_weight(if channel.unread {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(theme::text())
                    .child(channel.name.clone()),
            )
            .when(channel.unread, |row| row.child(dot(7.)))
            .on_click(move |_, _, cx| {
                let selection = selection.clone();
                state.update(cx, |state, cx| state.select(selection, cx));
            })
    }

    fn channels_body(&self, cx: &mut Context<Self>) -> AnyElement {
        let (sidebar, pinned_ids, selected) = {
            let state = self.state.read(cx);
            (
                state.sidebar.clone(),
                state.directory.pinned_channels.clone(),
                state.selection.clone(),
            )
        };
        let mut list = v_flex().w_full().pb(px(12.));
        let pinned: Vec<(&TeamRecord, &ChannelRecord)> = pinned_ids
            .iter()
            .filter_map(|id| {
                sidebar.teams.iter().find_map(|entry| {
                    entry
                        .channels
                        .iter()
                        .find(|channel| &channel.id == id)
                        .map(|channel| (&entry.team, channel))
                })
            })
            .collect();
        if !pinned.is_empty() {
            list = list.child(self.plain_header(PINNED_LABEL));
            for (team, channel) in pinned {
                let is_selected = selected == Some(Selection::Channel(channel.id.clone()));
                list = list.child(self.channel_row(&channel.id, team, channel, is_selected));
            }
        }
        list = list.child(self.plain_header(TEAMS_LABEL));
        let (shown_teams, hidden_teams): (Vec<&SidebarTeam>, Vec<&SidebarTeam>) =
            sidebar.teams.iter().partition(|entry| !entry.hidden);
        for entry in shown_teams {
            list = self.team_block(list, entry, selected.as_ref(), cx);
        }
        if !hidden_teams.is_empty() {
            let holds_selection = hidden_teams
                .iter()
                .any(|entry| team_holds(entry, selected.as_ref()));
            let open = self.hidden_teams_open || holds_selection;
            list = list.child(self.reveal_row(
                "hidden-teams".into(),
                format!("{HIDDEN_TEAMS_LABEL} ({})", hidden_teams.len()),
                open,
                8.,
                cx.listener(|this, _, _, cx| {
                    this.hidden_teams_open = !this.hidden_teams_open;
                    cx.notify();
                }),
            ));
            if open {
                for entry in hidden_teams {
                    list = self.team_block(list, entry, selected.as_ref(), cx);
                }
            }
        }
        div()
            .id("channels-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .track_scroll(&self.channels_scroll)
            .child(list)
            .into_any_element()
    }
}

fn team_holds(entry: &SidebarTeam, selected: Option<&Selection>) -> bool {
    entry
        .channels
        .iter()
        .any(|channel| selected == Some(&Selection::Channel(channel.id.clone())))
}

fn hidden_channels_label(count: usize) -> String {
    if count == 1 {
        "1 hidden channel".to_owned()
    } else {
        format!("{count} hidden channels")
    }
}

fn collect_faces(spec: &AvatarSpec, faces: &mut Vec<String>) {
    match spec {
        AvatarSpec::Single(face) => faces.extend(face.user_id.clone()),
        AvatarSpec::Pair(first, second) => {
            faces.extend(first.user_id.clone());
            faces.extend(second.user_id.clone());
        }
    }
}

fn with_spacer(list: Div, skipped_height: &mut f32) -> Div {
    let height = std::mem::take(skipped_height);
    if height > 0. {
        list.child(div().h(px(height)).flex_none())
    } else {
        list
    }
}

impl Render for SidebarView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (unread_chats, channel_dot) = {
            let state = self.state.read(cx);
            (
                unread_chat_count(&state.sidebar.chats),
                any_unread_channel(&state.sidebar),
            )
        };
        let window_height = f32::from(window.viewport_size().height);
        let body = match self.tab {
            SidebarTab::Chats => self.chats_body(window_height, cx),
            SidebarTab::Channels => self.channels_body(cx),
        };
        v_flex()
            .w(px(SIDEBAR_WIDTH))
            .h_full()
            .flex_none()
            .bg(theme::surface())
            .border_r_1()
            .border_color(theme::border())
            .overflow_hidden()
            .child(self.tab_bar(unread_chats, channel_dot, cx))
            .child(body)
    }
}
