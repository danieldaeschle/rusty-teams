use app_base::ui_locale::{tr, tr_args};
use chrono::Utc;
use gpui_kit::Context;
use store::{
    CalendarAccounts, CalendarRef, GoogleAccount, GoogleCalendarInfo, GoogleTokenSet,
    load_calendar_accounts, save_calendar_accounts,
};
use uuid::Uuid;

use chatsvc::{
    GoogleCalendar, GoogleOAuth, allocate_redirect_uri, client_id_from_env, exchange_code,
    parse_calendar_list, wait_for_code,
};

use crate::app_state::AppState;
use crate::runtime;

impl AppState {
    pub fn calendar_accounts(&self) -> CalendarAccounts {
        load_calendar_accounts(&self.store)
    }

    pub fn google_connect_pending(&self) -> Option<&str> {
        self.google_connect_pending.as_deref()
    }

    pub fn disconnect_google_account(&mut self, account_id: &str, cx: &mut Context<Self>) {
        let mut accounts = load_calendar_accounts(&self.store);
        accounts.disconnect_google(account_id);
        let _ = save_calendar_accounts(&self.store, &accounts);
        self.refresh_calendar(cx);
        self.load_calendar_week(cx);
        cx.notify();
    }

    pub fn connect_caldav_account(
        &mut self,
        url: String,
        username: String,
        password: String,
        cx: &mut Context<Self>,
    ) {
        let store = self.store.clone();
        let connecting = runtime::spawn(async move {
            let mut accounts = load_calendar_accounts(&store);
            let id = crate::caldav_sync::connect_caldav(&mut accounts, &url, &username, &password)
                .await?;
            save_calendar_accounts(&store, &accounts).map_err(|error| error.to_string())?;
            Ok::<_, String>(id)
        });
        cx.spawn(async move |this, cx| match connecting.await {
            Ok(Ok(_)) => {
                this.update(cx, |state, cx| {
                    state.raise_notice(tr("settings.calendar.caldav_connected"), None, cx);
                    state.refresh_calendar(cx);
                    state.load_calendar_week(cx);
                    cx.notify();
                })
                .ok();
            }
            Ok(Err(error)) => {
                this.update(cx, |state, cx| {
                    state.raise_notice(
                        crate::caldav_credentials::redact_secret_text(&error),
                        None,
                        cx,
                    );
                })
                .ok();
            }
            Err(_) => {
                this.update(cx, |state, cx| {
                    state.raise_notice(tr("settings.calendar.caldav_connect_failed"), None, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    pub fn disconnect_caldav_account(&mut self, account_id: &str, cx: &mut Context<Self>) {
        let mut accounts = load_calendar_accounts(&self.store);
        if let Err(error) = crate::caldav_sync::disconnect_caldav(&mut accounts, account_id) {
            self.raise_notice(error, None, cx);
            return;
        }
        let _ = save_calendar_accounts(&self.store, &accounts);
        self.refresh_calendar(cx);
        self.load_calendar_week(cx);
        cx.notify();
    }

    pub fn toggle_caldav_calendar_selected(
        &mut self,
        account_id: &str,
        calendar_id: &str,
        cx: &mut Context<Self>,
    ) {
        let mut accounts = load_calendar_accounts(&self.store);
        if let Some(calendar) = accounts.caldav_account_mut(account_id).and_then(|account| {
            account
                .calendars
                .iter_mut()
                .find(|calendar| calendar.id == calendar_id)
        }) {
            calendar.selected = !calendar.selected;
        }
        let _ = save_calendar_accounts(&self.store, &accounts);
        self.refresh_calendar(cx);
        self.load_calendar_week(cx);
        cx.notify();
    }

    pub fn connect_ics_subscription(
        &mut self,
        url: String,
        summary: String,
        cx: &mut Context<Self>,
    ) {
        let store = self.store.clone();
        let connecting = runtime::spawn(async move {
            let mut accounts = load_calendar_accounts(&store);
            let id = crate::ics_sync::add_ics_subscription(&mut accounts, &url, &summary).await?;
            save_calendar_accounts(&store, &accounts).map_err(|error| error.to_string())?;
            Ok::<_, String>(id)
        });
        cx.spawn(async move |this, cx| match connecting.await {
            Ok(Ok(_)) => {
                this.update(cx, |state, cx| {
                    state.raise_notice(tr("settings.calendar.ics_connected"), None, cx);
                    state.refresh_calendar(cx);
                    state.load_calendar_week(cx);
                    cx.notify();
                })
                .ok();
            }
            Ok(Err(error)) => {
                this.update(cx, |state, cx| state.raise_notice(error, None, cx))
                    .ok();
            }
            Err(_) => {
                this.update(cx, |state, cx| {
                    state.raise_notice(tr("settings.calendar.ics_connect_failed"), None, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    /// Picks from recent chats carry no mail; free/busy needs it, so it is looked up by user id.
    pub fn add_person_calendar(
        &mut self,
        user_id: String,
        mail: Option<String>,
        name: String,
        cx: &mut Context<Self>,
    ) {
        let engine = self.engine.clone();
        let resolving = runtime::spawn(async move {
            match mail.filter(|mail| !mail.is_empty()) {
                Some(mail) => Some(mail),
                None => {
                    let engine = engine?;
                    graph::Graph::new(engine.remote().session().clone())
                        .user_mail(&user_id)
                        .await
                        .ok()
                        .flatten()
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let mail = resolving.await.ok().flatten();
            this.update(cx, |state, cx| match mail {
                Some(mail) => {
                    let mut accounts = load_calendar_accounts(&state.store);
                    let known = accounts
                        .people_calendars
                        .iter()
                        .any(|person| person.mail.eq_ignore_ascii_case(&mail));
                    let color = crate::call::week_grid::free_palette_color(
                        &accounts,
                        app_base::theme::avatar_palette_len(),
                    );
                    accounts.add_person_calendar(&mail, &name);
                    if !known {
                        accounts.set_person_color(&mail, color);
                    }
                    let _ = save_calendar_accounts(&state.store, &accounts);
                    state.load_calendar_week(cx);
                    cx.notify();
                }
                None => state.raise_notice(tr("settings.calendar.people_no_mail"), None, cx),
            })
            .ok();
        })
        .detach();
    }

    pub fn set_person_calendar_color(&mut self, mail: &str, color: usize, cx: &mut Context<Self>) {
        let mut accounts = load_calendar_accounts(&self.store);
        accounts.set_person_color(mail, color);
        let _ = save_calendar_accounts(&self.store, &accounts);
        cx.notify();
    }

    pub fn remove_person_calendar(&mut self, mail: &str, cx: &mut Context<Self>) {
        let mut accounts = load_calendar_accounts(&self.store);
        accounts.remove_person_calendar(mail);
        let _ = save_calendar_accounts(&self.store, &accounts);
        self.load_calendar_week(cx);
        cx.notify();
    }

    pub fn toggle_person_calendar_selected(&mut self, mail: &str, cx: &mut Context<Self>) {
        let mut accounts = load_calendar_accounts(&self.store);
        if let Some(person) = accounts
            .people_calendars
            .iter_mut()
            .find(|person| person.mail.eq_ignore_ascii_case(mail))
        {
            person.selected = !person.selected;
        }
        let _ = save_calendar_accounts(&self.store, &accounts);
        self.load_calendar_week(cx);
        cx.notify();
    }

    pub fn set_people_calendars_selected(&mut self, selected: bool, cx: &mut Context<Self>) {
        let mut accounts = load_calendar_accounts(&self.store);
        for person in &mut accounts.people_calendars {
            person.selected = selected;
        }
        let _ = save_calendar_accounts(&self.store, &accounts);
        self.load_calendar_week(cx);
        cx.notify();
    }

    pub fn disconnect_ics_subscription(&mut self, subscription_id: &str, cx: &mut Context<Self>) {
        let mut accounts = load_calendar_accounts(&self.store);
        accounts.disconnect_ics(subscription_id);
        let _ = save_calendar_accounts(&self.store, &accounts);
        self.refresh_calendar(cx);
        self.load_calendar_week(cx);
        cx.notify();
    }

    pub fn toggle_ics_subscription_selected(
        &mut self,
        subscription_id: &str,
        cx: &mut Context<Self>,
    ) {
        let mut accounts = load_calendar_accounts(&self.store);
        if let Some(subscription) = accounts
            .ics_subscriptions
            .iter_mut()
            .find(|subscription| subscription.id == subscription_id)
        {
            subscription.selected = !subscription.selected;
        }
        let _ = save_calendar_accounts(&self.store, &accounts);
        self.refresh_calendar(cx);
        self.load_calendar_week(cx);
        cx.notify();
    }

    pub fn set_home_calendar(&mut self, home: CalendarRef, cx: &mut Context<Self>) {
        let mut accounts = load_calendar_accounts(&self.store);
        accounts.home = home;
        let _ = save_calendar_accounts(&self.store, &accounts);
        cx.notify();
    }

    pub fn toggle_write_target(&mut self, target: CalendarRef, cx: &mut Context<Self>) {
        let mut accounts = load_calendar_accounts(&self.store);
        if let Some(index) = accounts
            .write_targets
            .iter()
            .position(|existing| existing.key() == target.key())
        {
            accounts.write_targets.remove(index);
        } else if accounts.home.key() != target.key() {
            accounts.write_targets.push(target);
        }
        let _ = save_calendar_accounts(&self.store, &accounts);
        cx.notify();
    }

    pub fn toggle_google_calendar_selected(
        &mut self,
        account_id: &str,
        calendar_id: &str,
        cx: &mut Context<Self>,
    ) {
        let mut accounts = load_calendar_accounts(&self.store);
        if let Some(calendar) = accounts.google_account_mut(account_id).and_then(|account| {
            account
                .calendars
                .iter_mut()
                .find(|calendar| calendar.id == calendar_id)
        }) {
            calendar.selected = !calendar.selected;
        }
        let _ = save_calendar_accounts(&self.store, &accounts);
        self.refresh_calendar(cx);
        self.load_calendar_week(cx);
        cx.notify();
    }

    /// Group check box: turn every calendar of one Google account on or off.
    pub fn set_google_account_selected(
        &mut self,
        account_id: &str,
        on: bool,
        cx: &mut Context<Self>,
    ) {
        let mut accounts = load_calendar_accounts(&self.store);
        if let Some(account) = accounts.google_account_mut(account_id) {
            for calendar in &mut account.calendars {
                calendar.selected = on;
            }
        }
        let _ = save_calendar_accounts(&self.store, &accounts);
        self.refresh_calendar(cx);
        cx.notify();
    }

    pub fn toggle_outlook_calendar_selected(&mut self, cx: &mut Context<Self>) {
        let mut accounts = load_calendar_accounts(&self.store);
        accounts.outlook_selected = !accounts.outlook_selected;
        let _ = save_calendar_accounts(&self.store, &accounts);
        self.refresh_calendar(cx);
        self.load_calendar_week(cx);
        cx.notify();
    }

    pub fn reopen_google_connect(&mut self, cx: &mut Context<Self>) {
        if let Some(url) = self.google_connect_pending.clone() {
            crate::external_browser::open_url(cx, &self.store, &url);
        }
    }

    pub fn cancel_google_connect(&mut self, cx: &mut Context<Self>) {
        self.google_connect_generation = self.google_connect_generation.wrapping_add(1);
        self.google_connect_pending = None;
        self.raise_notice(tr("settings.calendar.sign_in_cancelled"), None, cx);
        cx.notify();
    }

    pub fn connect_google_calendar(&mut self, cx: &mut Context<Self>) {
        if self.google_connect_pending.is_some() {
            self.reopen_google_connect(cx);
            return;
        }
        let Some(oauth) = client_id_from_env() else {
            self.raise_notice(tr("settings.calendar.not_in_build"), None, cx);
            return;
        };
        let store = self.store.clone();
        self.google_connect_generation = self.google_connect_generation.wrapping_add(1);
        let generation = self.google_connect_generation;
        cx.spawn(async move |this, cx| {
            let prepared = runtime::spawn({
                let oauth = oauth.clone();
                async move {
                    let (redirect_uri, listener) = allocate_redirect_uri()
                        .await
                        .map_err(|error| error.to_string())?;
                    let pending = oauth.begin(&redirect_uri);
                    let authorize_url = pending.authorize_url.clone();
                    Ok::<_, String>((authorize_url, pending, listener, oauth))
                }
            });
            let (authorize_url, pending, listener, oauth) = match prepared.await {
                Ok(Ok(value)) => value,
                Ok(Err(error)) => {
                    this.update(cx, |state, cx| {
                        clear_google_connect_if(state, generation);
                        state.raise_notice(error, None, cx);
                    })
                    .ok();
                    return;
                }
                Err(_) => {
                    this.update(cx, |state, cx| {
                        clear_google_connect_if(state, generation);
                        state.raise_notice(tr("settings.calendar.sign_in_start_failed"), None, cx);
                    })
                    .ok();
                    return;
                }
            };
            let still_current = this
                .update(cx, |state, cx| {
                    if state.google_connect_generation != generation {
                        return false;
                    }
                    state.google_connect_pending = Some(authorize_url.clone());
                    crate::external_browser::open_url(cx, &state.store, &authorize_url);
                    cx.notify();
                    true
                })
                .unwrap_or(false);
            if !still_current {
                return;
            }
            let finished = runtime::spawn(async move {
                finish_google_connect(oauth, pending, listener, store).await
            });
            match finished.await {
                Ok(Ok(email)) => {
                    this.update(cx, |state, cx| {
                        if state.google_connect_generation != generation {
                            return;
                        }
                        state.google_connect_pending = None;
                        let mut args = std::collections::HashMap::new();
                        args.insert("email".to_owned(), email);
                        state.raise_notice(tr_args("settings.calendar.connected", &args), None, cx);
                        state.refresh_calendar(cx);
                        cx.notify();
                    })
                    .ok();
                }
                Ok(Err(error)) => {
                    this.update(cx, |state, cx| {
                        if state.google_connect_generation != generation {
                            return;
                        }
                        state.google_connect_pending = None;
                        state.raise_notice(error, None, cx);
                        cx.notify();
                    })
                    .ok();
                }
                Err(_) => {
                    this.update(cx, |state, cx| {
                        if state.google_connect_generation != generation {
                            return;
                        }
                        state.google_connect_pending = None;
                        state.raise_notice(tr("settings.calendar.sign_in_cancelled"), None, cx);
                        cx.notify();
                    })
                    .ok();
                }
            }
        })
        .detach();
    }
}

fn clear_google_connect_if(state: &mut AppState, generation: u64) {
    if state.google_connect_generation == generation {
        state.google_connect_pending = None;
    }
}

async fn finish_google_connect(
    oauth: GoogleOAuth,
    pending: chatsvc::AuthPending,
    listener: tokio::net::TcpListener,
    store: std::sync::Arc<store::Store>,
) -> Result<String, String> {
    let code = wait_for_code(&pending, listener)
        .await
        .map_err(|error| error.to_string())?;
    let bundle = exchange_code(&oauth, &pending, &code)
        .await
        .map_err(|error| error.to_string())?;
    let client = GoogleCalendar::new(bundle.access_token.clone());
    let list = client
        .list_calendars()
        .await
        .map_err(|error| error.to_string())?;
    let mut calendars = parse_calendar_list(&list)
        .into_iter()
        .map(|(id, summary, primary)| GoogleCalendarInfo {
            id,
            summary,
            selected: true,
            primary,
        })
        .collect::<Vec<_>>();
    if calendars.is_empty() {
        calendars.push(GoogleCalendarInfo {
            id: "primary".into(),
            summary: "Primary".into(),
            selected: true,
            primary: true,
        });
    }
    let email = calendars
        .iter()
        .find(|calendar| calendar.primary)
        .map(|calendar| calendar.summary.clone())
        .unwrap_or_else(|| "Google".into());
    let account = GoogleAccount {
        id: Uuid::new_v4().to_string(),
        email: email.clone(),
        calendars,
        tokens: GoogleTokenSet {
            access_token: bundle.access_token,
            refresh_token: bundle.refresh_token,
            expires_at: Utc::now().timestamp() + bundle.expires_in.max(60),
        },
    };
    let mut accounts = load_calendar_accounts(&store);
    accounts.google_accounts.push(account);
    save_calendar_accounts(&store, &accounts).map_err(|error| error.to_string())?;
    Ok(email)
}
