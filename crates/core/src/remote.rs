use chatsvc::sound_settings::{SoundSettings, SoundSettingsClient};
use chatsvc::substrate_search::{
    QueryRequest, QueryResults, SearchIdentity, SearchSession, SubstrateSearch, Suggestions,
};
use chatsvc::{
    ActivityFeed, AppConsumptionHorizon, MentionActivity, UserSettings,
    parse_app_consumption_horizon,
};
use chatsvc::{
    BotProfile, BotProfiles, CallLogEntry, CardActions, ChatApp, ConversationRef,
    ConversationState, Conversations, ForcedAvailability, ForwardResult, Gif, Gifs, HistoryShare,
    InvokeRequest, InvokeResponse, Language, LanguageSettings, LanguageSettingsClient, LinkInfo,
    MemberHorizon, MessageEmotions, MessageLanguage, MessageLinks, MessageWindow, Messages,
    PinnedMessage, Pins, PresenceLookup, PresenceService, PresenceStatus, Receipts,
    RecordingPolicy, SavedMessage, ScheduledDraft, ScheduledDrafts, StatusNote, ThreadMembers,
    TranslateRequest, Translation, TranslationTrigger, Translator, WindowDirection,
    WorkLocationKind,
};
use chrono::{DateTime, Utc};
use graph::{
    Channel, Chat, DriveEntry, DriveFolder, Graph, Member, Message, MessageExtras, MessageTarget,
    OutgoingMention, Photo, Presence, SharedFile, Team, UploadDestination, UploadedFile, User,
};

use crate::error::{Error, Result};
use crate::profile::PersonProfile;

pub const CHATSVC_CHAT_WRITES: bool = false;

