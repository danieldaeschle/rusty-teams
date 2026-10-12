use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const GALLERY_MAX: usize = 9;
pub const FILMSTRIP_MAX: usize = 6;
pub const FILMSTRIP_MIN_WIDTH: f32 = 160.;
pub const SWAP_AFTER_SPEAKING: Duration = Duration::from_secs(2);
pub const SWAP_INTERVAL: Duration = Duration::from_secs(5);
pub const SPEAKER_HOLD: Duration = Duration::from_millis(1500);
pub const SPEAKER_SWITCH_AFTER: Duration = Duration::from_millis(800);
pub const RECENT_SPEAKER: Duration = Duration::from_secs(3);
const TILE_ASPECT: f32 = 16. / 9.;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GalleryGrid {
    pub columns: usize,
    pub tile_width: f32,
    pub tile_height: f32,
}

pub fn gallery_grid(cells: usize, stage_width: f32, stage_height: f32, gap: f32) -> GalleryGrid {
    let cells = cells.max(1);
    let mut best = GalleryGrid {
        columns: 1,
        tile_width: 0.,
        tile_height: 0.,
    };
    for columns in 1..=cells {
        let rows = cells.div_ceil(columns);
        let cell_width = (stage_width - gap * (columns - 1) as f32) / columns as f32;
        let cell_height = (stage_height - gap * (rows - 1) as f32) / rows as f32;
        let tile_width = cell_width.min(cell_height * TILE_ASPECT).max(0.);
        if tile_width > best.tile_width {
            best = GalleryGrid {
                columns,
                tile_width,
                tile_height: tile_width / TILE_ASPECT,
            };
        }
    }
    best
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overflow {
    pub shown: usize,
    pub hidden: usize,
}

pub fn split_overflow(others: usize, own_cells: usize, capacity: usize) -> Overflow {
    if others + own_cells <= capacity {
        return Overflow {
            shown: others,
            hidden: 0,
        };
    }
    let shown = capacity.saturating_sub(own_cells + 1);
    Overflow {
        shown,
        hidden: others - shown,
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Filmstrip {
    pub tile_width: f32,
    pub tile_height: f32,
    pub overflow: Overflow,
}

pub fn filmstrip(
    others: usize,
    own_cells: usize,
    stage_width: f32,
    gap: f32,
    max_tile_width: f32,
) -> Filmstrip {
    let fitting = ((stage_width + gap) / (FILMSTRIP_MIN_WIDTH + gap)) as usize;
    let capacity = FILMSTRIP_MAX.min(fitting.max(1));
    let overflow = split_overflow(others, own_cells, capacity);
    let cells = (overflow.shown + own_cells + usize::from(overflow.hidden > 0)).max(1);
    let tile_width = ((stage_width - gap * (cells - 1) as f32) / cells as f32)
        .clamp(FILMSTRIP_MIN_WIDTH, max_tile_width);
    Filmstrip {
        tile_width,
        tile_height: tile_width / TILE_ASPECT,
        overflow,
    }
}

#[derive(Debug, Clone, Default)]
pub struct VisibleRotation {
    order: Vec<String>,
    speaking_since: HashMap<String, Instant>,
    last_spoke: HashMap<String, Instant>,
    last_swap: Option<Instant>,
}

impl VisibleRotation {
    pub fn order(&self) -> &[String] {
        &self.order
    }

    pub fn last_speaker(&self) -> Option<&str> {
        self.order
            .iter()
            .filter_map(|mri| Some((mri, self.last_spoke.get(mri)?)))
            .max_by_key(|(_, spoke)| **spoke)
            .map(|(mri, _)| mri.as_str())
    }

    pub fn spoke_within(&self, mri: &str, now: Instant, window: Duration) -> bool {
        self.last_spoke
            .get(mri)
            .is_some_and(|spoke| now.saturating_duration_since(*spoke) <= window)
    }

    pub fn steady_speaker(&self, now: Instant, members: &[String]) -> Option<&str> {
        self.speaking_since
            .iter()
            .filter(|(mri, since)| {
                members.contains(mri) && now.saturating_duration_since(**since) >= SPEAKER_SWITCH_AFTER
            })
            .min_by_key(|(_, since)| **since)
            .map(|(mri, _)| mri.as_str())
    }

    pub fn update(
        &mut self,
        now: Instant,
        members: &[String],
        priority: &[String],
        speaking: &[String],
        capacity: usize,
    ) -> bool {
        let before = self.order.clone();
        self.track_speakers(now, speaking);
        self.order.retain(|mri| members.contains(mri));
        let mut arrivals: Vec<(usize, &String)> = members
            .iter()
            .enumerate()
            .filter(|(_, mri)| !self.order.contains(mri))
            .collect();
        arrivals.sort_by_key(|(index, mri)| {
            (
                !priority.contains(mri),
                std::cmp::Reverse(self.last_spoke.get(*mri).copied()),
                *index,
            )
        });
        self.order
            .extend(arrivals.into_iter().map(|(_, mri)| mri.clone()));
        if capacity > 0 && capacity < self.order.len() {
            self.pull_in_priority(priority, capacity);
            self.pull_in_speaker(now, priority, capacity);
        }
        self.order != before
    }

    fn track_speakers(&mut self, now: Instant, speaking: &[String]) {
        self.speaking_since.retain(|mri, _| speaking.contains(mri));
        for mri in speaking {
            self.speaking_since.entry(mri.clone()).or_insert(now);
            self.last_spoke.insert(mri.clone(), now);
        }
    }

    fn pull_in_priority(&mut self, priority: &[String], capacity: usize) {
        for mri in priority {
            if let Some(index) = self.order.iter().position(|other| other == mri)
                && index >= capacity
            {
                self.swap_in(index, priority, capacity);
            }
        }
    }

    fn pull_in_speaker(&mut self, now: Instant, priority: &[String], capacity: usize) {
        let rested = self
            .last_swap
            .is_none_or(|swapped| now.saturating_duration_since(swapped) >= SWAP_INTERVAL);
        if !rested {
            return;
        }
        let candidate = self.order[capacity..]
            .iter()
            .enumerate()
            .filter_map(|(offset, mri)| Some((capacity + offset, *self.speaking_since.get(mri)?)))
            .filter(|(_, since)| now.saturating_duration_since(*since) >= SWAP_AFTER_SPEAKING)
            .min_by_key(|(_, since)| *since);
        if let Some((index, _)) = candidate
            && self.swap_in(index, priority, capacity)
        {
            self.last_swap = Some(now);
        }
    }

    fn swap_in(&mut self, index: usize, priority: &[String], capacity: usize) -> bool {
        let victim = self.order[..capacity]
            .iter()
            .enumerate()
            .filter(|(_, mri)| !priority.contains(mri))
            .min_by_key(|(position, mri)| {
                (
                    self.last_spoke.get(*mri).copied(),
                    std::cmp::Reverse(*position),
                )
            })
            .map(|(position, _)| position);
        let Some(victim) = victim else {
            return false;
        };
        self.order.swap(victim, index);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(count: usize) -> Vec<String> {
        (0..count).map(|index| format!("p{index}")).collect()
    }

    fn visible(rotation: &VisibleRotation, capacity: usize) -> Vec<String> {
        rotation.order()[..capacity].to_vec()
    }

    fn speak(mris: &[&str]) -> Vec<String> {
        mris.iter().map(|mri| (*mri).to_owned()).collect()
    }

    #[test]
    fn steady_speaker_needs_a_moment_of_speech_and_stays_recent_after() {
        let start = Instant::now();
        let members = names(4);
        let mut rotation = VisibleRotation::default();
        rotation.update(start, &members, &[], &speak(&["p2"]), 4);
        assert_eq!(rotation.steady_speaker(start + Duration::from_millis(300), &members), None);
        rotation.update(start + SPEAKER_SWITCH_AFTER, &members, &[], &speak(&["p2"]), 4);
        assert_eq!(rotation.steady_speaker(start + SPEAKER_SWITCH_AFTER, &members), Some("p2"));
        let silent = start + SPEAKER_SWITCH_AFTER + Duration::from_secs(1);
        rotation.update(silent, &members, &[], &[], 4);
        assert!(rotation.spoke_within("p2", silent, SPEAKER_HOLD));
        assert!(!rotation.spoke_within("p2", silent + RECENT_SPEAKER, RECENT_SPEAKER));
    }

    #[test]
    fn a_single_tile_takes_the_whole_stage() {
        let grid = gallery_grid(1, 1600., 900., 20.);
        assert_eq!(grid.columns, 1);
        assert!((grid.tile_width - 1600.).abs() < 0.01);
    }

    #[test]
    fn two_tiles_sit_side_by_side_on_a_wide_stage() {
        let grid = gallery_grid(2, 1600., 900., 20.);
        assert_eq!(grid.columns, 2);
        assert_eq!(grid.tile_width, 790.);
    }

    #[test]
    fn nine_tiles_make_three_by_three_and_fit_the_stage() {
        let grid = gallery_grid(9, 1600., 900., 20.);
        assert_eq!(grid.columns, 3);
        assert!(grid.tile_width * 3. + 40. <= 1600.);
        assert!(grid.tile_height * 3. + 40. <= 900.);
    }

    #[test]
    fn a_tall_stage_prefers_fewer_columns() {
        let grid = gallery_grid(4, 500., 1200., 20.);
        assert_eq!(grid.columns, 1);
    }

    #[test]
    fn tiles_stay_sixteen_by_nine() {
        let grid = gallery_grid(7, 1234., 777., 20.);
        assert!((grid.tile_width / grid.tile_height - 16. / 9.).abs() < 0.001);
    }

    #[test]
    fn nine_people_fit_and_ten_end_in_a_plus_tile() {
        assert_eq!(
            split_overflow(8, 1, GALLERY_MAX),
            Overflow {
                shown: 8,
                hidden: 0
            }
        );
        assert_eq!(
            split_overflow(9, 1, GALLERY_MAX),
            Overflow {
                shown: 7,
                hidden: 2
            }
        );
        assert_eq!(
            split_overflow(12, 0, GALLERY_MAX),
            Overflow {
                shown: 8,
                hidden: 4
            }
        );
    }

    #[test]
    fn the_filmstrip_holds_six_tiles_then_a_plus_tile() {
        let strip = filmstrip(5, 1, 1600., 12., 240.);
        assert_eq!(strip.overflow.hidden, 0);
        let strip = filmstrip(6, 1, 1600., 12., 240.);
        assert_eq!(
            strip.overflow,
            Overflow {
                shown: 4,
                hidden: 2
            }
        );
        assert_eq!(strip.tile_width, 240.);
    }

    #[test]
    fn a_narrow_filmstrip_keeps_tiles_at_the_minimum_and_adds_the_plus_tile() {
        let strip = filmstrip(5, 1, 504., 12., 240.);
        assert_eq!(strip.tile_width, FILMSTRIP_MIN_WIDTH);
        assert_eq!(strip.tile_height, 90.);
        assert_eq!(
            strip.overflow,
            Overflow {
                shown: 1,
                hidden: 4
            }
        );
    }

    #[test]
    fn new_members_start_in_priority_order_and_stay_put() {
        let mut rotation = VisibleRotation::default();
        let now = Instant::now();
        let members = names(5);
        rotation.update(now, &members, &speak(&["p3"]), &[], 4);
        assert_eq!(rotation.order()[0], "p3");
        let before = visible(&rotation, 4);
        rotation.update(now, &members, &speak(&["p3"]), &[], 4);
        assert_eq!(visible(&rotation, 4), before);
    }

    #[test]
    fn an_outside_speaker_swaps_in_only_after_two_seconds_of_speech() {
        let mut rotation = VisibleRotation::default();
        let start = Instant::now();
        let members = names(6);
        rotation.update(start, &members, &[], &[], 4);
        let outside = "p5";
        rotation.update(start, &members, &[], &speak(&[outside]), 4);
        rotation.update(
            start + Duration::from_millis(1900),
            &members,
            &[],
            &speak(&[outside]),
            4,
        );
        assert!(!visible(&rotation, 4).contains(&outside.to_owned()));
        rotation.update(
            start + SWAP_AFTER_SPEAKING,
            &members,
            &[],
            &speak(&[outside]),
            4,
        );
        assert!(visible(&rotation, 4).contains(&outside.to_owned()));
    }

    #[test]
    fn interrupted_speech_restarts_the_two_second_clock() {
        let mut rotation = VisibleRotation::default();
        let start = Instant::now();
        let members = names(6);
        rotation.update(start, &members, &[], &[], 4);
        rotation.update(start, &members, &[], &speak(&["p5"]), 4);
        rotation.update(start + Duration::from_millis(1500), &members, &[], &[], 4);
        rotation.update(
            start + Duration::from_millis(2500),
            &members,
            &[],
            &speak(&["p5"]),
            4,
        );
        rotation.update(
            start + Duration::from_millis(3500),
            &members,
            &[],
            &speak(&["p5"]),
            4,
        );
        assert!(!visible(&rotation, 4).contains(&"p5".to_owned()));
    }

    #[test]
    fn the_least_recently_speaking_visible_tile_is_replaced_and_the_others_keep_their_slots() {
        let mut rotation = VisibleRotation::default();
        let start = Instant::now();
        let members = names(6);
        rotation.update(start, &members, &[], &[], 4);
        rotation.update(start, &members, &[], &speak(&["p0", "p1", "p3"]), 4);
        let before = visible(&rotation, 4);
        rotation.update(start, &members, &[], &speak(&["p5", "p0", "p1", "p3"]), 4);
        rotation.update(
            start + SWAP_AFTER_SPEAKING,
            &members,
            &[],
            &speak(&["p5", "p0", "p1", "p3"]),
            4,
        );
        let after = visible(&rotation, 4);
        let slot = before.iter().position(|mri| mri == "p2").unwrap();
        assert_eq!(after[slot], "p5");
        for (index, mri) in before
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != slot)
        {
            assert_eq!(&after[index], mri);
        }
    }

    #[test]
    fn at_most_one_speaker_swaps_in_per_five_seconds() {
        let mut rotation = VisibleRotation::default();
        let start = Instant::now();
        let members = names(7);
        rotation.update(start, &members, &[], &[], 4);
        let talking = speak(&["p5", "p6"]);
        rotation.update(start, &members, &[], &talking, 4);
        rotation.update(start + SWAP_AFTER_SPEAKING, &members, &[], &talking, 4);
        let swapped = visible(&rotation, 4)
            .iter()
            .filter(|mri| *mri == "p5" || *mri == "p6")
            .count();
        assert_eq!(swapped, 1);
        rotation.update(
            start + SWAP_AFTER_SPEAKING + Duration::from_secs(4),
            &members,
            &[],
            &talking,
            4,
        );
        let still = visible(&rotation, 4)
            .iter()
            .filter(|mri| *mri == "p5" || *mri == "p6")
            .count();
        assert_eq!(still, 1);
        rotation.update(
            start + SWAP_AFTER_SPEAKING + SWAP_INTERVAL,
            &members,
            &[],
            &talking,
            4,
        );
        let both = visible(&rotation, 4)
            .iter()
            .filter(|mri| *mri == "p5" || *mri == "p6")
            .count();
        assert_eq!(both, 2);
    }

    #[test]
    fn a_pinned_person_outside_the_grid_swaps_in_at_once_and_is_never_replaced() {
        let mut rotation = VisibleRotation::default();
        let start = Instant::now();
        let members = names(6);
        rotation.update(start, &members, &[], &[], 4);
        let pinned = speak(&["p5"]);
        rotation.update(start, &members, &pinned, &[], 4);
        assert!(visible(&rotation, 4).contains(&"p5".to_owned()));
        let talking = speak(&["p4"]);
        rotation.update(
            start + Duration::from_secs(1),
            &members,
            &pinned,
            &talking,
            4,
        );
        rotation.update(
            start + Duration::from_secs(4),
            &members,
            &pinned,
            &talking,
            4,
        );
        assert!(visible(&rotation, 4).contains(&"p5".to_owned()));
    }

    #[test]
    fn people_who_leave_drop_out_and_the_next_one_moves_up() {
        let mut rotation = VisibleRotation::default();
        let now = Instant::now();
        let members = names(5);
        rotation.update(now, &members, &[], &[], 3);
        let remaining: Vec<String> = members.into_iter().filter(|mri| mri != "p1").collect();
        rotation.update(now, &remaining, &[], &[], 3);
        assert_eq!(visible(&rotation, 3), vec!["p0", "p2", "p3"]);
    }
}
