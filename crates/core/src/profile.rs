use chatsvc::{PresenceService, PresenceStatus, WorkLocation};
use graph::{Graph, ProfileDetails};

pub use graph::OrgPerson;

use crate::engine::SyncEngine;
use crate::error::Result;
use crate::presence::Availability;
use crate::remote::Remote;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PersonProfile {
    pub user_id: String,
    pub display_name: Option<String>,
    pub job_title: Option<String>,
    pub department: Option<String>,
    pub company: Option<String>,
    pub office_location: Option<String>,
    pub email: Option<String>,
    pub work_phones: Vec<String>,
    pub mobile_phone: Option<String>,
    pub manager: Option<OrgPerson>,
    pub direct_reports: Vec<OrgPerson>,
    pub time_zone: Option<String>,
    pub availability: Option<Availability>,
    pub status_message: Option<String>,
    pub out_of_office: Option<String>,
    pub work_location: Option<WorkLocation>,
}

impl PersonProfile {
    pub fn from_remote(
        details: ProfileDetails,
        time_zone: Option<String>,
        presence: Option<PresenceStatus>,
    ) -> Self {
        let ProfileDetails {
            user,
            manager,
            direct_reports,
        } = details;
        let email = user.mail.or(user.user_principal_name);
        PersonProfile {
            user_id: user.id,
            display_name: user.display_name,
            job_title: user.job_title,
            department: user.department,
            company: user.company_name,
            office_location: user.office_location,
            email,
            work_phones: user.business_phones,
            mobile_phone: user.mobile_phone,
            manager,
            direct_reports,
            time_zone,
            availability: presence
                .as_ref()
                .map(|detail| Availability::from_service(&detail.availability)),
            status_message: presence
                .as_ref()
                .and_then(|detail| detail.note.as_ref().map(|note| note.text.clone())),
            work_location: presence.as_ref().and_then(|detail| detail.work_location),
            out_of_office: presence.and_then(|detail| {
                detail
                    .out_of_office
                    .then(|| detail.out_of_office_note.unwrap_or_default())
            }),
        }
    }
}

pub(crate) async fn load_profile(graph: &Graph, user_id: &str) -> Result<PersonProfile> {
    let details = graph.user_profile(user_id).await?;
    let mail = details
        .user
        .mail
        .clone()
        .or_else(|| details.user.user_principal_name.clone());
    let zone = async {
        match mail {
            Some(mail) => graph.time_zone_name(&mail).await.ok().flatten(),
            None => None,
        }
    };
    let presence_service = PresenceService::new(graph.session());
    let (time_zone, presence) = tokio::join!(zone, presence_service.status(user_id));
    Ok(PersonProfile::from_remote(
        details,
        time_zone,
        presence.ok(),
    ))
}

impl<R: Remote> SyncEngine<R> {
    pub async fn person_profile(&self, user_id: &str) -> Result<PersonProfile> {
        self.remote.person_profile(user_id).await
    }
}

#[cfg(test)]
mod tests {
    use chatsvc::StatusNote;
    use graph::UserProfile;

    use super::*;

    fn details(user: UserProfile) -> ProfileDetails {
        ProfileDetails {
            user,
            manager: None,
            direct_reports: Vec::new(),
        }
    }

    #[test]
    fn the_user_principal_name_stands_in_for_a_missing_mail() {
        let user = UserProfile {
            id: "u".into(),
            user_principal_name: Some("ada@example.com".into()),
            ..Default::default()
        };
        let profile = PersonProfile::from_remote(details(user), None, None);
        assert_eq!(profile.email.as_deref(), Some("ada@example.com"));
        assert_eq!(profile.availability, None);
    }

    #[test]
    fn presence_fills_availability_note_and_out_of_office() {
        let presence = PresenceStatus {
            availability: "DoNotDisturb".into(),
            note: Some(StatusNote {
                text: "Focus".into(),
                show_when_messaged: false,
                expires_at: None,
            }),
            out_of_office: true,
            out_of_office_note: Some("Away until Monday".into()),
            ..PresenceStatus::default()
        };
        let profile = PersonProfile::from_remote(
            details(UserProfile::default()),
            Some("UTC".into()),
            Some(presence),
        );
        assert_eq!(profile.availability, Some(Availability::DoNotDisturb));
        assert_eq!(profile.status_message.as_deref(), Some("Focus"));
        assert_eq!(profile.out_of_office.as_deref(), Some("Away until Monday"));
        assert_eq!(profile.time_zone.as_deref(), Some("UTC"));
    }
}
