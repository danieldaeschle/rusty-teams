use std::sync::Arc;

use chatsvc::{CreatedEvent, GoogleEventDraft, NewEvent};
use store::{CalendarAccounts, CalendarProvider, CalendarRef, Store};

use super::launcher::CallLauncher;
use super::schedule::MeetingDraft;

/// Whether create must mint a Teams join URL before per-target inserts.
pub fn needs_join_first(accounts: &CalendarAccounts) -> bool {
    let targets = accounts.resolved_write_targets();
    targets.len() > 1
        || targets
            .iter()
            .any(|target| target.provider == CalendarProvider::Google)
}

/// Write targets used for New meeting (home + extras, de-duplicated).
pub fn create_targets(accounts: &CalendarAccounts) -> Vec<CalendarRef> {
    accounts.resolved_write_targets()
}

/// Format a partial-failure notice when the join link exists but a target failed.
pub fn partial_create_notice(join_url: &str, failed: &[String]) -> String {
    format!(
        "Meeting link ready, but calendar write failed for {}: {join_url}",
        failed.join(", ")
    )
}

pub async fn create_across_targets(
    launcher: Arc<CallLauncher>,
    draft: MeetingDraft,
    mut accounts: CalendarAccounts,
    store: Arc<Store>,
) -> Result<CreatedEvent, String> {
    if !needs_join_first(&accounts) {
        return launcher.create_meeting(draft).await;
    }
    let event = launcher.resolve_new_event(draft).await?;
    let join_url = launcher.create_meet_now(&event.subject).await?;
    let targets = create_targets(&accounts);
    let mut failed = Vec::new();
    let mut first_id = String::new();
    for target in targets {
        match write_target(launcher.as_ref(), &mut accounts, &target, &event, &join_url).await {
            Ok(id) => {
                if first_id.is_empty() {
                    first_id = id;
                }
            }
            Err(error) => failed.push(format!("{} ({error})", target.key())),
        }
    }
    let _ = store::save_calendar_accounts(&store, &accounts);
    if !failed.is_empty() {
        crate::diagnostics::record(partial_create_notice(&join_url, &failed));
        if first_id.is_empty() {
            return Err(partial_create_notice(&join_url, &failed));
        }
    }
    Ok(CreatedEvent {
        id: if first_id.is_empty() {
            join_url.clone()
        } else {
            first_id
        },
        join_url: Some(join_url),
    })
}

async fn write_target(
    launcher: &CallLauncher,
    accounts: &mut CalendarAccounts,
    target: &CalendarRef,
    event: &NewEvent,
    join_url: &str,
) -> Result<String, String> {
    match target.provider {
        CalendarProvider::Outlook => launcher
            .create_meeting_with_join(event.clone(), join_url)
            .await
            .map(|created| created.id),
        CalendarProvider::Google => {
            let calendar_id = if target.calendar_id.is_empty() {
                "primary"
            } else {
                target.calendar_id.as_str()
            };
            let mut draft = GoogleEventDraft::from(event);
            draft.join_url = Some(join_url.to_owned());
            crate::google_calendar_sync::insert_google_event(
                accounts,
                &target.account_id,
                calendar_id,
                &draft,
            )
            .await?;
            Ok(format!("google:{}:{}", target.account_id, calendar_id))
        }
    }
}

#[cfg(test)]
mod tests {
    use store::{CalendarRef, GoogleAccount, GoogleCalendarInfo, GoogleTokenSet};

    use super::*;

    fn google_home() -> CalendarAccounts {
        CalendarAccounts {
            home: CalendarRef::google("g1", "primary"),
            write_targets: Vec::new(),
            outlook_selected: true,
            google_accounts: vec![GoogleAccount {
                id: "g1".into(),
                email: "joe@example.com".into(),
                calendars: vec![GoogleCalendarInfo {
                    id: "primary".into(),
                    summary: "Joe".into(),
                    selected: true,
                    primary: true,
                }],
                tokens: GoogleTokenSet::default(),
            }],
        }
    }

    #[test]
    fn outlook_only_home_keeps_one_step() {
        let accounts = CalendarAccounts::default();
        assert!(!needs_join_first(&accounts));
        assert_eq!(create_targets(&accounts).len(), 1);
        assert_eq!(
            create_targets(&accounts)[0].provider,
            CalendarProvider::Outlook
        );
    }

    #[test]
    fn google_home_needs_join_first() {
        assert!(needs_join_first(&google_home()));
    }

    #[test]
    fn outlook_home_with_google_extra_needs_join_first() {
        let mut accounts = CalendarAccounts::default();
        accounts
            .write_targets
            .push(CalendarRef::google("g1", "primary"));
        assert!(needs_join_first(&accounts));
        assert_eq!(create_targets(&accounts).len(), 2);
    }

    #[test]
    fn partial_notice_keeps_join_url() {
        let notice = partial_create_notice(
            "https://teams.microsoft.com/l/meetup-join/x",
            &["Google:primary".into()],
        );
        assert!(notice.contains("teams.microsoft.com"));
        assert!(notice.contains("Google:primary"));
    }
}
