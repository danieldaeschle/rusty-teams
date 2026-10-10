use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use gpui_kit::{Image, ImageFormat};
use store::{ChatRecord, Store};

use teams_core::{Activity, Availability, ChatSectionSettings};
use tokio::sync::oneshot;

use crate::backend::Engine;
use crate::runtime;

const FAVORITES_KIND_MARKER: &str = "favorite";
const RECENT_KIND_MARKER: &str = "recent";
const MEETING_KIND_MARKER: &str = "meeting";
const MUTED_KIND_MARKER: &str = "muted";
const PRESENCE_MAX_AGE: Duration = Duration::from_secs(60);
const PUSHED_PRESENCE_MAX_AGE: Duration = Duration::from_secs(5 * 60);
const MEMBER_FACE_LIMIT: usize = 2;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub user_id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderKind {
    Favorites,
    UserCreated,
    Recent,
    Meeting,
    Muted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderInfo {
    pub id: String,
    pub name: String,
    pub kind: FolderKind,
    pub expanded: Option<bool>,
    pub conversation_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceKind {
    Available,
    Busy,
    DoNotDisturb,
    Away,
    Offline,
    Unknown,
}

impl PresenceKind {
    pub fn label(self) -> &'static str {
        match self {
            PresenceKind::Available => "Available",
            PresenceKind::Busy => "Busy",
            PresenceKind::DoNotDisturb => "Do not disturb",
            PresenceKind::Away => "Away",
            PresenceKind::Offline => "Offline",
            PresenceKind::Unknown => "",
        }
    }

    fn code(self) -> &'static str {
        match self {
            PresenceKind::Available => "Available",
            PresenceKind::Busy => "Busy",
            PresenceKind::DoNotDisturb => "DoNotDisturb",
            PresenceKind::Away => "Away",
            PresenceKind::Offline => "Offline",
            PresenceKind::Unknown => "Unknown",
        }
    }

    fn from_code(code: &str) -> PresenceKind {
        match code {
            "Available" => PresenceKind::Available,
            "Busy" => PresenceKind::Busy,
            "DoNotDisturb" => PresenceKind::DoNotDisturb,
            "Away" => PresenceKind::Away,
            "Offline" => PresenceKind::Offline,
            _ => PresenceKind::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presence {
    Live(PresenceKind),
    Cached(PresenceKind),
    Loading,
}

impl Presence {
    pub fn kind(self) -> PresenceKind {
        match self {
            Presence::Live(kind) | Presence::Cached(kind) => kind,
            Presence::Loading => PresenceKind::Unknown,
        }
    }
}

#[derive(Clone)]
pub enum AvatarState {
    Pending,
    Missing,
    Ready(Arc<Image>),
}

#[derive(Debug, Clone)]
pub struct ImageEntry {
    pub path: PathBuf,
    pub size: Option<(u32, u32)>,
}

#[derive(Default)]
pub struct Directory {
    pub me: Option<Person>,
    pub folders: Vec<FolderInfo>,
    pub section_settings: ChatSectionSettings,
    pub pinned_channels: Vec<String>,
    pub unread_counts: HashMap<String, u32>,
    pub(crate) avatars: HashMap<String, AvatarState>,
    pub(crate) presence: HashMap<String, (PresenceKind, Instant)>,
    pub(crate) activity: HashMap<String, Activity>,
    pub(crate) presence_cached: HashMap<String, PresenceKind>,
    pub(crate) presence_requested: HashMap<String, Instant>,
    pub(crate) presence_pending: HashSet<String>,
    pub(crate) images: HashMap<String, ImageEntry>,
    pub(crate) images_requested: HashSet<String>,
}

impl Directory {
    pub fn avatar(&self, user_id: &str) -> Option<&AvatarState> {
        self.avatars.get(user_id)
    }

    pub fn image(&self, key: &str) -> Option<&ImageEntry> {
        self.images.get(key)
    }

    pub fn set_image(&mut self, key: &str, path: &Path) {
        let size = image::image_dimensions(path).ok();
        self.images.insert(
            key.to_owned(),
            ImageEntry {
                path: path.to_path_buf(),
                size,
            },
        );
    }

    pub fn images_to_request(&mut self, keys: impl IntoIterator<Item = String>) -> Vec<String> {
        keys.into_iter()
            .filter(|key| {
                !self.images.contains_key(key) && self.images_requested.insert(key.clone())
            })
            .collect()
    }

    pub fn missing_avatars(&self, user_ids: impl IntoIterator<Item = String>) -> Vec<String> {
        let mut seen = HashSet::new();
        user_ids
            .into_iter()
            .filter(|user_id| !self.avatars.contains_key(user_id) && seen.insert(user_id.clone()))
            .collect()
    }

    pub fn mark_avatars_pending(&mut self, user_ids: &[String]) {
        for user_id in user_ids {
            self.avatars
                .entry(user_id.clone())
                .or_insert(AvatarState::Pending);
        }
    }

    pub fn set_avatar(&mut self, user_id: &str, image: Option<Arc<Image>>) {
        let state = image.map_or(AvatarState::Missing, AvatarState::Ready);
        self.avatars.insert(user_id.to_owned(), state);
    }

    pub fn pending_avatars(&self) -> Vec<String> {
        self.avatars
            .iter()
            .filter(|(_, state)| matches!(state, AvatarState::Pending))
            .map(|(user_id, _)| user_id.clone())
            .collect()
    }

    pub fn presence_of(&self, user_id: &str) -> Presence {
        if let Some((kind, _)) = self.presence.get(user_id) {
            return Presence::Live(*kind);
        }
        match self.presence_cached.get(user_id) {
            Some(kind) => Presence::Cached(*kind),
            None if self.presence_pending.contains(user_id) => Presence::Loading,
            None => Presence::Live(PresenceKind::Unknown),
        }
    }

    pub fn set_activity(&mut self, user_id: &str, activity: Option<Activity>) -> bool {
        match activity {
            Some(activity) => self.activity.insert(user_id.to_owned(), activity) != Some(activity),
            None => self.activity.remove(user_id).is_some(),
        }
    }

    pub fn status_label(&self, user_id: &str, presence: Presence) -> &'static str {
        match self.activity.get(user_id) {
            Some(activity) if matches!(presence, Presence::Live(_)) => activity_label(*activity),
            _ => presence.kind().label(),
        }
    }

    pub fn load_cached_presence(&mut self, store: &Store) {
        self.presence_cached = store
            .presences()
            .unwrap_or_default()
            .into_iter()
            .map(|(user_id, code)| (user_id, PresenceKind::from_code(&code)))
            .collect();
    }

    pub fn save_presence(&self, store: &Store, user_ids: &[String]) {
        let rows: Vec<(String, String)> = user_ids
            .iter()
            .filter_map(|user_id| {
                let (kind, _) = self.presence.get(user_id)?;
                Some((user_id.clone(), kind.code().to_owned()))
            })
            .collect();
        if !rows.is_empty() {
            let _ = store.upsert_presences(&rows, chrono::Utc::now());
        }
    }

    pub fn mark_presence_pending(&mut self, user_ids: &[String]) {
        self.presence_pending.extend(user_ids.iter().cloned());
    }

    pub fn settle_presence(&mut self, user_ids: &[String]) {
        for user_id in user_ids {
            self.presence_pending.remove(user_id);
        }
    }

    pub fn waiting_presence_ids(&self) -> Vec<String> {
        self.presence_pending.iter().cloned().collect()
    }

    pub fn set_presence(&mut self, user_id: &str, kind: PresenceKind) -> bool {
        self.presence
            .insert(user_id.to_owned(), (kind, Instant::now()))
            .is_none_or(|(previous, _)| previous != kind)
    }

    pub fn stale_presence(
        &self,
        user_ids: impl IntoIterator<Item = String>,
        now: Instant,
        pushed: bool,
    ) -> Vec<String> {
        let max_age = if pushed {
            PUSHED_PRESENCE_MAX_AGE
        } else {
            PRESENCE_MAX_AGE
        };
        let mut seen = HashSet::new();
        user_ids
            .into_iter()
            .filter(|user_id| {
                let fresh = |at: &Instant| now.duration_since(*at) < max_age;
                let known_fresh = self.presence.get(user_id).is_some_and(|(_, at)| fresh(at));
                let asked_fresh = self.presence_requested.get(user_id).is_some_and(fresh);
                !known_fresh && !asked_fresh && seen.insert(user_id.clone())
            })
            .collect()
    }

    pub fn requested_presence_ids(&self) -> Vec<String> {
        self.presence_requested.keys().cloned().collect()
    }

    pub fn mark_presence_requested(&mut self, user_ids: &[String], now: Instant) {
        for user_id in user_ids {
            self.presence_requested.insert(user_id.clone(), now);
        }
    }

    pub fn favorites(&self) -> Option<&FolderInfo> {
        self.folders
            .iter()
            .find(|folder| folder.kind == FolderKind::Favorites)
    }

    pub fn folder_of(&self, conversation_id: &str) -> Option<&FolderInfo> {
        self.folders.iter().find(|folder| {
            folder
                .conversation_ids
                .iter()
                .any(|id| id == conversation_id)
        })
    }

    pub fn assign(&mut self, conversation_id: &str, folder_id: Option<&str>) {
        for folder in &mut self.folders {
            folder.conversation_ids.retain(|id| id != conversation_id);
        }
        if let Some(folder) =
            folder_id.and_then(|id| self.folders.iter_mut().find(|folder| folder.id == id))
        {
            folder.conversation_ids.push(conversation_id.to_owned());
        }
    }
}

pub fn image_format(content_type: &str) -> ImageFormat {
    let lowered = content_type.to_ascii_lowercase();
    if lowered.contains("png") {
        ImageFormat::Png
    } else if lowered.contains("webp") {
        ImageFormat::Webp
    } else if lowered.contains("gif") {
        ImageFormat::Gif
    } else if lowered.contains("svg") {
        ImageFormat::Svg
    } else {
        ImageFormat::Jpeg
    }
}

pub fn is_one_on_one(chat: &ChatRecord) -> bool {
    chat.kind.eq_ignore_ascii_case("oneOnOne")
}

pub fn others(chat: &ChatRecord, me: Option<&Person>) -> Vec<(Option<String>, String)> {
    chat.members
        .iter()
        .filter(|member| match (&member.user_id, me) {
            (Some(user_id), Some(me)) => *user_id != me.user_id,
            _ => true,
        })
        .map(|member| (member.user_id.clone(), member.display_name.clone()))
        .collect()
}

pub fn face_members(chat: &ChatRecord, me: Option<&Person>) -> Vec<(Option<String>, String)> {
    others(chat, me)
        .into_iter()
        .take(MEMBER_FACE_LIMIT)
        .collect()
}

pub fn me(store: &Store) -> Option<Person> {
    let user_id = store.meta("me_user_id").ok().flatten()?;
    let display_name = store
        .meta("me_display_name")
        .ok()
        .flatten()
        .unwrap_or_default();
    Some(Person {
        user_id,
        display_name,
    })
}

fn folder_kind(kind: &str) -> FolderKind {
    let lowered = kind.to_ascii_lowercase();
    if lowered.contains(FAVORITES_KIND_MARKER) {
        FolderKind::Favorites
    } else if lowered.contains(RECENT_KIND_MARKER) {
        FolderKind::Recent
    } else if lowered.contains(MEETING_KIND_MARKER) {
        FolderKind::Meeting
    } else if lowered.contains(MUTED_KIND_MARKER) {
        FolderKind::Muted
    } else {
        FolderKind::UserCreated
    }
}

pub fn folders(store: &Store, pinned_ids: &[String]) -> Vec<FolderInfo> {
    let stored: Vec<FolderInfo> = store
        .folders()
        .unwrap_or_default()
        .into_iter()
        .map(|record| FolderInfo {
            kind: folder_kind(&record.kind),
            expanded: Some(record.expanded),
            id: record.id,
            name: record.name,
            conversation_ids: record.conversation_ids,
        })
        .collect();
    if !stored.is_empty() {
        return stored;
    }
    vec![FolderInfo {
        id: "favorites".to_owned(),
        name: String::new(),
        kind: FolderKind::Favorites,
        expanded: None,
        conversation_ids: pinned_ids.to_vec(),
    }]
}

pub fn pinned_channels(store: &Store) -> Vec<String> {
    store.pinned_channel_ids().unwrap_or_default()
}

pub fn cached_avatar(store: &Store, user_id: &str) -> Option<Option<Arc<Image>>> {
    let record = store.avatar(user_id).ok().flatten()?;
    Some(
        record
            .bytes
            .map(|bytes| avatar_image_from(&record.content_type, bytes)),
    )
}

pub fn avatar_image_from(content_type: &str, bytes: Vec<u8>) -> Arc<Image> {
    match crate::avatar_image::circular_png(&bytes) {
        Some(masked) => Arc::new(Image::from_bytes(ImageFormat::Png, masked)),
        None => Arc::new(Image::from_bytes(image_format(content_type), bytes)),
    }
}

pub fn unread_count(directory: &Directory, chat: &ChatRecord) -> Option<u32> {
    directory.unread_counts.get(&chat.id).copied()
}

pub fn unread_counts(engine: &Engine, chats: &[ChatRecord]) -> HashMap<String, u32> {
    chats
        .iter()
        .filter(|chat| chat.unread)
        .map(|chat| (chat.id.clone(), engine.unread_count(&chat.id)))
        .collect()
}

pub fn presence_kind_of(availability: Availability) -> PresenceKind {
    match availability {
        Availability::Available => PresenceKind::Available,
        Availability::Busy => PresenceKind::Busy,
        Availability::DoNotDisturb => PresenceKind::DoNotDisturb,
        Availability::Away => PresenceKind::Away,
        Availability::Offline => PresenceKind::Offline,
        Availability::Unknown => PresenceKind::Unknown,
    }
}

pub fn activity_label(activity: Activity) -> &'static str {
    match activity {
        Activity::InACall => "In a call",
        Activity::InAConferenceCall | Activity::InAMeeting => "In a meeting",
        Activity::Presenting => "Presenting",
        Activity::OutOfOffice => "Out of office",
    }
}

pub fn presence_activity(engine: &Engine, user_id: &str) -> Option<Activity> {
    engine
        .presence(user_id)
        .and_then(|presence| presence.activity)
}

pub fn presence_kind(engine: &Engine, user_id: &str) -> Option<PresenceKind> {
    engine
        .presence(user_id)
        .map(|presence| presence_kind_of(presence.availability))
}

pub fn in_call(engine: &Engine, user_id: &str) -> bool {
    engine
        .presence(user_id)
        .is_some_and(|presence| presence.in_call())
}

pub type Done = oneshot::Receiver<Result<(), String>>;

fn run<F>(future: F) -> Done
where
    F: std::future::Future<Output = teams_core::Result<()>> + Send + 'static,
{
    runtime::spawn(async move { future.await.map_err(|error| error.to_string()) })
}

pub fn fetch_image(engine: &Arc<Engine>, image: teams_core::ImageRef) -> Done {
    let engine = engine.clone();
    run(async move { engine.fetch_image(&image).await.map(|_| ()) })
}

pub fn fetch_avatars(engine: &Arc<Engine>, user_ids: Vec<String>) -> Done {
    let engine = engine.clone();
    run(async move { engine.fetch_avatars(&user_ids).await })
}

pub fn refresh_presence(engine: &Arc<Engine>, user_ids: Vec<String>) -> Done {
    let engine = engine.clone();
    run(async move { engine.refresh_presence(&user_ids).await })
}

pub fn watch_presence(engine: &Arc<Engine>, user_ids: Vec<String>) -> Done {
    let engine = engine.clone();
    run(async move { engine.watch_presence(&user_ids).await })
}

pub fn move_to_folder(engine: &Arc<Engine>, conversation_id: &str, folder_id: &str) -> Done {
    let (engine, conversation_id, folder_id) = (
        engine.clone(),
        conversation_id.to_owned(),
        folder_id.to_owned(),
    );
    run(async move { engine.move_to_folder(&conversation_id, &folder_id).await })
}

pub fn remove_from_folder(engine: &Arc<Engine>, conversation_id: &str, folder_id: &str) -> Done {
    let (engine, conversation_id, folder_id) = (
        engine.clone(),
        conversation_id.to_owned(),
        folder_id.to_owned(),
    );
    run(async move {
        engine
            .remove_from_folder(&conversation_id, &folder_id)
            .await
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_wording_replaces_availability_only_for_live_presence() {
        let cases = [
            (Activity::InACall, "In a call"),
            (Activity::InAConferenceCall, "In a meeting"),
            (Activity::InAMeeting, "In a meeting"),
            (Activity::Presenting, "Presenting"),
            (Activity::OutOfOffice, "Out of office"),
        ];
        for (activity, label) in cases {
            let mut directory = Directory::default();
            directory.set_activity("ada", Some(activity));
            let live = Presence::Live(PresenceKind::Busy);
            assert_eq!(directory.status_label("ada", live), label);
            assert_eq!(
                directory.status_label("ada", Presence::Cached(PresenceKind::Busy)),
                "Busy"
            );
            assert_eq!(directory.status_label("bob", live), "Busy");
            assert!(directory.set_activity("ada", None));
            assert_eq!(directory.status_label("ada", live), "Busy");
        }
    }

    fn folder(id: &str, kind: FolderKind, ids: &[&str]) -> FolderInfo {
        FolderInfo {
            id: id.into(),
            name: id.into(),
            kind,
            expanded: None,
            conversation_ids: ids.iter().map(|id| (*id).to_owned()).collect(),
        }
    }

    #[test]
    fn assign_moves_a_chat_between_folders() {
        let mut directory = Directory {
            folders: vec![
                folder("fav", FolderKind::Favorites, &["a"]),
                folder("work", FolderKind::UserCreated, &["b"]),
            ],
            ..Default::default()
        };
        directory.assign("a", Some("work"));
        assert!(directory.favorites().unwrap().conversation_ids.is_empty());
        assert_eq!(directory.folders[1].conversation_ids, vec!["b", "a"]);
        directory.assign("a", None);
        assert!(directory.folder_of("a").is_none());
    }

    #[test]
    fn missing_avatars_skip_known_and_duplicates() {
        let mut directory = Directory::default();
        directory.set_avatar("known", None);
        let wanted = directory.missing_avatars(["known", "x", "x", "y"].map(String::from));
        assert_eq!(wanted, vec!["x", "y"]);
        directory.mark_avatars_pending(&wanted);
        assert!(directory.missing_avatars(["x".to_owned()]).is_empty());
        assert_eq!(directory.pending_avatars().len(), 2);
    }

    #[test]
    fn images_are_requested_once() {
        let mut directory = Directory::default();
        let wanted = directory.images_to_request(["a", "a", "b"].map(String::from));
        assert_eq!(wanted, vec!["a", "b"]);
        assert!(directory.images_to_request(["a".to_owned()]).is_empty());
    }

    #[test]
    fn presence_is_refetched_after_the_max_age() {
        let mut directory = Directory::default();
        let start = Instant::now();
        let wanted = directory.stale_presence(["u".to_owned()], start, false);
        assert_eq!(wanted, vec!["u"]);
        directory.mark_presence_requested(&wanted, start);
        assert!(
            directory
                .stale_presence(["u".to_owned()], start, false)
                .is_empty()
        );
        let later = start + PRESENCE_MAX_AGE + Duration::from_secs(1);
        assert_eq!(
            directory.stale_presence(["u".to_owned()], later, false),
            vec!["u"]
        );
        assert!(
            directory
                .stale_presence(["u".to_owned()], later, true)
                .is_empty()
        );
        let much_later = start + PUSHED_PRESENCE_MAX_AGE + Duration::from_secs(1);
        assert_eq!(
            directory.stale_presence(["u".to_owned()], much_later, true),
            vec!["u"]
        );
        assert_eq!(directory.presence_of("u").kind(), PresenceKind::Unknown);
    }

    #[test]
    fn presence_prefers_live_over_cached_over_loading() {
        let mut directory = Directory::default();
        let ids = ["a".to_owned(), "b".to_owned(), "c".to_owned()];
        directory.mark_presence_pending(&ids);
        directory
            .presence_cached
            .insert("b".into(), PresenceKind::Away);
        directory
            .presence_cached
            .insert("c".into(), PresenceKind::Away);
        directory.set_presence("c", PresenceKind::Busy);
        assert_eq!(directory.presence_of("a"), Presence::Loading);
        assert_eq!(
            directory.presence_of("b"),
            Presence::Cached(PresenceKind::Away)
        );
        assert_eq!(
            directory.presence_of("c"),
            Presence::Live(PresenceKind::Busy)
        );
        directory.settle_presence(&ids);
        assert_eq!(
            directory.presence_of("a"),
            Presence::Live(PresenceKind::Unknown)
        );
    }

    #[test]
    fn presence_survives_a_restart_through_the_store() {
        let store = Store::open_in_memory().unwrap();
        let mut directory = Directory::default();
        directory.set_presence("a", PresenceKind::DoNotDisturb);
        directory.save_presence(&store, &["a".to_owned(), "missing".to_owned()]);
        let mut restarted = Directory::default();
        restarted.load_cached_presence(&store);
        assert_eq!(
            restarted.presence_of("a"),
            Presence::Cached(PresenceKind::DoNotDisturb)
        );
        assert_eq!(
            restarted.presence_of("missing"),
            Presence::Live(PresenceKind::Unknown)
        );
    }

    #[test]
    fn content_types_map_to_image_formats() {
        assert_eq!(image_format("image/png"), ImageFormat::Png);
        assert_eq!(image_format("image/jpeg"), ImageFormat::Jpeg);
        assert_eq!(image_format(""), ImageFormat::Jpeg);
    }
}
