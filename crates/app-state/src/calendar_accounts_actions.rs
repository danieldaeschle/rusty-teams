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

    pub fn disconnect_google_account(&mut self, account_id: &str, cx: &mut Context<Self>) {
        let mut accounts = load_calendar_accounts(&self.store);
        accounts.disconnect_google(account_id);
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

    pub fn toggle_outlook_calendar_selected(&mut self, cx: &mut Context<Self>) {
        let mut accounts = load_calendar_accounts(&self.store);
        accounts.outlook_selected = !accounts.outlook_selected;
        let _ = save_calendar_accounts(&self.store, &accounts);
        self.refresh_calendar(cx);
        self.load_calendar_week(cx);
        cx.notify();
    }

    pub fn connect_google_calendar(&mut self, cx: &mut Context<Self>) {
        let Some(oauth) = client_id_from_env() else {
            self.raise_notice(
                "Set TEAMS_GOOGLE_CLIENT_ID (and optional TEAMS_GOOGLE_CLIENT_SECRET) to connect Google Calendar."
                    .to_owned(),
                None,
                cx,
            );
            return;
        };
        let store = self.store.clone();
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
                    this.update(cx, |state, cx| state.raise_notice(error, None, cx))
                        .ok();
                    return;
                }
                Err(_) => {
                    this.update(cx, |state, cx| {
                        state.raise_notice("Could not start Google sign-in.".to_owned(), None, cx)
                    })
                    .ok();
                    return;
                }
            };
            this.update(cx, |_, cx| cx.open_url(&authorize_url)).ok();
            let finished = runtime::spawn(async move {
                finish_google_connect(oauth, pending, listener, store).await
            });
            match finished.await {
                Ok(Ok(email)) => {
                    this.update(cx, |state, cx| {
                        state.raise_notice(
                            format!("Google Calendar connected ({email})."),
                            None,
                            cx,
                        );
                        state.refresh_calendar(cx);
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
                        state.raise_notice("Google sign-in was cancelled.".to_owned(), None, cx)
                    })
                    .ok();
                }
            }
        })
        .detach();
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
