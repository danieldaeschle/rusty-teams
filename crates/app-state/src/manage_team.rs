use std::cmp::Ordering;

use gpui_kit::*;

use crate::app_state::{AppEvent, AppState};
use crate::runtime;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamMember {
    pub user_id: Option<String>,
    pub name: String,
    pub email: Option<String>,
    pub owner: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MembersLoad {
    Loading,
    Loaded(Vec<TeamMember>),
    Failed(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManageTeam {
    pub team_id: String,
    pub team_name: String,
    pub members: MembersLoad,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct MemberGroups<'a> {
    pub owners: Vec<&'a TeamMember>,
    pub others: Vec<&'a TeamMember>,
}

pub fn member_groups<'a>(members: &'a [TeamMember], query: &str) -> MemberGroups<'a> {
    let query = query.trim().to_lowercase();
    let mut matching: Vec<&TeamMember> = members
        .iter()
        .filter(|member| {
            query.is_empty()
                || member.name.to_lowercase().contains(&query)
                || member.email.as_deref().is_some_and(|email| email.to_lowercase().contains(&query))
        })
        .collect();
    matching.sort_by(|left, right| compare_names(&left.name, &right.name));
    let (owners, others) = matching.into_iter().partition(|member| member.owner);
    MemberGroups { owners, others }
}

fn compare_names(left: &str, right: &str) -> Ordering {
    left.to_lowercase().cmp(&right.to_lowercase()).then_with(|| left.cmp(right))
}

fn team_member(member: graph::Member) -> TeamMember {
    TeamMember {
        name: member.name().to_owned(),
        owner: member.is_owner(),
        user_id: member.user_id,
        email: member.email.filter(|email| !email.is_empty()),
    }
}

impl AppState {
    pub fn open_manage_team(&mut self, team_id: &str, cx: &mut Context<Self>) {
        let Some(team_name) = self
            .sidebar
            .teams
            .iter()
            .find(|team| team.team.id == team_id)
            .map(|team| team.team.name.clone())
        else {
            return;
        };
        self.quick_view = None;
        self.manage_team = Some(ManageTeam {
            team_id: team_id.to_owned(),
            team_name,
            members: MembersLoad::Loading,
        });
        cx.emit(AppEvent::Sidebar);
        cx.notify();
        if self.mode.demo {
            self.finish_member_load(team_id, Ok(crate::demo::team_members()), cx);
            return;
        }
        let Some(engine) = self.engine.clone() else {
            self.finish_member_load(team_id, Err("Not connected yet".to_owned()), cx);
            return;
        };
        let lookup = team_id.to_owned();
        let receiver = runtime::spawn(async move { engine.team_members(&lookup).await });
        let team_id = team_id.to_owned();
        cx.spawn(async move |this, cx| {
            let result = match receiver.await {
                Ok(Ok(members)) => Ok(members.into_iter().map(team_member).collect()),
                Ok(Err(error)) => Err(error.to_string()),
                Err(error) => Err(error.to_string()),
            };
            this.update(cx, |state, cx| state.finish_member_load(&team_id, result, cx))
                .ok();
        })
        .detach();
    }

    fn finish_member_load(&mut self, team_id: &str, result: Result<Vec<TeamMember>, String>, cx: &mut Context<Self>) {
        let Some(manage_team) = self.manage_team.as_mut().filter(|open| open.team_id == team_id) else {
            return;
        };
        let mut user_ids = Vec::new();
        manage_team.members = match result {
            Ok(members) => {
                user_ids = members.iter().filter_map(|member| member.user_id.clone()).collect();
                MembersLoad::Loaded(members)
            }
            Err(error) => {
                app_base::diagnostics::record(format!("manage team: members failed: {error}"));
                MembersLoad::Failed("Couldn't load the members. Try again later.".to_owned())
            }
        };
        self.request_avatars(user_ids, cx);
        cx.notify();
    }

    pub fn close_manage_team(&mut self, cx: &mut Context<Self>) {
        if self.manage_team.take().is_some() {
            cx.emit(AppEvent::Sidebar);
            cx.notify();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MemberGroups, TeamMember, member_groups, team_member};

    fn member(name: &str, email: Option<&str>, owner: bool) -> TeamMember {
        TeamMember {
            user_id: Some(name.to_lowercase()),
            name: name.to_owned(),
            email: email.map(str::to_owned),
            owner,
        }
    }

    #[test]
    fn owners_come_first_and_each_group_is_sorted_by_name() {
        let members = [
            member("tobias", None, false),
            member("Mara", None, true),
            member("Anna", None, false),
            member("Bert", None, true),
        ];
        let groups = member_groups(&members, "");
        let names = |group: &[&TeamMember]| group.iter().map(|member| member.name.clone()).collect::<Vec<_>>();
        assert_eq!(names(&groups.owners), ["Bert", "Mara"]);
        assert_eq!(names(&groups.others), ["Anna", "tobias"]);
    }

    #[test]
    fn search_matches_name_or_email_ignoring_case() {
        let members = [
            member("Mara Lindqvist", Some("mara@example.com"), true),
            member("Jonas Ortega", Some("j.ortega@example.com"), false),
        ];
        assert_eq!(member_groups(&members, "LIND").owners.len(), 1);
        assert_eq!(member_groups(&members, "ortega@").others.len(), 1);
        assert_eq!(member_groups(&members, "nobody"), MemberGroups::default());
    }

    #[test]
    fn graph_roles_mark_owners() {
        let graph_member = |roles: &[&str]| graph::Member {
            id: None,
            user_id: Some("u".to_owned()),
            tenant_id: None,
            display_name: Some("Name".to_owned()),
            email: Some(String::new()),
            roles: roles.iter().map(|role| (*role).to_owned()).collect(),
        };
        assert!(team_member(graph_member(&["owner"])).owner);
        assert!(!team_member(graph_member(&["guest"])).owner);
        assert_eq!(team_member(graph_member(&[])).email, None);
    }
}
