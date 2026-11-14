//! The InnerTube request manager for the media library. Every request gets a number, and its
//! answer is matched back to what asked for it.
//!
//! Requests belong to a slot: the main list, the nav column's playlists, or a radio. Starting a
//! request for a slot makes the slot's earlier answers stale, so an answer to an old search (or to
//! a page the user has left) is dropped when it arrives. A continuation is the exception: it
//! extends the page on show rather than replacing it. Requests made before the page is ready wait
//! in a queue and go out when `Evt::Ready` arrives.

use std::collections::HashMap;

use tubeamp_bridge::Cmd;
use tubeamp_ytm::Request;

/// A group of requests whose answers replace each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Slot {
    /// The main list: search, home, library, liked, an opened album or playlist, and continuations.
    Main,
    /// The playlists under the nav column's caption.
    Nav,
    /// "Start radio".
    Radio,
}

/// What an answer is for, so the library knows how to read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Search,
    Home,
    /// The library's playlists, for the main list.
    Library,
    Liked,
    /// An album or a playlist page.
    Page,
    /// The next page of whatever the main list shows.
    More,
    /// The library's playlists, for the nav column.
    NavPlaylists,
    /// The up-next queue of a track.
    Radio,
}

impl Purpose {
    /// The slot this purpose's requests belong to.
    pub fn slot(self) -> Slot {
        match self {
            Purpose::NavPlaylists => Slot::Nav,
            Purpose::Radio => Slot::Radio,
            _ => Slot::Main,
        }
    }

    /// Requests that only work for a signed-in user.
    pub fn needs_sign_in(self) -> bool {
        matches!(
            self,
            Purpose::Library | Purpose::Liked | Purpose::NavPlaylists
        )
    }
}

/// A request sent (or queued) and waiting for its answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Pending {
    slot: Slot,
    generation: u64,
    purpose: Purpose,
}

/// What `on_ready` releases: the requests to send now, and the purposes it refused (queued
/// requests that need a signed-in user when the page says there is none).
#[derive(Debug, Default, PartialEq)]
pub struct Released {
    pub cmds: Vec<Cmd>,
    pub refused: Vec<Purpose>,
}

/// Numbers requests, matches answers, and holds requests until the page is ready.
#[derive(Debug, Default)]
pub struct ApiManager {
    next_id: u64,
    ready: bool,
    signed_in: bool,
    generations: HashMap<Slot, u64>,
    /// Sent requests, by number.
    pending: HashMap<u64, Pending>,
    /// Requests made before `Evt::Ready`, with the request body they will send.
    queued: Vec<(u64, Pending, Request)>,
}

impl ApiManager {
    pub fn new() -> ApiManager {
        ApiManager {
            next_id: 1,
            ..ApiManager::default()
        }
    }

    /// Whether the page has reported that InnerTube requests can run.
    pub fn ready(&self) -> bool {
        self.ready
    }

    /// Whether the page reported a signed-in user. Only meaningful once [`ApiManager::ready`].
    pub fn signed_in(&self) -> bool {
        self.signed_in
    }

    /// Starts a request for `purpose`. Returns the command to send now, or `None` when the page
    /// is not ready yet and the request waits in the queue.
    pub fn start(&mut self, purpose: Purpose, request: Request) -> Option<Cmd> {
        let slot = purpose.slot();
        if purpose != Purpose::More {
            self.cancel(slot);
        }
        let id = self.next_id;
        self.next_id += 1;
        let pending = Pending {
            slot,
            generation: self.generation(slot),
            purpose,
        };
        if !self.ready {
            self.queued.push((id, pending, request));
            return None;
        }
        self.pending.insert(id, pending);
        Some(command(id, request))
    }

    /// The page is ready (`signed_in` as it reported). Releases the queue: requests that were
    /// superseded while queued are dropped, and sign-in requests are refused without a user.
    pub fn on_ready(&mut self, signed_in: bool) -> Released {
        self.ready = true;
        self.signed_in = signed_in;
        let mut released = Released::default();
        for (id, pending, request) in std::mem::take(&mut self.queued) {
            if pending.generation != self.generation(pending.slot) {
                continue;
            }
            if pending.purpose.needs_sign_in() && !signed_in {
                released.refused.push(pending.purpose);
                continue;
            }
            self.pending.insert(id, pending);
            released.cmds.push(command(id, request));
        }
        released
    }

    /// The purpose of answer `req`. `None` for an unknown request or a stale one. Either way the
    /// request is forgotten.
    pub fn take(&mut self, req: u64) -> Option<Purpose> {
        let pending = self.pending.remove(&req)?;
        (pending.generation == self.generation(pending.slot)).then_some(pending.purpose)
    }

