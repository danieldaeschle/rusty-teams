#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChatPaging {
    next_link: Option<String>,
    exhausted: bool,
    loading: bool,
    stalled: bool,
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

    pub fn begin(&mut self) -> Option<String> {
        if self.loading || self.stalled {
            return None;
        }
        let link = self.next_link.clone()?;
        self.loading = true;
        Some(link)
    }

    pub fn finish(&mut self, next_link: Option<String>) {
        self.loading = false;
        self.exhausted = next_link.is_none();
        self.next_link = next_link;
    }

    pub fn fail(&mut self) {
        self.loading = false;
        self.stalled = true;
    }

    pub fn retry(&mut self) {
        self.stalled = false;
    }

    pub fn adopt_continuation(&mut self, next_link: Option<String>) {
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
        assert_eq!(paging.begin(), Some("page2".to_owned()));
        assert!(paging.is_loading());
        assert_eq!(paging.begin(), None);
    }

    #[test]
    fn finish_with_a_link_allows_the_next_page() {
        let mut paging = with_link("page2");
        paging.begin();
        paging.finish(Some("page3".to_owned()));
        assert!(!paging.is_loading());
        assert!(paging.has_more());
        assert_eq!(paging.begin(), Some("page3".to_owned()));
    }

    #[test]
    fn finish_without_a_link_is_done_for_good() {
        let mut paging = with_link("page2");
        paging.begin();
        paging.finish(None);
        assert!(!paging.has_more());
        assert!(paging.is_exhausted());
        assert!(paging.is_known());
        assert_eq!(paging.begin(), None);
    }

    #[test]
    fn a_failed_page_stalls_until_the_next_refresh_retries() {
        let mut paging = with_link("page2");
        paging.begin();
        paging.fail();
        assert!(!paging.is_loading());
        assert!(paging.has_more());
        assert_eq!(paging.begin(), None);
        paging.retry();
        assert_eq!(paging.begin(), Some("page2".to_owned()));
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
}
