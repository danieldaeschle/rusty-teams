use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use teams_core::{
    Availability, OrgPerson, PersonProfile, WorkLocation, WorkLocationKind, WorkLocationSource,
};

use crate::demo::{DEMO_USER_ID, JONAS_ID, LEA_ID, MARA_ID, PRIYA_ID, TOBIAS_ID};

const DELAY_VARIABLE: &str = "TEAMS_DEMO_PROFILE_DELAY_MS";
const DEFAULT_DELAY: Duration = Duration::from_millis(500);
const HENRIK_ID: &str = "demo-henrik";
const SOFIA_ID: &str = "demo-sofia";
const OMAR_ID: &str = "demo-omar";
const LONG_STATUS: &str = "Heads down on the Q4 roadmap review until Thursday. Please use the Product channel for anything that can wait and call my mobile only for production incidents. Back to normal office hours next week, and I will answer open threads then.";
const OUT_OF_OFFICE: &str = "I am out of office until 20 October with limited access to email. For urgent customer matters please contact Tobias Klein.";

static TOBIAS_FAILED_ONCE: AtomicBool = AtomicBool::new(false);

fn person(id: &str, name: &str, title: &str) -> OrgPerson {
    OrgPerson {
        id: id.to_owned(),
        display_name: Some(name.to_owned()),
        job_title: Some(title.to_owned()),
    }
}

fn base(id: &str, name: &str, title: &str, department: &str) -> PersonProfile {
    let address = name.to_lowercase().replace(' ', ".");
    PersonProfile {
        user_id: id.to_owned(),
        display_name: Some(name.to_owned()),
        job_title: Some(title.to_owned()),
        department: Some(department.to_owned()),
        company: Some("Example Corp".to_owned()),
        office_location: Some("Berlin, Building A".to_owned()),
        email: Some(format!("{address}@example.com")),
        work_phones: vec!["+49 30 5550 1000".to_owned()],
        mobile_phone: Some("+49 151 5550 2000".to_owned()),
        manager: None,
        direct_reports: Vec::new(),
        time_zone: Some("W. Europe Standard Time".to_owned()),
        availability: Some(Availability::Available),
        status_message: None,
        out_of_office: None,
        work_location: None,
    }
}

fn henrik() -> OrgPerson {
    person(HENRIK_ID, "Henrik Vogt", "VP Engineering")
}

fn priya() -> OrgPerson {
    person(PRIYA_ID, "Priya Nair", "Engineering Manager")
}

pub fn profile(user_id: &str) -> Option<PersonProfile> {
    Some(match user_id {
        MARA_ID => PersonProfile {
            manager: Some(henrik()),
            availability: Some(Availability::Busy),
            status_message: Some(LONG_STATUS.to_owned()),
            work_location: Some(WorkLocation {
                kind: WorkLocationKind::Office,
                source: WorkLocationSource::Set,
            }),
            ..base(MARA_ID, "Mara Lindqvist", "Product Owner", "Product")
        },
        JONAS_ID => PersonProfile {
            manager: Some(priya()),
            work_phones: Vec::new(),
            ..base(JONAS_ID, "Jonas Ortega", "Backend Engineer", "Engineering")
        },
        PRIYA_ID => PersonProfile {
            manager: Some(henrik()),
            direct_reports: vec![
                person(JONAS_ID, "Jonas Ortega", "Backend Engineer"),
                person(TOBIAS_ID, "Tobias Klein", "Support Engineer"),
                person(DEMO_USER_ID, "Dana Demo", "Software Engineer"),
                person(SOFIA_ID, "Sofia Marin", "QA Engineer"),
            ],
            availability: Some(Availability::DoNotDisturb),
            status_message: Some("In the planning meeting, back at 3".to_owned()),
            time_zone: Some("Pacific Standard Time".to_owned()),
            ..base(PRIYA_ID, "Priya Nair", "Engineering Manager", "Engineering")
        },
        LEA_ID => PersonProfile {
            manager: Some(person(OMAR_ID, "Omar Haddad", "Head of Customer Success")),
            availability: Some(Availability::Offline),
            out_of_office: Some(OUT_OF_OFFICE.to_owned()),
            time_zone: Some("GMT Standard Time".to_owned()),
            mobile_phone: None,
            ..base(
                LEA_ID,
                "Lea Schneider",
                "Customer Success Manager",
                "Customer Success",
            )
        },
        TOBIAS_ID => PersonProfile {
            manager: Some(priya()),
            availability: Some(Availability::Away),
            work_location: Some(WorkLocation {
                kind: WorkLocationKind::Remote,
                source: WorkLocationSource::Set,
            }),
            time_zone: Some("AUS Eastern Standard Time".to_owned()),
            office_location: None,
            ..base(
                TOBIAS_ID,
                "Tobias Klein",
                "Support Engineer",
                "Customer Success",
            )
        },
        DEMO_USER_ID => PersonProfile {
            manager: Some(priya()),
            ..base(
                DEMO_USER_ID,
                "Dana Demo",
                "Software Engineer",
                "Engineering",
            )
        },
        HENRIK_ID => PersonProfile {
            direct_reports: vec![priya(), person(MARA_ID, "Mara Lindqvist", "Product Owner")],
            ..base(HENRIK_ID, "Henrik Vogt", "VP Engineering", "Engineering")
        },
        SOFIA_ID => PersonProfile {
            manager: Some(priya()),
            ..base(SOFIA_ID, "Sofia Marin", "QA Engineer", "Engineering")
        },
        OMAR_ID => PersonProfile {
            direct_reports: vec![person(LEA_ID, "Lea Schneider", "Customer Success Manager")],
            ..base(
                OMAR_ID,
                "Omar Haddad",
                "Head of Customer Success",
                "Customer Success",
            )
        },
        _ => return None,
    })
}

pub async fn load(user_id: String) -> Result<PersonProfile, String> {
    tokio::time::sleep(delay()).await;
    if user_id == TOBIAS_ID && !TOBIAS_FAILED_ONCE.swap(true, Ordering::SeqCst) {
        return Err("demo failure".to_owned());
    }
    profile(&user_id).ok_or_else(|| "unknown demo person".to_owned())
}

fn delay() -> Duration {
    std::env::var(DELAY_VARIABLE)
        .ok()
        .and_then(|value| value.parse().ok())
        .map_or(DEFAULT_DELAY, Duration::from_millis)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_demo_person_has_a_profile() {
        let ids = [MARA_ID, JONAS_ID, PRIYA_ID, LEA_ID, TOBIAS_ID, DEMO_USER_ID];
        for id in ids {
            let profile = profile(id).expect("demo profile");
            assert_eq!(profile.user_id, id);
        }
    }

    #[test]
    fn one_person_is_out_of_office_and_one_has_a_long_message() {
        assert!(profile(LEA_ID).unwrap().out_of_office.is_some());
        assert!(profile(MARA_ID).unwrap().status_message.unwrap().len() > 200);
    }
}