    /// Makes every request in `slot` stale, so their answers are dropped.
    pub fn cancel(&mut self, slot: Slot) {
        *self.generations.entry(slot).or_insert(0) += 1;
    }

    fn generation(&self, slot: Slot) -> u64 {
        self.generations.get(&slot).copied().unwrap_or(0)
    }
}

fn command(id: u64, (endpoint, body): Request) -> Cmd {
    Cmd::Api {
        req: id,
        endpoint: endpoint.to_string(),
        body,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tubeamp_ytm as ytm;

    fn req_id(cmd: &Cmd) -> u64 {
        match cmd {
            Cmd::Api { req, .. } => *req,
            other => panic!("not an API command: {other:?}"),
        }
    }

    #[test]
    fn requests_wait_for_ready_and_go_out_in_order() {
        let mut api = ApiManager::new();
        assert!(api.start(Purpose::Home, ytm::home()).is_none());
        assert!(api.start(Purpose::Radio, ytm::next("abc")).is_none());
        let released = api.on_ready(false);
        assert_eq!(released.cmds.len(), 2);
        assert_eq!(
            released.cmds[0],
            Cmd::Api {
                req: 1,
                endpoint: "browse".into(),
                body: serde_json::json!({"browseId": "FEmusic_home"}),
            }
        );
        assert!(
            matches!(&released.cmds[1], Cmd::Api { req: 2, endpoint, .. } if endpoint == "next")
        );
        assert!(released.refused.is_empty());
    }

    #[test]
    fn a_new_search_makes_the_old_answer_stale() {
        let mut api = ApiManager::new();
        api.on_ready(true);
        let old = req_id(&api.start(Purpose::Search, ytm::search("a", None)).unwrap());
        let new = req_id(&api.start(Purpose::Search, ytm::search("b", None)).unwrap());
        assert_eq!(api.take(old), None, "the old search is stale");
        assert_eq!(api.take(new), Some(Purpose::Search));
        assert_eq!(api.take(new), None, "an answer is taken once");
    }

    #[test]
    fn a_continuation_does_not_replace_its_page() {
        let mut api = ApiManager::new();
        api.on_ready(true);
        let page = req_id(&api.start(Purpose::Home, ytm::home()).unwrap());
        let more = req_id(&api.start(Purpose::More, ytm::continuation("tok")).unwrap());
        assert_eq!(api.take(page), Some(Purpose::Home));
        assert_eq!(api.take(more), Some(Purpose::More));
    }

    #[test]
    fn leaving_a_page_drops_its_continuation() {
        let mut api = ApiManager::new();
        api.on_ready(true);
        let more = req_id(&api.start(Purpose::More, ytm::continuation("tok")).unwrap());
        api.cancel(Slot::Main);
        assert_eq!(api.take(more), None);
    }

    #[test]
    fn other_slots_are_not_affected_by_a_main_request() {
        let mut api = ApiManager::new();
        api.on_ready(true);
        let radio = req_id(&api.start(Purpose::Radio, ytm::next("abc")).unwrap());
        let _ = api.start(Purpose::Search, ytm::search("x", None));
        assert_eq!(api.take(radio), Some(Purpose::Radio));
    }

    #[test]
    fn queued_requests_superseded_before_ready_are_dropped() {
        let mut api = ApiManager::new();
        api.start(Purpose::Home, ytm::home());
        api.start(Purpose::Search, ytm::search("x", None));
        let released = api.on_ready(true);
        assert_eq!(released.cmds.len(), 1, "only the search is still wanted");
    }

    #[test]
    fn sign_in_requests_are_refused_without_a_user() {
        let mut api = ApiManager::new();
        api.start(Purpose::Library, ytm::library_playlists());
        api.start(Purpose::NavPlaylists, ytm::library_playlists());
        api.start(Purpose::Radio, ytm::next("abc"));
        let released = api.on_ready(false);
        assert_eq!(released.cmds.len(), 1, "the radio request needs no sign-in");
        assert_eq!(
            released.refused,
            vec![Purpose::Library, Purpose::NavPlaylists]
        );
        assert!(!api.signed_in());
    }

    #[test]
    fn a_newer_main_request_drops_a_queued_older_one_without_refusing_it() {
        let mut api = ApiManager::new();
        api.start(Purpose::Library, ytm::library_playlists());
        api.start(Purpose::Home, ytm::home());
        let released = api.on_ready(false);
        assert_eq!(released.cmds.len(), 1);
        assert!(
            released.refused.is_empty(),
            "the library request was superseded, not refused"
        );
    }

    #[test]
    fn unknown_requests_have_no_purpose() {
        let mut api = ApiManager::new();
        api.on_ready(true);
        assert_eq!(api.take(99), None);
    }
}
