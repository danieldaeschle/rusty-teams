#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatPaging {
    next_link: Option<String>,
    exhausted: bool,
    loading: bool,
    epoch: u64,
}

impl ChatPaging {
    pub fn restored(next_link: Option<String>, exhausted: bool) -> Self {
        ChatPaging {
            next_link,
            exhausted,
            ..ChatPaging::default()
        }
    }

    pub fn is_known(&self) -> bool {
        self.next_link.is_some() || self.exhausted
    }

    pub fn has_more(&self) -> bool {
        self.next_link.is_some()
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    pub fn next_link(&self) -> Option<&str> {
        self.next_link.as_deref()
    }

    pub fn is_exhausted(&self) -> bool {
        self.exhausted
    }

    pub fn is_current(&self, epoch: u64) -> bool {
        self.epoch == epoch
    }

    pub fn begin(&mut self) -> Option<(String, u64)> {
        if self.loading {
            return None;
        }
        let link = self.next_link.clone()?;
        self.loading = true;
        Some((link, self.epoch))
    }

    pub fn finish(&mut self, epoch: u64, next_link: Option<String>) {
        self.loading = false;
        if self.is_current(epoch) {
            self.exhausted = next_link.is_none();
            self.next_link = next_link;
        }
    }

    pub fn fail(&mut self, epoch: u64) {
        self.loading = false;
        if self.is_current(epoch) {
            self.next_link = None;
            self.exhausted = false;
        }
    }

    pub fn adopt_continuation(&mut self, next_link: Option<String>) {
        let before = (self.next_link.clone(), self.exhausted);
        match next_link {
            Some(link) if self.next_link.is_none() => {
                self.next_link = Some(link);
                self.exhausted = false;
            }
            Some(_) => {}
            None => {
                self.next_link = None;
                self.exhausted = true;
            }
        }
        if before != (self.next_link.clone(), self.exhausted) {
            self.epoch += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_link(link: &str) -> ChatPaging {
        ChatPaging::restored(Some(link.to_owned()), false)
    }

    #[test]
    fn a_fresh_state_knows_nothing_and_loads_nothing() {
        let mut paging = ChatPaging::default();
        assert!(!paging.is_known());
        assert!(!paging.has_more());
        assert_eq!(paging.begin(), None);
    }

    #[test]
    fn begin_hands_out_the_link_once_while_a_page_is_in_flight() {
        let mut paging = with_link("page2");
        assert_eq!(paging.begin(), Some(("page2".to_owned(), 0)));
        assert!(paging.is_loading());
        assert_eq!(paging.begin(), None);
    }

    #[test]
    fn finish_with_a_link_allows_the_next_page() {
        let mut paging = with_link("page2");
        let (_, epoch) = paging.begin().unwrap();
        paging.finish(epoch, Some("page3".to_owned()));
        assert!(!paging.is_loading());
        assert!(paging.has_more());
        assert_eq!(paging.begin(), Some(("page3".to_owned(), 0)));
    }

    #[test]
    fn finish_without_a_link_is_done_for_good() {
        let mut paging = with_link("page2");
        let (_, epoch) = paging.begin().unwrap();
        paging.finish(epoch, None);
        assert!(!paging.has_more());
        assert!(paging.is_exhausted());
        assert!(paging.is_known());
        assert_eq!(paging.begin(), None);
    }

    #[test]
    fn a_failed_page_drops_the_link_so_the_next_refresh_walks_again() {
        let mut paging = with_link("page2");
        let (_, epoch) = paging.begin().unwrap();
        paging.fail(epoch);
        assert!(!paging.is_loading());
        assert!(!paging.has_more());
        assert!(!paging.is_exhausted());
        assert!(!paging.is_known());
        assert_eq!(paging.begin(), None);
        paging.adopt_continuation(Some("fresh".to_owned()));
        assert_eq!(paging.next_link(), Some("fresh"));
    }

    #[test]
    fn adopting_a_continuation_keeps_deeper_progress() {
        let mut paging = with_link("deep");
        paging.adopt_continuation(Some("shallow".to_owned()));
        assert_eq!(paging.next_link(), Some("deep"));

        let mut fresh = ChatPaging::default();
        fresh.adopt_continuation(Some("shallow".to_owned()));
        assert_eq!(fresh.next_link(), Some("shallow"));
    }

    #[test]
    fn adopting_a_complete_walk_marks_everything_loaded() {
        let mut paging = with_link("deep");
        paging.adopt_continuation(None);
        assert!(!paging.has_more());
        assert!(paging.is_exhausted());
    }

    #[test]
    fn a_result_from_before_a_refresh_changed_the_state_is_ignored() {
        let mut paging = with_link("page2");
        let (_, epoch) = paging.begin().unwrap();
        paging.adopt_continuation(None);
        assert!(!paging.is_current(epoch));
        paging.finish(epoch, Some("stale".to_owned()));
        assert!(!paging.is_loading());
        assert_eq!(paging.next_link(), None);
        assert!(paging.is_exhausted());
    }

    #[test]
    fn an_unchanged_adoption_keeps_the_page_in_flight_valid() {
        let mut paging = with_link("deep");
        let (_, epoch) = paging.begin().unwrap();
        paging.adopt_continuation(Some("shallow".to_owned()));
        assert!(paging.is_current(epoch));
    }
}
