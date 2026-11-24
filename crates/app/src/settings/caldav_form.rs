use gpui_kit::component::input::{Input, InputContentType, InputState};
use gpui_kit::component::{h_flex, v_flex};
use gpui_kit::*;

use super::rows::{control_row, stacked_field, text_button};
use crate::app_state::AppState;
use crate::i18n::tr;
use crate::views::new_chat::NewChatDraft;

pub struct CalDavForm {
    app: Entity<AppState>,
    url: Entity<InputState>,
    username: Entity<InputState>,
    password: Entity<InputState>,
    show_form: bool,
}

impl CalDavForm {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let password = cx.new(|cx| InputState::new(window, cx));
        password.update(cx, |input, cx| input.toggle_masked(window, cx));
        CalDavForm {
            app,
            url: cx.new(|cx| {
                InputState::new(window, cx).placeholder("https://cloud.example/remote.php/dav/")
            }),
            username: cx.new(|cx| InputState::new(window, cx)),
            password,
            show_form: false,
        }
    }
}

impl Render for CalDavForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.show_form {
            let connect = self.app.clone();
            let url_input = self.url.clone();
            let user_input = self.username.clone();
            let pass_input = self.password.clone();
            let set_nextcloud = self.url.clone();
            let set_fastmail = self.url.clone();
            let set_icloud = self.url.clone();
            let hide = cx.entity().clone();
            v_flex()
                .w_full()
                .gap(px(8.))
                .child(control_row(
                    tr("settings.calendar.caldav_title"),
                    tr("settings.calendar.caldav_hint"),
                    h_flex()
                        .gap_2()
                        .child(
                            text_button(
                                "settings-caldav-nextcloud",
                                tr("settings.calendar.caldav_template_nextcloud"),
                            )
                            .on_click(move |_, window, cx| {
                                set_nextcloud.update(cx, |input, cx| {
                                    input.set_value(
                                        "https://cloud.example/remote.php/dav/",
                                        window,
                                        cx,
                                    );
                                });
                            }),
                        )
                        .child(
                            text_button(
                                "settings-caldav-fastmail",
                                tr("settings.calendar.caldav_template_fastmail"),
                            )
                            .on_click(move |_, window, cx| {
                                set_fastmail.update(cx, |input, cx| {
                                    input.set_value("https://caldav.fastmail.com/dav/", window, cx);
                                });
                            }),
                        )
                        .child(
                            text_button(
                                "settings-caldav-icloud",
                                tr("settings.calendar.caldav_template_icloud"),
                            )
                            .on_click(move |_, window, cx| {
                                set_icloud.update(cx, |input, cx| {
                                    input.set_value("https://caldav.icloud.com/", window, cx);
                                });
                            }),
                        ),
                ))
                .child(stacked_field(
                    tr("settings.calendar.caldav_url"),
                    "",
                    Input::new(&self.url),
                ))
                .child(stacked_field(
                    tr("settings.calendar.caldav_username"),
                    "",
                    Input::new(&self.username),
                ))
                .child(stacked_field(
                    tr("settings.calendar.caldav_password"),
                    "",
                    Input::new(&self.password)
                        .content_type(InputContentType::Password)
                        .mask_toggle(),
                ))
                .child(control_row(
                    tr("settings.calendar.caldav_title"),
                    "",
                    h_flex()
                        .gap_2()
                        .child(
                            text_button(
                                "settings-caldav-connect",
                                tr("settings.calendar.caldav_connect"),
                            )
                            .on_click(move |_, _, cx| {
                                let url = url_input.read(cx).value().to_string();
                                let username = user_input.read(cx).value().to_string();
                                let password = pass_input.read(cx).value().to_string();
                                connect.update(cx, |state, cx| {
                                    state.connect_caldav_account(url, username, password, cx)
                                })
                            }),
                        )
                        .child(
                            text_button("settings-caldav-cancel", tr("common.cancel")).on_click(
                                move |_, _, cx| {
                                    hide.update(cx, |form, cx| {
                                        form.show_form = false;
                                        cx.notify();
                                    });
                                },
                            ),
                        ),
                ))
        } else {
            let show = cx.entity().clone();
            control_row(
                tr("settings.calendar.caldav_title"),
                tr("settings.calendar.caldav_hint"),
                text_button(
                    "settings-caldav-show",
                    tr("settings.calendar.caldav_connect"),
                )
                .on_click(move |_, _, cx| {
                    show.update(cx, |form, cx| {
                        form.show_form = true;
                        cx.notify();
                    });
                }),
            )
        }
    }
}