#[derive(Debug, Clone)]
pub struct RemotePage {
    pub items: Vec<Message>,
    pub next_link: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DeltaPage {
    pub items: Vec<Message>,
    pub next_link: Option<String>,
    pub delta_link: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ChatsPage {
    pub items: Vec<Chat>,
    pub next_link: Option<String>,
}

#[allow(async_fn_in_trait)]
pub trait Remote {
    async fn me(&self) -> Result<User>;
    async fn chats_page(&self, top: usize) -> Result<ChatsPage>;
    async fn chats_at(&self, next_link: &str) -> Result<ChatsPage>;
    async fn chat(&self, chat_id: &str) -> Result<Chat>;
    async fn chat_members(&self, chat_id: &str) -> Result<Vec<Member>>;
    async fn joined_teams(&self) -> Result<Vec<Team>>;
    async fn channels_for_teams(&self, team_ids: &[String]) -> Result<Vec<Result<Vec<Channel>>>>;
    async fn chat_messages(
        &self,
        chat_id: &str,
        before: Option<DateTime<Utc>>,
        top: usize,
    ) -> Result<RemotePage>;
    async fn chat_message(&self, chat_id: &str, message_id: &str) -> Result<Message>;
    async fn channel_messages(
        &self,
        team_id: &str,
        channel_id: &str,
        top: usize,
    ) -> Result<RemotePage>;
    async fn channel_messages_at(&self, next_link: &str) -> Result<RemotePage>;
    async fn channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
    ) -> Result<Message>;
    async fn channel_reply(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
        reply_id: &str,
    ) -> Result<Message>;
    async fn channel_delta(
        &self,
        team_id: &str,
        channel_id: &str,
        modified_after: Option<DateTime<Utc>>,
    ) -> Result<DeltaPage>;
    async fn channel_delta_at(&self, link: &str) -> Result<DeltaPage>;
    async fn channel_replies(
        &self,
        team_id: &str,
        channel_id: &str,
        root_ids: &[String],
    ) -> Result<Vec<Result<Vec<Message>>>>;
    async fn send_chat_message(
        &self,
        chat_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<Message>;
    async fn send_channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        html: &str,
        subject: Option<&str>,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<Message>;
    async fn reply_to_channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<Message>;
    async fn mark_chat_read(&self, chat_id: &str, user_id: &str, tenant_id: &str) -> Result<()>;
    async fn mark_chat_unread(
        &self,
        chat_id: &str,
        user_id: &str,
        tenant_id: &str,
        last_read_at: DateTime<Utc>,
    ) -> Result<()>;
    async fn hide_chat(&self, chat_id: &str, user_id: &str, tenant_id: &str) -> Result<()>;
    async fn unhide_chat(&self, chat_id: &str, user_id: &str, tenant_id: &str) -> Result<()>;
    async fn remove_chat_member(&self, chat_id: &str, user_id: &str) -> Result<()>;
    async fn add_chat_members(
        &self,
        chat_id: &str,
        user_ids: &[String],
        share: HistoryShare,
    ) -> Result<()>;
    async fn chat_states(&self) -> Result<Vec<ConversationState>>;
    async fn clear_chat_history(&self, _chat_id: &str, _cleared_until_ms: i64) -> Result<()> {
        Err(Error::Unsupported("removing chat history"))
    }
    async fn set_chat_topic(&self, _chat_id: &str, _topic: &str) -> Result<()> {
        Err(Error::Unsupported("renaming a chat"))
    }
    async fn set_chat_muted(&self, chat_id: &str, muted: bool) -> Result<()>;
    async fn set_read_horizon(&self, conversation_id: &str, horizon: &str) -> Result<()>;
    async fn set_channel_notifications(
        &self,
        channel_id: &str,
        notifications: store::ChannelNotifications,
    ) -> Result<()>;
    async fn send_typing(&self, conversation: &ConversationRef, active: bool) -> Result<()>;
    async fn set_reaction(&self, target: &MessageTarget, reaction_type: &str) -> Result<()>;
    async fn unset_reaction(&self, target: &MessageTarget, reaction_type: &str) -> Result<()>;
    async fn edit_message(
        &self,
        target: &MessageTarget,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<()>;
    async fn soft_delete_message(&self, user_id: &str, target: &MessageTarget) -> Result<()>;
    async fn undo_soft_delete_message(&self, user_id: &str, target: &MessageTarget) -> Result<()>;
    async fn create_one_on_one(&self, my_user_id: &str, user_id: &str) -> Result<Chat>;
    async fn create_group(
        &self,
        my_user_id: &str,
        user_ids: &[String],
        topic: Option<&str>,
    ) -> Result<Chat>;
    async fn search_people(&self, query: &str) -> Result<Vec<User>>;

    async fn reply_with_quote(
        &self,
        _chat_id: &str,
        _quoted_message_id: &str,
        _html: &str,
        _mentions: &[OutgoingMention],
        _extras: &MessageExtras,
    ) -> Result<Message> {
        Err(Error::Unsupported("a quoted reply"))
    }

    async fn channel_files_folder(&self, _team_id: &str, _channel_id: &str) -> Result<DriveFolder> {
        Err(Error::Unsupported("a channel files folder"))
    }

    async fn channel_files_root(&self, _team_id: &str, _channel_id: &str) -> Result<DriveEntry> {
        Err(Error::Unsupported("a channel library"))
    }

    async fn channel_tab_web_url(
        &self,
        _team_id: &str,
        _channel_id: &str,
        _tab_id: &str,
    ) -> Result<Option<String>> {
        Err(Error::Unsupported("a channel tab link"))
    }

    async fn list_children(&self, _folder: &DriveFolder) -> Result<Vec<DriveEntry>> {
        Err(Error::Unsupported("a folder listing"))
    }

    async fn create_folder(&self, _parent: &DriveFolder, _name: &str) -> Result<DriveEntry> {
        Err(Error::Unsupported("creating a folder"))
    }

    async fn upload_file(
        &self,
        _destination: &UploadDestination,
        _file_name: &str,
        _bytes: &[u8],
        _progress: &(dyn Fn(u8) + Send + Sync),
    ) -> Result<UploadedFile> {
        Err(Error::Unsupported("a file upload"))
    }

    async fn share_file(&self, _file: &UploadedFile, _user_ids: &[String]) -> Result<()> {
        Err(Error::Unsupported("sharing a file"))
    }

    async fn delete_file(&self, _file: &UploadedFile) -> Result<()> {
        Err(Error::Unsupported("deleting a file"))
    }

    async fn resolve_share(&self, _open_url: &str) -> Result<SharedFile> {
        Err(Error::Unsupported("resolving a shared file"))
    }

    async fn resolve_drive_item(&self, _drive_id: &str, _item_id: &str) -> Result<SharedFile> {
        Err(Error::Unsupported("resolving a drive item"))
    }

    async fn download_range(
        &self,
        _download_url: &str,
        _start: u64,
        _end: Option<u64>,
    ) -> Result<Vec<u8>> {
        Err(Error::Unsupported("a file download"))
    }

    async fn hosted_content(&self, _url: &str) -> Result<Photo> {
        Err(Error::Unsupported("hosted content"))
    }

    async fn external_image(&self, _url: &str) -> Result<Photo> {
        Err(Error::Unsupported("external images"))
    }

    async fn user_photos(&self, _user_ids: &[String]) -> Result<Vec<Result<Option<Photo>>>> {
        Err(Error::Unsupported("profile photos"))
    }

    async fn presences(&self, _user_ids: &[String]) -> Result<Vec<Presence>> {
        Err(Error::Unsupported("presence"))
    }

    async fn person_profile(&self, _user_id: &str) -> Result<PersonProfile> {
        Err(Error::Unsupported("profile cards"))
    }

    async fn bot_profiles(&self, _bot_ids: &[String]) -> Result<Vec<BotProfile>> {
        Err(Error::Unsupported("bot profiles"))
    }

    async fn subscribe_presence(
        &self,
        _endpoint_id: &str,
        _trouter_uri: &str,
        _user_ids: &[String],
        _purge: bool,
    ) -> Result<()> {
        Err(Error::Unsupported("presence subscriptions"))
    }

    async fn own_status(&self, _user_id: &str) -> Result<PresenceStatus> {
        Err(Error::Unsupported("own status"))
    }

    async fn set_availability(&self, _forced: Option<&ForcedAvailability>) -> Result<()> {
        Err(Error::Unsupported("setting availability"))
    }

    async fn set_status_note(&self, _note: Option<&StatusNote>) -> Result<()> {
        Err(Error::Unsupported("setting the status message"))
    }

    async fn set_work_location(
        &self,
        _location: Option<(WorkLocationKind, DateTime<Utc>)>,
    ) -> Result<()> {
        Err(Error::Unsupported("setting the work location"))
    }

    async fn consumption_horizons(&self, _conversation_id: &str) -> Result<Vec<MemberHorizon>> {
        Err(Error::Unsupported("read receipts"))
    }

    async fn forward_messages(
        &self,
        _source_conversation_id: &str,
        _target_conversation_id: &str,
        _message_ids: &[String],
        _comment_html: &str,
    ) -> Result<ForwardResult> {
        Err(Error::Unsupported("forwarding messages"))
    }

    async fn set_saved(
        &self,
        _conversation_id: &str,
        _root_id: &str,
        _message_id: &str,
        _saved: bool,
    ) -> Result<()> {
        Err(Error::Unsupported("saving messages"))
    }

    async fn list_saved(&self) -> Result<Vec<SavedMessage>> {
        Err(Error::Unsupported("saved messages"))
    }

    async fn list_call_logs(&self) -> Result<Vec<CallLogEntry>> {
        Err(Error::Unsupported("call history"))
    }

    async fn chat_message_window(
        &self,
        _chat_id: &str,
        _anchor_id: &str,
        _direction: WindowDirection,
        _page_size: usize,
    ) -> Result<MessageWindow> {
        Err(Error::Unsupported("message windows"))
    }

    async fn chat_messages_by_id(
        &self,
        _chat_id: &str,
        _message_ids: &[String],
    ) -> Result<Vec<Result<Message>>> {
        Err(Error::Unsupported("batched message reads"))
    }

    async fn chat_pins(&self, _chat_id: &str) -> Result<Vec<PinnedMessage>> {
        Err(Error::Unsupported("pinned messages"))
    }

    async fn pin_message(&self, _chat_id: &str, _message_id: &str) -> Result<()> {
        Err(Error::Unsupported("pinning messages"))
    }

    async fn unpin_message(
        &self,
        _chat_id: &str,
        _message_id: &str,
        _parent_id: Option<&str>,
    ) -> Result<()> {
        Err(Error::Unsupported("pinning messages"))
    }

    async fn channel_pinned_posts(
        &self,
        _team_thread_id: &str,
        _channel_id: &str,
    ) -> Result<Vec<PinnedMessage>> {
        Err(Error::Unsupported("pinned channel posts"))
    }

    async fn set_post_pinned(
        &self,
        _team_thread_id: &str,
        _channel_id: &str,
        _post_id: &str,
        _pinned: bool,
    ) -> Result<()> {
        Err(Error::Unsupported("pinning channel posts"))
    }

    async fn mention_activities(&self) -> Result<Vec<MentionActivity>> {
        Err(Error::Unsupported("the activity feed"))
    }

    async fn app_consumption_horizon(&self) -> Result<AppConsumptionHorizon> {
        Ok(AppConsumptionHorizon::default())
    }

    async fn mark_all_channels_read(&self, _now_ms: i64) -> Result<AppConsumptionHorizon> {
        Err(Error::Unsupported("marking all channels as read"))
    }

    async fn chat_apps(&self, _chat_id: &str) -> Result<Vec<ChatApp>> {
        Err(Error::Unsupported("chat apps"))
    }

    async fn invoke_card(&self, _invoke: InvokeRequest) -> Result<InvokeResponse> {
        Err(Error::Unsupported("card actions"))
    }

    async fn message_links(
        &self,
        _conversation: &ConversationRef,
        _page_size: usize,
    ) -> Result<Vec<MessageLinks>> {
        Err(Error::Unsupported("link previews"))
    }

    async fn message_emotions(
        &self,
        _conversation: &ConversationRef,
        _newest_message_id: &str,
        _page_size: usize,
    ) -> Result<Vec<MessageEmotions>> {
        Err(Error::Unsupported("chat service reactions"))
    }

    async fn set_message_links(&self, _target: &MessageTarget, _links_json: &str) -> Result<()> {
        Err(Error::Unsupported("link previews"))
    }

    async fn link_info(&self, _url: &str) -> Result<LinkInfo> {
        Err(Error::Unsupported("link previews"))
    }

    async fn message_languages(
        &self,
        _conversation: &ConversationRef,
        _page_size: usize,
    ) -> Result<Vec<MessageLanguage>> {
        Err(Error::Unsupported("message languages"))
    }

    async fn translate_chat_messages(
        &self,
        _chat_id: &str,
        _to_language: &str,
        _messages: &[TranslateRequest],
        _trigger: &TranslationTrigger,
    ) -> Result<Vec<Translation>> {
        Err(Error::Unsupported("translation"))
    }

    async fn translate_channel_messages(
        &self,
        _team_id: &str,
        _channel_id: &str,
        _root_id: &str,
        _to_language: &str,
        _messages: &[TranslateRequest],
        _trigger: &TranslationTrigger,
    ) -> Result<Vec<Translation>> {
        Err(Error::Unsupported("translation"))
    }

    async fn translation_languages(&self, _locale: &str) -> Result<Vec<Language>> {
        Err(Error::Unsupported("translation"))
    }

    async fn language_settings(&self) -> Result<LanguageSettings> {
        Err(Error::Unsupported("translation settings"))
    }

    async fn patch_language_settings(&self, _patch: serde_json::Value) -> Result<()> {
        Err(Error::Unsupported("translation settings"))
    }

    async fn recording_policy(&self) -> Result<RecordingPolicy> {
        Err(Error::Unsupported("recording policy"))
    }

    async fn sound_settings(&self) -> Result<SoundSettings> {
        Err(Error::Unsupported("sound settings"))
    }

    async fn save_sound_settings(
        &self,
        _user_mri: &str,
        _previous: &SoundSettings,
        _updated: &SoundSettings,
    ) -> Result<()> {
        Err(Error::Unsupported("sound settings"))
    }

    async fn search_gifs(&self, _query: &str) -> Result<Vec<Gif>> {
        Err(Error::Unsupported("GIF search"))
    }

    async fn server_search_init(
        &self,
        _identity: &SearchIdentity,
        _search_session: &SearchSession,
    ) -> Result<()> {
        Err(Error::Unsupported("server search"))
    }

    async fn server_suggestions(
        &self,
        _identity: &SearchIdentity,
        _search_session: &SearchSession,
        _text: &str,
    ) -> Result<Suggestions> {
        Err(Error::Unsupported("server search"))
    }

    async fn server_query(
        &self,
        _identity: &SearchIdentity,
        _search_session: &SearchSession,
        _request: &QueryRequest,
    ) -> Result<QueryResults> {
        Err(Error::Unsupported("server search"))
    }

    async fn create_scheduled(
        &self,
        _inner_thread_id: &str,
        _html: &str,
        _send_at: DateTime<Utc>,
        _display_name: &str,
    ) -> Result<ScheduledDraft> {
        Err(Error::Unsupported("schedule send"))
    }

    async fn scheduled_drafts(&self) -> Result<Vec<ScheduledDraft>> {
        Err(Error::Unsupported("schedule send"))
    }

    async fn update_scheduled(
        &self,
        _draft: &ScheduledDraft,
        _html: &str,
        _display_name: &str,
    ) -> Result<ScheduledDraft> {
        Err(Error::Unsupported("schedule send"))
    }

    async fn cancel_scheduled(&self, _draft_id: &str) -> Result<()> {
        Err(Error::Unsupported("schedule send"))
    }
}

impl Remote for Graph {
    async fn me(&self) -> Result<User> {
        Ok(Graph::me(self).await?)
    }

    async fn chats_page(&self, top: usize) -> Result<ChatsPage> {
        Ok(Graph::chats_page(self, top).await?.into())
    }

    async fn chats_at(&self, next_link: &str) -> Result<ChatsPage> {
        Ok(Graph::chats_at(self, next_link).await?.into())
    }

    async fn chat(&self, chat_id: &str) -> Result<Chat> {
        Ok(Graph::chat(self, chat_id).await?)
    }

    async fn chat_members(&self, chat_id: &str) -> Result<Vec<Member>> {
        Ok(Graph::chat_members(self, chat_id).await?)
    }

    async fn joined_teams(&self) -> Result<Vec<Team>> {
        Ok(Graph::joined_teams(self).await?)
    }

    async fn channels_for_teams(&self, team_ids: &[String]) -> Result<Vec<Result<Vec<Channel>>>> {
        let listings = Graph::channels_for_teams(self, team_ids).await?;
        Ok(listings
            .into_iter()
            .map(|listing| listing.map_err(Into::into))
            .collect())
    }

    async fn chat_messages(
        &self,
        chat_id: &str,
        before: Option<DateTime<Utc>>,
        top: usize,
    ) -> Result<RemotePage> {
        Ok(Graph::chat_messages(self, chat_id, before, top)
            .await?
            .into())
    }

    async fn chat_message(&self, chat_id: &str, message_id: &str) -> Result<Message> {
        Ok(Graph::chat_message(self, chat_id, message_id).await?)
    }

    async fn channel_messages(
        &self,
        team_id: &str,
        channel_id: &str,
        top: usize,
    ) -> Result<RemotePage> {
        Ok(Graph::channel_messages(self, team_id, channel_id, top)
            .await?
            .into())
    }

    async fn channel_messages_at(&self, next_link: &str) -> Result<RemotePage> {
        Ok(Graph::channel_messages_at(self, next_link).await?.into())
    }

    async fn channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
    ) -> Result<Message> {
        Ok(Graph::channel_message(self, team_id, channel_id, message_id).await?)
    }

    async fn channel_reply(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
        reply_id: &str,
    ) -> Result<Message> {
        Ok(Graph::channel_reply(self, team_id, channel_id, message_id, reply_id).await?)
    }

    async fn channel_delta(
        &self,
        team_id: &str,
        channel_id: &str,
        modified_after: Option<DateTime<Utc>>,
    ) -> Result<DeltaPage> {
        Ok(
            Graph::channel_delta(self, team_id, channel_id, modified_after)
                .await?
                .into(),
        )
    }

    async fn channel_delta_at(&self, link: &str) -> Result<DeltaPage> {
        Ok(Graph::channel_delta_at(self, link).await?.into())
    }

    async fn channel_replies(
        &self,
        team_id: &str,
        channel_id: &str,
        root_ids: &[String],
    ) -> Result<Vec<Result<Vec<Message>>>> {
        let listings = Graph::channel_replies(self, team_id, channel_id, root_ids).await?;
        Ok(listings
            .into_iter()
            .map(|listing| listing.map_err(Into::into))
            .collect())
    }

    async fn send_chat_message(
        &self,
        chat_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<Message> {
        Ok(Graph::send_chat_message(self, chat_id, html, mentions, extras).await?)
    }

    async fn send_channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        html: &str,
        subject: Option<&str>,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<Message> {
        Ok(
            Graph::send_channel_message(self, team_id, channel_id, html, subject, mentions, extras)
                .await?,
        )
    }

    async fn reply_to_channel_message(
        &self,
        team_id: &str,
        channel_id: &str,
        message_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<Message> {
        Ok(Graph::reply_to_channel_message(
            self, team_id, channel_id, message_id, html, mentions, extras,
        )
        .await?)
    }

    async fn mark_chat_read(&self, chat_id: &str, user_id: &str, tenant_id: &str) -> Result<()> {
        Ok(Graph::mark_chat_read(self, chat_id, user_id, tenant_id).await?)
    }

    async fn mark_chat_unread(
        &self,
        chat_id: &str,
        user_id: &str,
        tenant_id: &str,
        last_read_at: DateTime<Utc>,
    ) -> Result<()> {
        Ok(Graph::mark_chat_unread(self, chat_id, user_id, tenant_id, last_read_at).await?)
    }

    async fn hide_chat(&self, chat_id: &str, user_id: &str, tenant_id: &str) -> Result<()> {
        Ok(Graph::hide_chat(self, chat_id, user_id, tenant_id).await?)
    }

    async fn unhide_chat(&self, chat_id: &str, user_id: &str, tenant_id: &str) -> Result<()> {
        Ok(Graph::unhide_chat(self, chat_id, user_id, tenant_id).await?)
    }

    async fn remove_chat_member(&self, chat_id: &str, user_id: &str) -> Result<()> {
        Ok(ThreadMembers::new(self.session())
            .remove(chat_id, user_id)
            .await?)
    }

    async fn add_chat_members(
        &self,
        chat_id: &str,
        user_ids: &[String],
        share: HistoryShare,
    ) -> Result<()> {
        Ok(ThreadMembers::new(self.session())
            .add(chat_id, user_ids, share, Utc::now().timestamp_millis())
            .await?)
    }

    async fn chat_states(&self) -> Result<Vec<ConversationState>> {
        Ok(Conversations::new(self.session())
            .conversation_states()
            .await?)
    }

    async fn clear_chat_history(&self, chat_id: &str, cleared_until_ms: i64) -> Result<()> {
        Ok(Conversations::new(self.session())
            .clear_history(chat_id, cleared_until_ms)
            .await?)
    }

    async fn set_chat_topic(&self, chat_id: &str, topic: &str) -> Result<()> {
        Ok(ThreadMembers::new(self.session())
            .set_topic(chat_id, topic)
            .await?)
    }

    async fn set_chat_muted(&self, chat_id: &str, muted: bool) -> Result<()> {
        Ok(Conversations::new(self.session())
            .set_alerts(chat_id, !muted)
            .await?)
    }

    async fn set_read_horizon(&self, conversation_id: &str, horizon: &str) -> Result<()> {
        Ok(Conversations::new(self.session())
            .set_read_horizon(conversation_id, horizon)
            .await?)
    }

    async fn set_channel_notifications(
        &self,
        channel_id: &str,
        notifications: store::ChannelNotifications,
    ) -> Result<()> {
        Ok(Conversations::new(self.session())
            .set_channel_notifications(
                channel_id,
                crate::folders::chatsvc_notifications(notifications),
            )
            .await?)
    }

    async fn send_typing(&self, conversation: &ConversationRef, active: bool) -> Result<()> {
        Ok(Messages::new(self.session())
            .send_typing(conversation, active)
            .await?)
    }

    async fn set_reaction(&self, target: &MessageTarget, reaction_type: &str) -> Result<()> {
        if let Some(key) = chatsvc::reaction_key(reaction_type) {
            return Ok(Messages::new(self.session())
                .set_emotion(&conversation_ref(target), target.message_id(), &key)
                .await?);
        }
        Ok(Graph::set_reaction(self, target, reaction_type).await?)
    }

    async fn unset_reaction(&self, target: &MessageTarget, reaction_type: &str) -> Result<()> {
        let keys = chatsvc::reaction_keys(reaction_type);
        if !keys.is_empty() {
            let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
            return Ok(Messages::new(self.session())
                .unset_emotions(&conversation_ref(target), target.message_id(), &keys)
                .await?);
        }
        Ok(Graph::unset_reaction(self, target, reaction_type).await?)
    }

    async fn edit_message(
        &self,
        target: &MessageTarget,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<()> {
        match chatsvc_conversation(target) {
            Some(conversation) if mentions.is_empty() => {
                let html = format!("{}{html}{}", extras.kept_html(), extras.files_html());
                Ok(Messages::new(self.session())
                    .edit_message(&conversation, target.message_id(), &html)
                    .await?)
            }
            Some(_) => Err(Error::Unsupported(
                "editing a channel message with mentions",
            )),
            None => Ok(Graph::edit_message(self, target, html, mentions, extras).await?),
        }
    }

    async fn soft_delete_message(&self, user_id: &str, target: &MessageTarget) -> Result<()> {
        match chatsvc_conversation(target) {
            Some(conversation) => Ok(Messages::new(self.session())
                .soft_delete_message(&conversation, target.message_id())
                .await?),
            None => Ok(Graph::soft_delete_message(self, user_id, target).await?),
        }
    }

    async fn undo_soft_delete_message(&self, user_id: &str, target: &MessageTarget) -> Result<()> {
        match chatsvc_conversation(target) {
            Some(conversation) => Ok(Messages::new(self.session())
                .undelete_message(&conversation, target.message_id())
                .await?),
            None => Ok(Graph::undo_soft_delete_message(self, user_id, target).await?),
        }
    }

    async fn create_one_on_one(&self, my_user_id: &str, user_id: &str) -> Result<Chat> {
        Ok(Graph::create_one_on_one(self, my_user_id, user_id).await?)
    }

    async fn create_group(
        &self,
        my_user_id: &str,
        user_ids: &[String],
        topic: Option<&str>,
    ) -> Result<Chat> {
        Ok(Graph::create_group(self, my_user_id, user_ids, topic).await?)
    }

    async fn search_people(&self, query: &str) -> Result<Vec<User>> {
        Ok(Graph::search_people(self, query).await?)
    }

    async fn reply_with_quote(
        &self,
        chat_id: &str,
        quoted_message_id: &str,
        html: &str,
        mentions: &[OutgoingMention],
        extras: &MessageExtras,
    ) -> Result<Message> {
        Ok(
            Graph::reply_with_quote(self, chat_id, quoted_message_id, html, mentions, extras)
                .await?,
        )
    }

    async fn channel_files_folder(&self, team_id: &str, channel_id: &str) -> Result<DriveFolder> {
        Ok(Graph::channel_files_folder(self, team_id, channel_id).await?)
    }

    async fn channel_files_root(&self, team_id: &str, channel_id: &str) -> Result<DriveEntry> {
        Ok(Graph::channel_files_root(self, team_id, channel_id).await?)
    }

    async fn channel_tab_web_url(
        &self,
        team_id: &str,
        channel_id: &str,
        tab_id: &str,
    ) -> Result<Option<String>> {
        Ok(Graph::channel_tab_web_url(self, team_id, channel_id, tab_id).await?)
    }

    async fn list_children(&self, folder: &DriveFolder) -> Result<Vec<DriveEntry>> {
        Ok(Graph::list_children(self, folder).await?)
    }

    async fn create_folder(&self, parent: &DriveFolder, name: &str) -> Result<DriveEntry> {
        Ok(Graph::create_folder(self, parent, name).await?)
    }

    async fn upload_file(
        &self,
        destination: &UploadDestination,
        file_name: &str,
        bytes: &[u8],
        progress: &(dyn Fn(u8) + Send + Sync),
    ) -> Result<UploadedFile> {
        Ok(Graph::upload_file(self, destination, file_name, bytes, progress).await?)
    }

    async fn share_file(&self, file: &UploadedFile, user_ids: &[String]) -> Result<()> {
        Ok(Graph::share_item(self, file, user_ids).await?)
    }

    async fn delete_file(&self, file: &UploadedFile) -> Result<()> {
        Ok(Graph::delete_drive_item(self, &file.drive_id, &file.item_id).await?)
    }

    async fn resolve_share(&self, open_url: &str) -> Result<SharedFile> {
        Ok(Graph::resolve_share(self, open_url).await?)
    }

    async fn resolve_drive_item(&self, drive_id: &str, item_id: &str) -> Result<SharedFile> {
        Ok(Graph::resolve_drive_item(self, drive_id, item_id).await?)
    }

    async fn download_range(
        &self,
        download_url: &str,
        start: u64,
        end: Option<u64>,
    ) -> Result<Vec<u8>> {
        Ok(Graph::download_range(self, download_url, start, end).await?)
    }

    async fn hosted_content(&self, url: &str) -> Result<Photo> {
        if chatsvc::is_link_image_url(url) {
            let image = Messages::new(self.session()).link_image(url).await?;
            return Ok(Photo {
                bytes: image.bytes,
                content_type: image.content_type,
            });
        }
        Ok(Graph::download_hosted_content(self, url).await?)
    }

    async fn external_image(&self, url: &str) -> Result<Photo> {
        Ok(Graph::download_external_image(self, url).await?)
    }

    async fn user_photos(&self, user_ids: &[String]) -> Result<Vec<Result<Option<Photo>>>> {
        let photos = Graph::user_photos(self, user_ids).await?;
        Ok(photos
            .into_iter()
            .map(|photo| photo.map_err(Into::into))
            .collect())
    }

    async fn presences(&self, user_ids: &[String]) -> Result<Vec<Presence>> {
        let found = PresenceLookup::new(self.session())
            .presences(user_ids)
            .await?;
        Ok(found
            .into_iter()
            .map(|presence| Presence {
                user_id: presence.user_id,
                availability: presence.availability,
                activity: presence.activity,
            })
            .collect())
    }

    async fn person_profile(&self, user_id: &str) -> Result<PersonProfile> {
        crate::profile::load_profile(self, user_id).await
    }

    async fn bot_profiles(&self, bot_ids: &[String]) -> Result<Vec<BotProfile>> {
        Ok(BotProfiles::new(self.session()).fetch(bot_ids).await?)
    }

    async fn subscribe_presence(
        &self,
        endpoint_id: &str,
        trouter_uri: &str,
        user_ids: &[String],
        purge: bool,
    ) -> Result<()> {
        Ok(PresenceLookup::new(self.session())
            .subscribe(endpoint_id, trouter_uri, user_ids, purge)
            .await?)
    }

    async fn own_status(&self, user_id: &str) -> Result<PresenceStatus> {
        Ok(PresenceService::new(self.session()).status(user_id).await?)
    }

    async fn set_availability(&self, forced: Option<&ForcedAvailability>) -> Result<()> {
        Ok(PresenceService::new(self.session())
            .set_availability(forced)
            .await?)
    }

    async fn set_status_note(&self, note: Option<&StatusNote>) -> Result<()> {
        Ok(PresenceService::new(self.session()).set_note(note).await?)
    }

    async fn set_work_location(
        &self,
        location: Option<(WorkLocationKind, DateTime<Utc>)>,
    ) -> Result<()> {
        Ok(PresenceService::new(self.session())
            .set_work_location(location)
            .await?)
    }

    async fn consumption_horizons(&self, conversation_id: &str) -> Result<Vec<MemberHorizon>> {
        Ok(Receipts::new(self.session())
            .consumption_horizons(conversation_id)
            .await?)
    }

    async fn forward_messages(
        &self,
        source_conversation_id: &str,
        target_conversation_id: &str,
        message_ids: &[String],
        comment_html: &str,
    ) -> Result<ForwardResult> {
        Ok(Messages::new(self.session())
            .forward_messages(
                source_conversation_id,
                target_conversation_id,
                message_ids,
                comment_html,
            )
            .await?)
    }

    async fn set_saved(
        &self,
        conversation_id: &str,
        root_id: &str,
        message_id: &str,
        saved: bool,
    ) -> Result<()> {
        Ok(Messages::new(self.session())
            .set_saved(conversation_id, root_id, message_id, saved)
            .await?)
    }

    async fn list_saved(&self) -> Result<Vec<SavedMessage>> {
        Ok(Messages::new(self.session()).list_saved().await?)
    }

    async fn list_call_logs(&self) -> Result<Vec<CallLogEntry>> {
        Ok(Messages::new(self.session()).list_call_logs().await?)
    }

    async fn chat_message_window(
        &self,
        chat_id: &str,
        anchor_id: &str,
        direction: WindowDirection,
        page_size: usize,
    ) -> Result<MessageWindow> {
        Ok(Messages::new(self.session())
            .message_window(chat_id, anchor_id, direction, page_size)
            .await?)
    }

    async fn chat_messages_by_id(
        &self,
        chat_id: &str,
        message_ids: &[String],
    ) -> Result<Vec<Result<Message>>> {
        let messages = Graph::chat_messages_by_id(self, chat_id, message_ids).await?;
        Ok(messages
            .into_iter()
            .map(|message| message.map_err(Into::into))
            .collect())
    }

    async fn chat_pins(&self, chat_id: &str) -> Result<Vec<PinnedMessage>> {
        Ok(Pins::new(self.session()).chat_pins(chat_id).await?)
    }

    async fn pin_message(&self, chat_id: &str, message_id: &str) -> Result<()> {
        Ok(Pins::new(self.session())
            .pin_message(chat_id, message_id)
            .await?)
    }

    async fn unpin_message(
        &self,
        chat_id: &str,
        message_id: &str,
        parent_id: Option<&str>,
    ) -> Result<()> {
        Ok(Pins::new(self.session())
            .unpin_message(chat_id, message_id, parent_id)
            .await?)
    }

    async fn channel_pinned_posts(
        &self,
        team_thread_id: &str,
        channel_id: &str,
    ) -> Result<Vec<PinnedMessage>> {
        Ok(Pins::with_skype_token(self.session())
            .channel_pinned_posts(team_thread_id, channel_id)
            .await?)
    }

    async fn set_post_pinned(
        &self,
        team_thread_id: &str,
        channel_id: &str,
        post_id: &str,
        pinned: bool,
    ) -> Result<()> {
        Ok(Pins::with_skype_token(self.session())
            .set_post_pinned(team_thread_id, channel_id, post_id, pinned)
            .await?)
    }

    async fn mention_activities(&self) -> Result<Vec<MentionActivity>> {
        Ok(ActivityFeed::new(self.session()).mentions().await?)
    }

    async fn app_consumption_horizon(&self) -> Result<AppConsumptionHorizon> {
        let properties = UserSettings::new(self.session()).properties().await?;
        Ok(parse_app_consumption_horizon(&properties))
    }

    async fn mark_all_channels_read(&self, now_ms: i64) -> Result<AppConsumptionHorizon> {
        Ok(UserSettings::new(self.session())
            .mark_all_channels_read(now_ms)
            .await?)
    }

    async fn chat_apps(&self, chat_id: &str) -> Result<Vec<ChatApp>> {
        Ok(CardActions::new(self.session()).chat_apps(chat_id).await?)
    }

    async fn invoke_card(&self, invoke: InvokeRequest) -> Result<InvokeResponse> {
        Ok(CardActions::new(self.session()).invoke(invoke).await?)
    }

    async fn message_links(
        &self,
        conversation: &ConversationRef,
        page_size: usize,
    ) -> Result<Vec<MessageLinks>> {
        Ok(Messages::new(self.session())
            .list_message_links(conversation, page_size)
            .await?)
    }

    async fn message_emotions(
        &self,
        conversation: &ConversationRef,
        newest_message_id: &str,
        page_size: usize,
    ) -> Result<Vec<MessageEmotions>> {
        Ok(Messages::new(self.session())
            .message_emotions(
                &conversation.conversation_id(),
                newest_message_id,
                page_size,
            )
            .await?)
    }

    async fn set_message_links(&self, target: &MessageTarget, links_json: &str) -> Result<()> {
        Ok(Messages::new(self.session())
            .set_links(&conversation_ref(target), target.message_id(), links_json)
            .await?)
    }

    async fn link_info(&self, url: &str) -> Result<LinkInfo> {
        Ok(Messages::new(self.session()).link_info(url).await?)
    }

    async fn message_languages(
        &self,
        conversation: &ConversationRef,
        page_size: usize,
    ) -> Result<Vec<MessageLanguage>> {
        Ok(Messages::new(self.session())
            .list_language_stamps(conversation, page_size)
            .await?)
    }

    async fn translate_chat_messages(
        &self,
        chat_id: &str,
        to_language: &str,
        messages: &[TranslateRequest],
        trigger: &TranslationTrigger,
    ) -> Result<Vec<Translation>> {
        Ok(Translator::new(self.session())
            .translate_chat(chat_id, to_language, messages, trigger)
            .await?)
    }

    async fn translate_channel_messages(
        &self,
        team_id: &str,
        channel_id: &str,
        root_id: &str,
        to_language: &str,
        messages: &[TranslateRequest],
        trigger: &TranslationTrigger,
    ) -> Result<Vec<Translation>> {
        Ok(Translator::new(self.session())
            .translate_channel(team_id, channel_id, root_id, to_language, messages, trigger)
            .await?)
    }

    async fn translation_languages(&self, locale: &str) -> Result<Vec<Language>> {
        Ok(Translator::new(self.session()).languages(locale).await?)
    }

    async fn language_settings(&self) -> Result<LanguageSettings> {
        Ok(LanguageSettingsClient::new(self.session()).read().await?)
    }

    async fn recording_policy(&self) -> Result<RecordingPolicy> {
        Ok(LanguageSettingsClient::new(self.session())
            .read_recording_policy()
            .await?)
    }

    async fn patch_language_settings(&self, patch: serde_json::Value) -> Result<()> {
        Ok(LanguageSettingsClient::new(self.session())
            .patch(patch)
            .await?)
    }

    async fn sound_settings(&self) -> Result<SoundSettings> {
        Ok(SoundSettingsClient::new(self.session()).read().await?)
    }

    async fn save_sound_settings(
        &self,
        user_mri: &str,
        previous: &SoundSettings,
        updated: &SoundSettings,
    ) -> Result<()> {
        Ok(SoundSettingsClient::new(self.session())
            .write(user_mri, previous, updated)
            .await?)
    }

    async fn search_gifs(&self, query: &str) -> Result<Vec<Gif>> {
        Ok(Gifs::new(self.session()).search(query).await?)
    }

    async fn server_search_init(
        &self,
        identity: &SearchIdentity,
        search_session: &SearchSession,
    ) -> Result<()> {
        Ok(SubstrateSearch::new(self.session(), identity.clone())
            .init(search_session)
            .await?)
    }

    async fn server_suggestions(
        &self,
        identity: &SearchIdentity,
        search_session: &SearchSession,
        text: &str,
    ) -> Result<Suggestions> {
        Ok(SubstrateSearch::new(self.session(), identity.clone())
            .suggestions(search_session, text)
            .await?)
    }

    async fn server_query(
        &self,
        identity: &SearchIdentity,
        search_session: &SearchSession,
        request: &QueryRequest,
    ) -> Result<QueryResults> {
        Ok(SubstrateSearch::new(self.session(), identity.clone())
            .query(search_session, request)
            .await?)
    }

    async fn create_scheduled(
        &self,
        inner_thread_id: &str,
        html: &str,
        send_at: DateTime<Utc>,
        display_name: &str,
    ) -> Result<ScheduledDraft> {
        Ok(ScheduledDrafts::new(self.session())
            .create(inner_thread_id, html, send_at, display_name)
            .await?)
    }

    async fn scheduled_drafts(&self) -> Result<Vec<ScheduledDraft>> {
        Ok(ScheduledDrafts::new(self.session()).list().await?)
    }

    async fn update_scheduled(
        &self,
        draft: &ScheduledDraft,
        html: &str,
        display_name: &str,
    ) -> Result<ScheduledDraft> {
        Ok(ScheduledDrafts::new(self.session())
            .update(draft, html, display_name)
            .await?)
    }

    async fn cancel_scheduled(&self, draft_id: &str) -> Result<()> {
        Ok(ScheduledDrafts::new(self.session())
            .cancel(draft_id)
            .await?)
    }
}

fn chatsvc_conversation(target: &MessageTarget) -> Option<ConversationRef> {
    match target {
        MessageTarget::Chat { .. } => CHATSVC_CHAT_WRITES.then(|| conversation_ref(target)),
        MessageTarget::Channel { .. } => Some(conversation_ref(target)),
    }
}

fn conversation_ref(target: &MessageTarget) -> ConversationRef {
    match target {
        MessageTarget::Chat { chat_id, .. } => ConversationRef::chat(chat_id),
        MessageTarget::Channel {
            channel_id,
            root_id: None,
            ..
        } => ConversationRef::channel_root(channel_id),
        MessageTarget::Channel {
            channel_id,
            root_id: Some(root_id),
            ..
        } => ConversationRef::channel_reply(channel_id, root_id),
    }
}

impl From<graph::Page<Message>> for RemotePage {
    fn from(page: graph::Page<Message>) -> Self {
        RemotePage {
            items: page.items,
            next_link: page.next_link,
        }
    }
}

impl From<graph::Page<Message>> for DeltaPage {
    fn from(page: graph::Page<Message>) -> Self {
        DeltaPage {
            items: page.items,
            next_link: page.next_link,
            delta_link: page.delta_link,
        }
    }
}

impl From<graph::Page<Chat>> for ChatsPage {
    fn from(page: graph::Page<Chat>) -> Self {
        ChatsPage {
            items: page.items,
            next_link: page.next_link,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_targets_route_to_chatsvc() {
        let root = MessageTarget::channel("t", "19:c@thread.tacv2", "m1", None);
        let reply = MessageTarget::channel("t", "19:c@thread.tacv2", "r1", Some("m1"));
        assert_eq!(
            chatsvc_conversation(&root),
            Some(ConversationRef::channel_root("19:c@thread.tacv2"))
        );
        assert_eq!(
            chatsvc_conversation(&reply),
            Some(ConversationRef::channel_reply("19:c@thread.tacv2", "m1"))
        );
    }

    #[test]
    fn reactions_reach_chats_regardless_of_the_feature_switch() {
        let chat = MessageTarget::chat("19:a@thread.v2", "m1");
        assert_eq!(
            conversation_ref(&chat),
            ConversationRef::chat("19:a@thread.v2")
        );
    }

    #[test]
    fn chats_follow_the_feature_switch() {
        let chat = MessageTarget::chat("19:a@thread.v2", "m1");
        assert_eq!(chatsvc_conversation(&chat).is_some(), CHATSVC_CHAT_WRITES);
    }
}
