use std::collections::{HashMap, VecDeque};

use agents::Invocation;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Lane {
    pub active: Vec<u64>,
    pub kills_pending: u32,
    pub waiting: VecDeque<u64>,
}

impl Lane {
    fn is_idle(&self) -> bool {
        self.active.is_empty() && self.kills_pending == 0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Admit {
    Launch,
    Queued,
}

pub struct RestoreInfo {
    pub id: u64,
    pub session_id: String,
    pub queued: bool,
    pub kill: Option<Invocation>,
}

pub struct Restored {
    pub lanes: Lanes,
    pub resume_queue: Vec<u64>,
    pub interrupted: Vec<u64>,
    pub kills: Vec<(String, Invocation)>,
}

#[derive(Debug, Default)]
pub struct Lanes {
    lanes: HashMap<String, Lane>,
    renamed: HashMap<String, String>,
}

impl Lanes {
    pub fn get(&self, session_id: &str) -> Option<&Lane> {
        self.lanes.get(session_id)
    }

    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }

    pub fn idle_sessions(&self) -> Vec<String> {
        self.lanes
            .iter()
            .filter(|(_, lane)| lane.is_idle())
            .map(|(session_id, _)| session_id.clone())
            .collect()
    }

    pub fn has_kills_pending(&self, session_id: &str) -> bool {
        self.lanes
            .get(session_id)
            .is_some_and(|lane| lane.kills_pending > 0)
    }

    pub fn admit(&mut self, session_id: &str, run_id: u64) -> Admit {
        let lane = self.lanes.entry(session_id.to_owned()).or_default();
        if lane.is_idle() && lane.waiting.is_empty() {
            lane.active.push(run_id);
            Admit::Launch
        } else {
            lane.waiting.push_back(run_id);
            Admit::Queued
        }
    }

    pub fn finished(&mut self, session_id: &str, run_id: u64) -> bool {
        let Some(lane) = self.lanes.get_mut(session_id) else {
            return false;
        };
        let before = lane.active.len();
        lane.active.retain(|active| *active != run_id);
        lane.active.len() != before
    }

    pub fn next(&mut self, session_id: &str, is_queued: impl Fn(u64) -> bool) -> Option<u64> {
        let lane = self.lanes.get_mut(session_id)?;
        if !lane.is_idle() {
            return None;
        }
        while let Some(candidate) = lane.waiting.pop_front() {
            if is_queued(candidate) {
                lane.active.push(candidate);
                return Some(candidate);
            }
        }
        self.lanes.remove(session_id);
        None
    }

    pub fn kill_started(&mut self, session_id: &str) {
        self.lanes
            .entry(session_id.to_owned())
            .or_default()
            .kills_pending += 1;
    }

    pub fn resolve(&self, session_id: &str) -> String {
        let mut current = session_id;
        for _ in 0..=self.renamed.len() {
            match self.renamed.get(current) {
                Some(next) => current = next,
                None => break,
            }
        }
        current.to_owned()
    }

    pub fn kill_done(&mut self, session_id: &str) {
        let session_id = self.resolve(session_id);
        if let Some(lane) = self.lanes.get_mut(&session_id) {
            lane.kills_pending = lane.kills_pending.saturating_sub(1);
        }
    }

    pub fn remove_waiting(&mut self, session_id: &str, run_id: u64) {
        if let Some(lane) = self.lanes.get_mut(session_id) {
            lane.waiting.retain(|waiting| *waiting != run_id);
        }
    }

    pub fn drain_waiting(&mut self, session_id: &str) -> Vec<u64> {
        self.lanes
            .get_mut(session_id)
            .map(|lane| lane.waiting.drain(..).collect())
            .unwrap_or_default()
    }

    pub fn rename(&mut self, from: &str, to: &str) -> Vec<u64> {
        let Some(lane) = self.lanes.remove(from) else {
            return Vec::new();
        };
        self.renamed.insert(from.to_owned(), to.to_owned());
        let moved: Vec<u64> = lane.waiting.iter().copied().collect();
        let target = self.lanes.entry(to.to_owned()).or_default();
        target.active.extend(lane.active);
        target.kills_pending += lane.kills_pending;
        target.waiting.extend(lane.waiting);
        moved
    }

    pub fn restore(infos: &[RestoreInfo], can_resume: impl Fn(u64) -> bool) -> Restored {
        let mut lanes = Lanes::default();
        let mut resume_queue = Vec::new();
        let mut interrupted = Vec::new();
        let mut kills = Vec::new();
        for info in infos.iter().filter(|info| !info.queued) {
            let has_active = lanes
                .get(&info.session_id)
                .is_some_and(|lane| !lane.active.is_empty());
            if !has_active && can_resume(info.id) {
                lanes.admit(&info.session_id, info.id);
                lanes.kill_started(&info.session_id);
                resume_queue.push(info.id);
            } else {
                interrupted.push(info.id);
                if let Some(kill) = &info.kill {
                    lanes.kill_started(&info.session_id);
                    kills.push((info.session_id.clone(), kill.clone()));
                }
            }
        }
        for info in infos.iter().filter(|info| info.queued) {
            lanes
                .lanes
                .entry(info.session_id.clone())
                .or_default()
                .waiting
                .push_back(info.id);
        }
        Restored {
            lanes,
            resume_queue,
            interrupted,
            kills,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(id: u64, session_id: &str, queued: bool, kill: bool) -> RestoreInfo {
        RestoreInfo {
            id,
            session_id: session_id.to_owned(),
            queued,
            kill: kill.then(|| Invocation {
                program: "kill".to_owned(),
                args: Vec::new(),
                cwd: None,
                stdin: None,
            }),
        }
    }

    fn waiting(lanes: &Lanes, session_id: &str) -> Vec<u64> {
        lanes
            .get(session_id)
            .unwrap()
            .waiting
            .iter()
            .copied()
            .collect()
    }

    #[test]
    fn admit_launches_on_an_idle_lane_and_queues_while_busy() {
        let mut lanes = Lanes::default();
        assert_eq!(lanes.admit("s", 1), Admit::Launch);
        assert_eq!(lanes.admit("s", 2), Admit::Queued);
        assert_eq!(lanes.admit("s", 3), Admit::Queued);
        assert_eq!(lanes.admit("other", 4), Admit::Launch);
        assert_eq!(lanes.get("s").unwrap().active, [1]);
        assert_eq!(waiting(&lanes, "s"), [2, 3]);
    }

    #[test]
    fn a_pending_kill_blocks_admission_and_release() {
        let mut lanes = Lanes::default();
        lanes.kill_started("s");
        assert_eq!(lanes.admit("s", 1), Admit::Queued);
        assert_eq!(lanes.next("s", |_| true), None);
        lanes.kill_done("s");
        assert_eq!(lanes.next("s", |_| true), Some(1));
    }

    #[test]
    fn waiting_runs_are_released_in_order_one_at_a_time() {
        let mut lanes = Lanes::default();
        lanes.admit("s", 1);
        lanes.admit("s", 2);
        lanes.admit("s", 3);
        assert_eq!(lanes.next("s", |_| true), None);
        assert!(lanes.finished("s", 1));
        assert!(!lanes.finished("s", 1));
        assert_eq!(lanes.next("s", |_| true), Some(2));
        assert_eq!(lanes.next("s", |_| true), None);
        assert!(lanes.finished("s", 2));
        assert_eq!(lanes.next("s", |_| true), Some(3));
        assert!(lanes.finished("s", 3));
        assert_eq!(lanes.next("s", |_| true), None);
        assert!(lanes.is_empty());
    }

    #[test]
    fn release_skips_runs_that_are_no_longer_queued() {
        let mut lanes = Lanes::default();
        lanes.admit("s", 1);
        lanes.admit("s", 2);
        lanes.admit("s", 3);
        lanes.finished("s", 1);
        assert_eq!(lanes.next("s", |id| id == 3), Some(3));
    }

    #[test]
    fn kills_are_counted_not_flagged() {
        let mut lanes = Lanes::default();
        lanes.kill_started("s");
        lanes.kill_started("s");
        lanes.admit("s", 1);
        lanes.kill_done("s");
        assert!(lanes.has_kills_pending("s"));
        assert_eq!(lanes.next("s", |_| true), None);
        lanes.kill_done("s");
        lanes.kill_done("s");
        assert!(!lanes.has_kills_pending("s"));
        assert_eq!(lanes.next("s", |_| true), Some(1));
    }

    #[test]
    fn removing_and_draining_waiting_runs() {
        let mut lanes = Lanes::default();
        for id in 1..=4 {
            lanes.admit("s", id);
        }
        lanes.remove_waiting("s", 3);
        assert_eq!(waiting(&lanes, "s"), [2, 4]);
        assert_eq!(lanes.drain_waiting("s"), [2, 4]);
        assert!(waiting(&lanes, "s").is_empty());
        assert!(lanes.drain_waiting("none").is_empty());
    }

    #[test]
    fn renaming_moves_active_and_waiting_runs() {
        let mut lanes = Lanes::default();
        lanes.admit("x", 1);
        lanes.admit("x", 2);
        assert_eq!(lanes.rename("x", "y"), [2]);
        assert!(lanes.get("x").is_none());
        assert_eq!(lanes.get("y").unwrap().active, [1]);
        assert_eq!(waiting(&lanes, "y"), [2]);
        assert!(lanes.rename("missing", "z").is_empty());
    }

    #[test]
    fn a_rename_collision_keeps_both_active_runs_blocking() {
        let mut lanes = Lanes::default();
        lanes.admit("x", 1);
        lanes.admit("x", 2);
        lanes.admit("y", 3);
        lanes.admit("y", 4);
        lanes.kill_started("x");
        assert_eq!(lanes.rename("x", "y"), [2]);
        let lane = lanes.get("y").unwrap();
        assert_eq!(lane.active, [3, 1]);
        assert_eq!(lane.kills_pending, 1);
        assert_eq!(waiting(&lanes, "y"), [4, 2]);
        lanes.kill_done("y");
        assert!(lanes.finished("y", 3));
        assert_eq!(lanes.next("y", |_| true), None);
        assert!(lanes.finished("y", 1));
        assert_eq!(lanes.next("y", |_| true), Some(4));
    }

    #[test]
    fn a_kill_pending_across_a_rename_is_released_under_the_old_id() {
        let mut lanes = Lanes::default();
        lanes.admit("x", 1);
        lanes.kill_started("x");
        lanes.rename("x", "y");
        assert_eq!(lanes.resolve("x"), "y");
        assert!(lanes.has_kills_pending("y"));
        lanes.kill_done("x");
        assert!(!lanes.has_kills_pending("y"));
    }

    #[test]
    fn restore_builds_active_candidates_waiting_order_and_kills() {
        let infos = [
            info(5, "shared", false, false),
            info(6, "shared", false, true),
            info(7, "shared", true, false),
            info(8, "shared", true, false),
            info(9, "dead", false, true),
            info(10, "other", false, false),
        ];
        let restored = Lanes::restore(&infos, |id| id != 9);
        assert_eq!(restored.resume_queue, [5, 10]);
        assert_eq!(restored.interrupted, [6, 9]);
        let sessions: Vec<&str> = restored
            .kills
            .iter()
            .map(|(session, _)| session.as_str())
            .collect();
        assert_eq!(sessions, ["shared", "dead"]);
        let lanes = restored.lanes;
        let shared = lanes.get("shared").unwrap();
        assert_eq!(shared.active, [5]);
        assert_eq!(shared.kills_pending, 2);
        assert_eq!(waiting(&lanes, "shared"), [7, 8]);
        assert_eq!(lanes.get("dead").unwrap().kills_pending, 1);
        assert_eq!(lanes.get("other").unwrap().kills_pending, 1);
        assert!(lanes.idle_sessions().is_empty());
    }

    #[test]
    fn a_restored_lane_with_only_waiting_runs_is_idle() {
        let restored = Lanes::restore(&[info(7, "s", true, false)], |_| true);
        assert_eq!(restored.lanes.idle_sessions(), ["s"]);
    }
}