pub struct IcsForm {
    app: Entity<AppState>,
    url: Entity<InputState>,
    name: Entity<InputState>,
    show_form: bool,
}

impl IcsForm {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        IcsForm {
            app,
            url: cx.new(|cx| {
                InputState::new(window, cx).placeholder("https://example.com/calendar.ics")
            }),
            name: cx.new(|cx| InputState::new(window, cx).placeholder("Holidays")),
            show_form: false,
        }
    }
}

impl Render for IcsForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.show_form {
            let ics_connect = self.app.clone();
            let url_input = self.url.clone();
            let name_input = self.name.clone();
            let hide = cx.entity().clone();
            v_flex()
                .w_full()
                .gap(px(8.))
                .child(control_row(
                    tr("settings.calendar.ics_title"),
                    tr("settings.calendar.ics_hint"),
                    div(),
                ))
                .child(stacked_field(
                    tr("settings.calendar.ics_url"),
                    "",
                    Input::new(&self.url),
                ))
                .child(stacked_field(
                    tr("settings.calendar.ics_name"),
                    "",
                    Input::new(&self.name),
                ))
                .child(control_row(
                    tr("settings.calendar.ics_title"),
                    "",
                    h_flex()
                        .gap_2()
                        .child(
                            text_button(
                                "settings-ics-connect",
                                tr("settings.calendar.ics_connect"),
                            )
                            .on_click(move |_, _, cx| {
                                let url = url_input.read(cx).value().to_string();
                                let name = name_input.read(cx).value().to_string();
                                ics_connect.update(cx, |state, cx| {
                                    state.connect_ics_subscription(url, name, cx)
                                })
                            }),
                        )
                        .child(
                            text_button("settings-ics-cancel", tr("common.cancel")).on_click(
                                move |_, _, cx| {
                                    hide.update(cx, |form, cx| {
                                        form.show_form = false;
                                        cx.notify();
                                    });
                                },
                            ),
                        ),
                ))
        } else {
            let show = cx.entity().clone();
            control_row(
                tr("settings.calendar.ics_title"),
                tr("settings.calendar.ics_hint"),
                text_button("settings-ics-show", tr("settings.calendar.ics_connect")).on_click(
                    move |_, _, cx| {
                        show.update(cx, |form, cx| {
                            form.show_form = true;
                            cx.notify();
                        });
                    },
                ),
            )
        }
    }
}

pub struct PeopleCalendarForm {
    app: Entity<AppState>,
    people: Entity<NewChatDraft>,
}

impl PeopleCalendarForm {
    pub fn new(app: Entity<AppState>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let draft_app = app.clone();
        PeopleCalendarForm {
            app,
            people: cx.new(|cx| NewChatDraft::new(draft_app, window, cx).compact()),
        }
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        let picks = self.people.read(cx).chips().to_vec();
        for pick in picks {
            self.app.update(cx, |state, cx| {
                state.add_person_calendar(pick.user_id, pick.mail, pick.name, cx)
            });
        }
        self.people
            .update(cx, |draft, cx| draft.set_chips(Vec::new(), cx));
    }
}

impl Render for PeopleCalendarForm {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let form = cx.entity().clone();
        stacked_field(
            tr("settings.calendar.people_title"),
            tr("settings.calendar.people_hint"),
            h_flex()
                .w_full()
                .gap_2()
                .items_center()
                .child(div().flex_1().min_w_0().child(self.people.clone()))
                .child(
                    text_button("settings-people-add", tr("settings.calendar.people_add"))
                        .on_click(move |_, _, cx| form.update(cx, |form, cx| form.add(cx))),
                ),
        )
    }
}
