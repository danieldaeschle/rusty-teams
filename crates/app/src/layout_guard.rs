use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

const WIDGETS_FILE: &str = "widgets.rs";
const SCAN_ALLOWLIST: &[(&str, &str)] = &[
    ("views/toolbar_placement.rs", ".top(px(-SLOT_HEIGHT))"),
    ("views/conversation.rs", ".bottom(px(-1.))"),
];

fn source_roots() -> Vec<PathBuf> {
    let crates_directory = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut roots: Vec<PathBuf> = fs::read_dir(&crates_directory)
        .expect("read crates directory")
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("app"))
        })
        .map(|path| path.join("src"))
        .filter(|path| path.is_dir())
        .collect();
    roots.sort();
    roots
}

fn source_files(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(directory).expect("read source directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            source_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
}

fn negative_offsets(
    pattern: &Regex,
    source: &str,
    relative: &str,
) -> Vec<(usize, Option<usize>, String)> {
    pattern
        .find_iter(source)
        .map(|found| {
            let line_number = source[..found.start()].matches('\n').count() + 1;
            let from_match = &source[found.start()..];
            let allowed = SCAN_ALLOWLIST.iter().position(|(allowed_file, snippet)| {
                *allowed_file == relative && from_match.starts_with(snippet)
            });
            let line_end = from_match.find('\n').unwrap_or(from_match.len());
            (
                line_number,
                allowed,
                from_match[..line_end].trim().to_owned(),
            )
        })
        .collect()
}

fn offset_pattern() -> Regex {
    Regex::new(r"\.(top|right|bottom|left|inset)\(\s*(px\(\s*)?-").expect("pattern")
}

#[test]
fn matcher_catches_wrapped_offsets() {
    let source = "let a = 1;\nlet b = div()\n    .top(\n        px(-4.))\n    .left(px(2.));\n";
    let found = negative_offsets(&offset_pattern(), source, "views/other.rs");
    assert_eq!(found, [(3, None, ".top(".to_owned())]);
}

#[test]
fn matcher_matches_allowlist_from_match_start() {
    let source = "x\n    .bottom(px(-1.))\n";
    let found = negative_offsets(&offset_pattern(), source, "views/conversation.rs");
    assert_eq!(found, [(2, Some(1), ".bottom(px(-1.))".to_owned())]);
}

#[test]
fn negative_offsets_go_through_corner_badge() {
    let pattern = offset_pattern();
    let mut matched_allowlist = HashSet::new();
    let mut violations = Vec::new();
    for root in source_roots() {
        let mut files = Vec::new();
        source_files(&root, &mut files);
        for file in files {
            let relative = file
                .strip_prefix(&root)
                .expect("inside src")
                .to_string_lossy()
                .replace('\\', "/");
            if relative == WIDGETS_FILE || relative == "layout_guard.rs" {
                continue;
            }
            let source = fs::read_to_string(&file).expect("read source file");
            for (line_number, position, text) in negative_offsets(&pattern, &source, &relative) {
                match position {
                    Some(position) => {
                        matched_allowlist.insert(position);
                    }
                    None => violations.push(format!(
                        "{relative}:{line_number}: negative absolute offset, use widgets::corner_badge: {text}"
                    )),
                }
            }
        }
    }
    let stale: Vec<String> = SCAN_ALLOWLIST
        .iter()
        .enumerate()
        .filter(|(position, _)| !matched_allowlist.contains(position))
        .map(|(_, (file, snippet))| format!("stale allowlist entry, remove it: {file} {snippet}"))
        .collect();
    assert!(
        violations.is_empty() && stale.is_empty(),
        "{}",
        violations
            .into_iter()
            .chain(stale)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn hover_styles_never_toggle_display() {
    let toggle =
        Regex::new(r"hover\([^|]*\|\w+\|\s*\w+\s*\.\s*(hidden|flex|block|grid)\(\)").unwrap();
    let mut files = Vec::new();
    for root in source_roots() {
        source_files(&root, &mut files);
    }
    let offenders: Vec<String> = files
        .iter()
        .flat_map(|path| {
            let source = fs::read_to_string(path).unwrap();
            source
                .lines()
                .enumerate()
                .filter(|(_, line)| toggle.is_match(line))
                .map(|(index, _)| format!("{}:{}", path.display(), index + 1))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "hover styles must toggle visible()/invisible(), not display (gpui panics \"must call prepaint before paint\"): {offenders:?}"
    );
}

mod frames {
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::time::Duration;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Bounds, Pixels, ScrollDelta, ScrollWheelEvent, TestAppContext,
        VisualTestContext, WindowOptions, point, px, size,
    };
    use store::Store;

    use std::time::Instant;

    use calling::{CallState, CallUpdate, CaptionEntry, CaptionState, RosterEntry, call_channel};
    use gpui_kit::Entity;

    use crate::app_state::{AppHandle, AppState, Mode, Selection};
    use crate::call::{ActiveCall, CallModel, ConsultCall};
    use crate::demo;
    use crate::settings::select_selector;
    use crate::settings::{item_selector, label, label_selector, metrics};
    use crate::settings_section::SettingsSection;
    use crate::user_activity::UserInput;
    use crate::views::call_transcript::TranscriptPanel;
    use crate::views::call_view::CHAT_PANEL_TAG;
    use crate::views::composer::delivery::{
        COMPOSER_DELIVERY_TAG, COMPOSER_IMPORTANCE_LABEL_TAG, COMPOSER_URGENT_BANNER_TAG,
        DELIVERY_MENU_TAG,
    };
    use crate::views::composer::{
        COMPOSER_BOX_TAG, COMPOSER_FORMAT_BAR_TAG, COMPOSER_FORMAT_DIVIDER_TAG,
        COMPOSER_FORMAT_TAG, COMPOSER_LEADING_TAG, COMPOSER_SEND_TAG,
    };
    use crate::views::conversation::{
        CHANNEL_DETAILS_BUTTON, CHANNEL_DETAILS_PINNED_TAG, CHANNEL_DETAILS_SHARED_TAG,
        CHANNEL_DETAILS_TAG,
    };
    use crate::views::conversation::{HEADER_DIVIDER, MEMBERS_BUTTON, TIMELINE_TAG};
    use crate::views::find_panel::{
        FIND_DIVIDER_TAG, FIND_INPUT_TAG, FIND_PANEL_TAG, FIND_RESULTS_TAG,
    };
    use crate::views::message_row::{BUBBLE_TAGS, IMPORTANCE_LABEL_TAG, bubble_padding_y};
    use crate::views::pin_banner::{BANNER_INSET, PIN_BANNER_TAG};
    use crate::views::proposal_card::{PROPOSAL_BUTTONS, PROPOSAL_CARD, PROPOSAL_THUMBNAILS};
    use crate::views::reaction_proposal_pill::REACTION_PILL;
    use crate::views::recap_tab::RECAP_REGION;
    use crate::views::shell::{
        AppShell, Startup, bind_keys, install_globals, sidebar_hidden_in_call,
    };
    use crate::views::shortcuts::{
        ClearChatFilters, FilterMutedChats, FindInChat, OpenActivity, OpenAgents, OpenCallsTab,
        OpenChannelsTab, OpenChatFilter, OpenChatsTab, OpenSaved,
    };
    use crate::views::sidebar::SIDEBAR_SCROLL_DIVIDER;
    use crate::views::sidebar::chat_row_selector;
    use crate::views::sidebar::chat_filter_bar::{
        FILTER_EMPTY_TAG, FILTER_INPUT_TAG, FILTER_PILLS_TAG, FILTER_TAG,
    };
    use crate::views::status_menu::{OWN_AVATAR_DISC, TITLE_AVATAR_SIZE};
    use crate::views::widgets::{
        COUNT_BADGE, RAIL_BADGE, badge_ink_offset, badge_pill, badge_selector, badge_text_selector,
        count_badge,
    };
    use gpui_kit::MouseButton;
    use gpui_kit::component::fluent::{
        DROPDOWN_OFFSET, MENU_ICON_LABEL_GAP, MENU_LIST_PADDING, OPTION_CHECK_GAP,
        OPTION_CHECK_INSET,
    };
    use gpui_kit::component::menu::selectors::{
        ITEMS_SELECTOR, icon_selector, item_selector as menu_item_selector,
        label_selector as menu_label_selector,
    };

    const GUARDED: &[&str] = &[
        "shell",
        "shell/title-bar",
        "shell/title-bar/search",
        "shell/title-bar/own-status",
        "shell/title-bar/own-status/avatar",
        OWN_AVATAR_DISC,
        "shell/islands",
        "shell/islands/rail",
        "shell/islands/sidebar",
        "shell/islands/main",
    ];
    const CALL_GUARDED: &[&str] = &[
        "shell/islands/main/call-header",
        "shell/islands/main/call-stage",
        "shell/islands/main/call-controls",
        "shell/islands/main/call-controls/mic",
        "shell/islands/main/call-controls/leave",
    ];
    const AGENTS_LIST: &str = "shell/islands/sidebar/agents";
    const CHAT_FILTER_TAGS: &[&str] = &[FILTER_TAG, FILTER_PILLS_TAG];
    const CHAT_FILTER_OPEN_TAGS: &[&str] = &[FILTER_INPUT_TAG, FILTER_EMPTY_TAG];
    const FIND_TAGS: &[&str] = &[
        FIND_PANEL_TAG,
        FIND_DIVIDER_TAG,
        FIND_INPUT_TAG,
        FIND_RESULTS_TAG,
    ];
    const CALLS_TAGS: &[&str] = &[
        "shell/islands/sidebar/calls-actions",
        "shell/islands/sidebar/calls-actions/meet-now",
        "shell/islands/sidebar/calls-actions/new-meeting",
    ];
    const PREJOIN_TAGS: &[&str] = &[
        "shell/islands/main/prejoin",
        "shell/islands/main/prejoin/preview",
        "shell/islands/main/prejoin/controls",
        "shell/islands/main/prejoin/devices",
        "shell/islands/main/prejoin/audio",
        "shell/islands/main/prejoin/actions",
    ];
    const PREJOIN_LINK: &str = "https://teams.microsoft.com/l/meetup-join/19%3ameeting_guard%40thread.v2/0?context=%7b%22Tid%22%3a%22t%22%2c%22Oid%22%3a%22o%22%7d";
    const CONSULT_GUARDED: &[&str] = &[
        "shell/islands/main/call-header/call-consult",
        "shell/islands/main/call-header/call-consult/transfer",
        "shell/islands/main/call-header/call-consult/resume",
    ];
    const CALL_TRANSCRIPT: &str = "shell/islands/main/call-transcript";
    const PEOPLE_PANEL: &str = "shell/islands/main/call-people";
    const SHARE_GUARDED: &[&str] = &[
        "shell/islands/main/call-stage/share-content",
        "shell/islands/main/call-stage/share-content/zoom-chip",
        "shell/islands/main/call-stage/share-divider",
        "shell/islands/main/call-stage/share-strip",
    ];
    const OVERLAY_PANELS: &[&str] = &[CALL_TRANSCRIPT, PEOPLE_PANEL];
    const OVERLAY_CHROME: &[&str] = &[
        "shell/islands/main/call-header",
        "shell/islands/main/call-stage",
    ];
    const MENU: &str = "popup-menu";
    const OWN_STATUS: &str = "shell/title-bar/own-status";
    const RECAP_TAB: &str = "shell/islands/main/recap-tab";
    const RECAP_MEETING_CHAT: &str = "demo-chat-meeting-standup";
    const PROPOSAL_CHAT: &str = "demo-chat-release";
    const REACTION_CHAT: &str = "demo-chat-mara";
    const IMPORTANCE_CHATS: [&str; 2] = ["demo-chat-priya", "demo-chat-atlas"];
    const IMPORTANCE_LABEL_INSET_X: f32 = 12.;
    const AGENT_TAGS: [&str; 4] = [
        PROPOSAL_CARD,
        PROPOSAL_THUMBNAILS,
        PROPOSAL_BUTTONS,
        REACTION_PILL,
    ];
    const SETTINGS_TAGS: &[&str] = &[
        "settings",
        "settings/nav",
        "settings/nav/divider",
        "settings/nav/about",
        "settings/main",
        "settings/main/header",
        "settings/main/header/close",
        "settings/main/body",
    ];
    const SETTINGS_BODY: &str = "settings/main/body";
    const SETTINGS_NAV: &str = "settings/nav";
    const SETTINGS_NAV_ABOUT: &str = "settings/nav/about";
    const NAV_LABEL_SIZE: f32 = 14.;
    const NAV_LINE_HEIGHT: f32 = 20.;
    const SETTINGS_CONTENT: &str = "settings/main/body/content";
    const MEMBERS_TAGS: &[&str] = &["chat-members-popup", "chat-members-popup/divider"];
    const MEMBERS_CHAT: &str = "demo-chat-release";
    const SIDEBAR: &str = "shell/islands/sidebar";
    const MAIN: &str = "shell/islands/main";
    const EDGE_DIVIDERS: &[&str] = &[SIDEBAR_SCROLL_DIVIDER, HEADER_DIVIDER];
    const DIVIDER_SEGMENT: &str = "divider";
    const DIVIDER_THICKNESS: f32 = 1.;
    const INSET_TOLERANCE: f32 = 1.;
    const INSET_RULES: &[InsetRule] = &[
        InsetRule {
            panel: "settings/nav",
            content: "settings/nav/about",
            edges: &[Edge::Left, Edge::Right, Edge::Bottom],
        },
        InsetRule {
            panel: "settings/main",
            content: "settings/main/header/close",
            edges: &[Edge::Top, Edge::Right],
        },
    ];
    const SCROLLED_INSET_RULE: InsetRule = InsetRule {
        panel: SETTINGS_BODY,
        content: SETTINGS_CONTENT,
        edges: &[Edge::Left, Edge::Bottom],
    };
    const SCROLLED_INSET_ALLOWLIST: &[(SettingsSection, &str)] = &[(
        SettingsSection::Agents,
        "Agents pages carry their own PAGE_PADDING side inset, the body narrows its side padding to keep the text edge",
    )];
    const SETTINGS_SECTIONS: [SettingsSection; 9] = [
        SettingsSection::General,
        SettingsSection::Notifications,
        SettingsSection::Appearance,
        SettingsSection::Chats,
        SettingsSection::Calls,
        SettingsSection::Devices,
        SettingsSection::Agents,
        SettingsSection::Mcp,
        SettingsSection::About,
    ];
    const SIZES: [(f32, f32); 3] = [(640., 480.), (1280., 800.), (1920., 1080.)];
    const TOLERANCE: f32 = 0.5;
    const KNOWN_VIOLATIONS: &[&str] = &[];
    const ZOOM_STEP: f32 = 1.25;

    type Tag = (&'static str, Bounds<Pixels>);

    #[derive(Clone, Copy, Debug)]
    enum Edge {
        Top,
        Right,
        Bottom,
        Left,
    }

    struct InsetRule {
        panel: &'static str,
        content: &'static str,
        edges: &'static [Edge],
    }

    fn inset(panel: Bounds<Pixels>, content: Bounds<Pixels>, edge: Edge) -> f32 {
        f32::from(match edge {
            Edge::Top => content.top() - panel.top(),
            Edge::Right => panel.right() - content.right(),
            Edge::Bottom => panel.bottom() - content.bottom(),
            Edge::Left => content.left() - panel.left(),
        })
    }

    fn inset_violation(
        rule: &InsetRule,
        panel: Bounds<Pixels>,
        content: Bounds<Pixels>,
    ) -> Option<(String, String)> {
        let insets: Vec<(Edge, f32)> = rule
            .edges
            .iter()
            .map(|edge| (*edge, inset(panel, content, *edge)))
            .collect();
        let values = insets.iter().map(|(_, value)| *value);
        let spread = values.clone().fold(f32::MIN, f32::max) - values.fold(f32::MAX, f32::min);
        (spread > INSET_TOLERANCE).then(|| {
            (
                format!("{}:uneven-inset", rule.panel),
                format!(
                    "{} insets {} by {insets:?}, they must match",
                    rule.panel, rule.content
                ),
            )
        })
    }

    fn inset_violations(tags: &[Tag], rules: &[InsetRule]) -> Vec<(String, String)> {
        let bounds_of = |path: &str| {
            tags.iter()
                .find(|(tag, _)| *tag == path)
                .map(|(_, bounds)| *bounds)
        };
        rules
            .iter()
            .filter_map(
                |rule| match (bounds_of(rule.panel), bounds_of(rule.content)) {
                    (Some(panel), Some(content)) => inset_violation(rule, panel, content),
                    _ => None,
                },
            )
            .collect()
    }

    fn divider_violations(tags: &[Tag]) -> Vec<(String, String)> {
        let mut found = Vec::new();
        for (path, bounds) in tags {
            let Some((parent, segment)) = path.rsplit_once('/') else {
                continue;
            };
            if !segment.starts_with(DIVIDER_SEGMENT) {
                continue;
            }
            let Some((_, panel)) = tags.iter().find(|(tag, _)| *tag == parent) else {
                found.push((
                    format!("{path}:no-panel"),
                    format!("{path} needs its panel {parent} tagged"),
                ));
                continue;
            };
            let thin = |length: Pixels| f32::from(length) <= DIVIDER_THICKNESS + TOLERANCE;
            let near = |first: Pixels, second: Pixels| f32::from(first - second).abs() <= TOLERANCE;
            let spans = if thin(bounds.size.height) {
                near(bounds.left(), panel.left()) && near(bounds.right(), panel.right())
            } else if thin(bounds.size.width) {
                near(bounds.top(), panel.top()) && near(bounds.bottom(), panel.bottom())
            } else {
                false
            };
            if !spans {
                found.push((
                    format!("{path}:short-divider"),
                    format!(
                        "{path} {} must be a {DIVIDER_THICKNESS} px line from edge to edge of {parent} {}",
                        describe(*bounds),
                        describe(*panel)
                    ),
                ));
            }
        }
        found
    }

    enum Screen {
        Tab(&'static str, Option<fn(&mut VisualTestContext)>),
        StatusMenu,
        Call,
        Prejoin,
        CallTranscript,
        CallPeople,
        CallShare,
        CallConsult,
    }

    fn parent_of(path: &str) -> Option<&str> {
        path.rsplit_once('/').map(|(parent, _)| parent)
    }

    fn contains(outer: Bounds<Pixels>, inner: Bounds<Pixels>) -> bool {
        let tolerance = px(TOLERANCE);
        inner.left() >= outer.left() - tolerance
            && inner.top() >= outer.top() - tolerance
            && inner.right() <= outer.right() + tolerance
            && inner.bottom() <= outer.bottom() + tolerance
    }

    fn overlaps(first: Bounds<Pixels>, second: Bounds<Pixels>) -> bool {
        let tolerance = px(TOLERANCE);
        first.left() < second.right() - tolerance
            && second.left() < first.right() - tolerance
            && first.top() < second.bottom() - tolerance
            && second.top() < first.bottom() - tolerance
    }

    fn describe(bounds: Bounds<Pixels>) -> String {
        format!(
            "[{:.1},{:.1} {:.1}x{:.1}]",
            f32::from(bounds.origin.x),
            f32::from(bounds.origin.y),
            f32::from(bounds.size.width),
            f32::from(bounds.size.height)
        )
    }

    fn violations(
        tags: &[Tag],
        window: Bounds<Pixels>,
        expected: &[&str],
    ) -> Vec<(String, String)> {
        let mut found = Vec::new();
        let bounds_of = |path: &str| {
            tags.iter()
                .find(|(tag, _)| *tag == path)
                .map(|(_, bounds)| *bounds)
        };
        for path in expected {
            if bounds_of(path).is_none() {
                found.push((
                    format!("{path}:missing"),
                    format!("{path} was not rendered"),
                ));
            }
        }
        for (path, bounds) in tags {
            if !contains(window, *bounds) {
                found.push((
                    format!("{path}:outside-window"),
                    format!(
                        "{path} {} leaves the window {}",
                        describe(*bounds),
                        describe(window)
                    ),
                ));
            }
            if let Some(parent) =
                parent_of(path).and_then(|parent| bounds_of(parent).map(|bounds| (parent, bounds)))
            {
                if !contains(parent.1, *bounds) {
                    found.push((
                        format!("{path}:outside-parent"),
                        format!(
                            "{path} {} leaves {} {}",
                            describe(*bounds),
                            parent.0,
                            describe(parent.1)
                        ),
                    ));
                }
            }
            for (other, other_bounds) in tags {
                let overlays = |first: &str, second: &str| {
                    OVERLAY_PANELS.contains(&first) && OVERLAY_CHROME.contains(&second)
                };
                let siblings = path < other
                    && parent_of(path).is_some()
                    && parent_of(path) == parent_of(other)
                    && !overlays(path, other)
                    && !overlays(other, path);
                if siblings && overlaps(*bounds, *other_bounds) {
                    found.push((
                        format!("{path}:overlaps:{other}"),
                        format!(
                            "{path} {} overlaps {other} {}",
                            describe(*bounds),
                            describe(*other_bounds)
                        ),
                    ));
                }
            }
        }
        found.extend(divider_violations(tags));
        found
    }

    const SETTLE_FRAMES: usize = 2;
    const SETTLE_LIMIT: usize = 60;
    const SETTLE_STEP: Duration = Duration::from_millis(50);

    fn collect(visual: &mut VisualTestContext, awaited: Option<&'static str>) -> Vec<Tag> {
        let mut frames = 0;
        while frames < SETTLE_FRAMES
            || awaited.is_some_and(|path| visual.debug_bounds(path).is_none())
        {
            assert!(frames < SETTLE_LIMIT, "{awaited:?} never rendered");
            visual.executor().advance_clock(SETTLE_STEP);
            std::thread::sleep(SETTLE_STEP);
            visual.run_until_parked();
            visual.update(|window, cx| window.render_frame(cx));
            frames += 1;
        }
        GUARDED
            .iter()
            .chain(CALL_GUARDED)
            .chain(CALLS_TAGS)
            .chain(CHAT_FILTER_TAGS)
            .chain(CHAT_FILTER_OPEN_TAGS)
            .chain(FIND_TAGS)
            .chain(PREJOIN_TAGS)
            .chain(SHARE_GUARDED)
            .chain(CONSULT_GUARDED)
            .chain([
                &AGENTS_LIST,
                &CALL_TRANSCRIPT,
                &MENU,
                &RECAP_TAB,
                &RECAP_REGION,
                &PEOPLE_PANEL,
            ])
            .chain(&AGENT_TAGS)
            .chain(SETTINGS_TAGS)
            .chain(MEMBERS_TAGS)
            .chain(EDGE_DIVIDERS)
            .filter_map(|path| visual.debug_bounds(path).map(|bounds| (*path, bounds)))
            .collect()
    }

    fn sharing_call() -> ActiveCall {
        let mut call = gallery_call();
        call.model
            .apply(CallUpdate::ScreenShare(Some("8:orgid:guard-1".to_owned())));
        call.share_stage.view.step_in((800., 450.));
        call
    }

    fn gallery_call() -> ActiveCall {
        let (handle, _control) = call_channel();
        let mut model = CallModel::meeting("Standup", false);
        model.apply(CallUpdate::State(CallState::Connected {
            since: Instant::now(),
        }));
        model.apply(CallUpdate::Roster(
            (0..11)
                .map(|index| RosterEntry {
                    mri: format!("8:orgid:guard-{index}"),
                    display_name: format!("Guest {index}"),
                    muted: false,
                    in_lobby: false,
                    has_video: false,
                    sharing: false,
                    hand: None,
                    spotlight: None,
                    organizer: false,
                    ..Default::default()
                })
                .collect(),
        ));
        ActiveCall::new(1, model, handle.commands, None, handle.video)
    }

    fn consult_calls() -> (ActiveCall, ActiveCall) {
        let connected = |id: u64, name: &str, mri: &str| {
            let (handle, _control) = call_channel();
            let mut model = CallModel::people(name, &[(mri.to_owned(), name.to_owned())]);
            model.apply(CallUpdate::State(CallState::Connected {
                since: Instant::now(),
            }));
            ActiveCall::new(id, model, handle.commands, None, handle.video)
        };
        let held = connected(1, "Christopher Montgomery-Lindqvist", "8:orgid:guard-held");
        let mut consult = connected(2, "Jonas Ortega", "8:orgid:guard-consult");
        consult.consult = Some(ConsultCall::new(
            (
                "8:orgid:guard-consult".to_owned(),
                "Jonas Ortega".to_owned(),
            ),
            held.model.peer_name.clone(),
        ));
        (consult, held)
    }

    fn mount(cx: &mut TestAppContext) -> (gpui_kit::AnyWindowHandle, Entity<AppState>) {
        mount_with(cx, false)
    }

    fn mount_with(
        cx: &mut TestAppContext,
        agent_api: bool,
    ) -> (gpui_kit::AnyWindowHandle, Entity<AppState>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            crate::theme::apply(cx);
            bind_keys(cx);
            let store = Store::open_in_memory().expect("in-memory store");
            demo::seed(&store);
            let store = Arc::new(store);
            if agent_api {
                crate::agent_api::init(store.clone(), true, cx);
            }
            let mode = Mode {
                demo: true,
                read_only: false,
                demo_sync: None,
            };
            let state = cx.new(|_| AppState::new(store, mode));
            install_globals(&state, UserInput::new(), cx);
            crate::agents::install(state.clone(), cx);
            demo::start_session(&state, cx);
            let shared_state = state.clone();
            let startup = Startup {
                switcher_query: None,
                composer_text: None,
                reply_to_last: false,
            };
            let window = gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
                cx.new(|cx| AppShell::new(state, None, startup, window, cx))
            })
            .expect("open test window")
            .0;
            (window, shared_state)
        })
    }

    fn rectangle(left: f32, top: f32, width: f32, height: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(left), px(top)),
            size: size(px(width), px(height)),
        }
    }

    #[test]
    fn rules_flag_overlap_and_overflow() {
        let window = rectangle(0., 0., 100., 100.);
        let tags: Vec<Tag> = vec![
            ("shell", rectangle(0., 0., 100., 100.)),
            ("shell/islands", rectangle(0., 0., 100., 50.)),
            ("shell/islands/rail", rectangle(0., 0., 60., 50.)),
            ("shell/islands/sidebar", rectangle(50., 0., 60., 50.)),
        ];
        let keys: Vec<String> = violations(&tags, window, &["shell/status-bar"])
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        let line_tags: Vec<Tag> = vec![
            ("panel", rectangle(0., 0., 100., 100.)),
            ("panel/divider", rectangle(0., 50., 100., 1.)),
            ("panel/divider-inset", rectangle(12., 60., 76., 1.)),
            ("panel/divider-side", rectangle(40., 0., 1., 100.)),
            ("panel/about", rectangle(12., 76., 76., 12.)),
            ("panel/footer", rectangle(12., 64., 76., 12.)),
        ];
        let line_keys: Vec<String> = divider_violations(&line_tags)
            .into_iter()
            .chain(inset_violations(
                &line_tags,
                &[
                    InsetRule {
                        panel: "panel",
                        content: "panel/about",
                        edges: &[Edge::Left, Edge::Right, Edge::Bottom],
                    },
                    InsetRule {
                        panel: "panel",
                        content: "panel/footer",
                        edges: &[Edge::Left, Edge::Bottom],
                    },
                ],
            ))
            .map(|(key, _)| key)
            .collect();
        assert_eq!(
            line_keys,
            ["panel/divider-inset:short-divider", "panel:uneven-inset"]
        );
        assert_eq!(
            keys,
            [
                "shell/status-bar:missing",
                "shell/islands/rail:overlaps:shell/islands/sidebar",
                "shell/islands/sidebar:outside-window",
                "shell/islands/sidebar:outside-parent",
            ]
        );
    }

    #[gpui_kit::test]
    fn no_element_overlaps_or_leaves_its_container(cx: &mut TestAppContext) {
        check_layout(cx, &SIZES, 1., KNOWN_VIOLATIONS);
    }

    #[gpui_kit::test]
    fn layout_holds_at_a_zoomed_step(cx: &mut TestAppContext) {
        let (width, height) = SIZES[1];
        let zoomed = [(width / ZOOM_STEP, height / ZOOM_STEP)];
        check_layout(cx, &zoomed, ZOOM_STEP, &[]);
    }

    fn check_layout(
        cx: &mut TestAppContext,
        sizes: &[(f32, f32)],
        zoom: f32,
        known_violations: &[&str],
    ) {
        let (handle, app) = mount(cx);
        let platform_scale = cx
            .update_window(handle, |_, window, _| window.scale_factor())
            .expect("test window");
        cx.simulate_window_scale_factor_change(handle, platform_scale * zoom);
        let mut visual = VisualTestContext::from_window(handle, cx);
        let mut seen_known = HashSet::new();
        let mut unexpected = Vec::new();
        for &(width, height) in sizes {
            visual.simulate_resize(size(px(width), px(height)));
            let window = Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(width), px(height)),
            };
            let screens = [
                Screen::Tab("chats", Some(|visual| visual.dispatch_action(OpenChatsTab))),
                Screen::Tab(
                    "chat-filter",
                    Some(|visual| {
                        visual.dispatch_action(OpenChatsTab);
                        visual.dispatch_action(OpenChatFilter);
                        visual.dispatch_action(FilterMutedChats);
                        visual.run_until_parked();
                        visual.simulate_input("zzz");
                    }),
                ),
                Screen::Tab(
                    "find",
                    Some(|visual| {
                        visual.dispatch_action(OpenChatsTab);
                        visual.dispatch_action(FindInChat);
                        visual.run_until_parked();
                        visual.simulate_input("access");
                    }),
                ),
                Screen::Tab(
                    "activity",
                    Some(|visual| visual.dispatch_action(OpenActivity)),
                ),
                Screen::Tab(
                    "channels",
                    Some(|visual| visual.dispatch_action(OpenChannelsTab)),
                ),
                Screen::Tab("saved", Some(|visual| visual.dispatch_action(OpenSaved))),
                Screen::Tab("calls", Some(|visual| visual.dispatch_action(OpenCallsTab))),
                Screen::Tab("agents", Some(|visual| visual.dispatch_action(OpenAgents))),
                Screen::Prejoin,
                Screen::StatusMenu,
                Screen::Call,
                Screen::CallTranscript,
                Screen::CallPeople,
                Screen::CallShare,
                Screen::CallConsult,
            ];
            for screen in screens {
                let (screen_name, status_menu) = match screen {
                    Screen::Tab(name, open) => {
                        if let Some(open) = open {
                            open(&mut visual);
                        }
                        (name, false)
                    }
                    Screen::Call | Screen::CallPeople => {
                        let people_open = matches!(screen, Screen::CallPeople);
                        visual.update(|_, cx| {
                            app.update(cx, |state, cx| {
                                let mut call = gallery_call();
                                call.people_open = people_open;
                                state.call = Some(call);
                                cx.notify();
                            })
                        });
                        (if people_open { "call-people" } else { "call" }, false)
                    }
                    Screen::Prejoin => {
                        let state = visual.update(|_, cx| cx.global::<AppHandle>().0.clone());
                        visual.update(|_, cx| {
                            state.update(cx, |state, cx| state.join_meeting_link(PREJOIN_LINK, cx))
                        });
                        ("prejoin", false)
                    }
                    Screen::CallTranscript => {
                        visual.update(|window, cx| {
                            app.update(cx, |state, cx| {
                                let mut call = gallery_call();
                                call.model.apply(CallUpdate::Captions(CaptionState::On));
                                for index in 0..40 {
                                    call.model.note_caption(
                                        CaptionEntry {
                                            id: format!("guard-{index}"),
                                            user_id: "8:orgid:guard-0".into(),
                                            display_name: "Guest 0".into(),
                                            text: "A long guarded sentence that has to wrap inside the panel".into(),
                                            is_final: true,
                                        },
                                        Instant::now(),
                                    );
                                }
                                call.transcript_open = true;
                                state.call = Some(call);
                                let app_entity = cx.entity();
                                state.transcript_panel = Some(cx.new(|cx| TranscriptPanel::new(app_entity, window, cx)).into());
                                cx.notify();
                            })
                        });
                        ("call-transcript", false)
                    }
                    Screen::CallShare => {
                        visual.update(|_, cx| {
                            app.update(cx, |state, cx| {
                                state.call = Some(sharing_call());
                                cx.notify();
                            })
                        });
                        ("call-share", false)
                    }
                    Screen::CallConsult => {
                        visual.update(|_, cx| {
                            app.update(cx, |state, cx| {
                                let (consult, held) = consult_calls();
                                state.call = Some(consult);
                                state.held_call = Some(held);
                                cx.notify();
                            })
                        });
                        ("call-consult", false)
                    }
                    Screen::StatusMenu => {
                        let tags = collect(&mut visual, None);
                        let button = tags
                            .iter()
                            .find(|(path, _)| *path == OWN_STATUS)
                            .map(|(_, bounds)| *bounds)
                            .expect("own status button rendered");
                        visual.simulate_click(button.center(), Default::default());
                        ("status-menu", true)
                    }
                };
                let is_call = matches!(
                    screen_name,
                    "call" | "call-people" | "call-transcript" | "call-share" | "call-consult"
                );
                let tags = collect(&mut visual, status_menu.then_some(MENU));
                let mut expected = GUARDED.to_vec();
                if is_call {
                    expected.extend(CALL_GUARDED);
                    let panel_open = matches!(screen_name, "call-people" | "call-transcript");
                    if sidebar_hidden_in_call(width, panel_open) {
                        expected.retain(|path| *path != "shell/islands/sidebar");
                    }
                }
                if screen_name == "call-transcript" {
                    expected.push(CALL_TRANSCRIPT);
                }
                if screen_name == "call-people" {
                    expected.push(PEOPLE_PANEL);
                }
                if screen_name == "call-share" {
                    expected.extend(SHARE_GUARDED);
                }
                if screen_name == "call-consult" {
                    expected.extend(CONSULT_GUARDED);
                }
                if status_menu {
                    expected.push(MENU);
                }
                match screen_name {
                    "chats" => expected.extend(CHAT_FILTER_TAGS),
                    "chat-filter" => {
                        expected.extend(CHAT_FILTER_TAGS);
                        expected.extend(CHAT_FILTER_OPEN_TAGS);
                    }
                    "find" => expected.extend(FIND_TAGS),
                    "calls" => expected.extend(CALLS_TAGS),
                    "agents" => expected.push(AGENTS_LIST),
                    "prejoin" => expected.extend(PREJOIN_TAGS),
                    _ => {}
                }
                let mut found = violations(&tags, window, &expected);
                if status_menu {
                    let menu = tags
                        .iter()
                        .find(|(path, _)| *path == MENU)
                        .map(|(_, bounds)| *bounds);
                    let button = tags
                        .iter()
                        .find(|(path, _)| *path == OWN_STATUS)
                        .map(|(_, bounds)| *bounds);
                    if let (Some(menu), Some(button)) = (menu, button) {
                        if overlaps(menu, button) {
                            found.push((
                                format!("{MENU}:covers:{OWN_STATUS}"),
                                format!(
                                    "{MENU} {} covers {OWN_STATUS} {}",
                                    describe(menu),
                                    describe(button)
                                ),
                            ));
                        }
                    }
                    visual.simulate_keystrokes("escape");
                    visual.run_until_parked();
                }
                if is_call {
                    visual.update(|_, cx| {
                        app.update(cx, |state, cx| {
                            state.call = None;
                            state.held_call = None;
                            cx.notify();
                        })
                    });
                }
                if screen_name == "find" {
                    visual.dispatch_action(gpui_kit::component::input::Escape);
                    visual.run_until_parked();
                }
                if screen_name == "chat-filter" {
                    visual.dispatch_action(ClearChatFilters);
                    visual.dispatch_action(OpenChatFilter);
                    visual.run_until_parked();
                }
                if screen_name == "prejoin" {
                    let state = visual.update(|_, cx| cx.global::<AppHandle>().0.clone());
                    visual.update(|_, cx| state.update(cx, |state, cx| state.cancel_prejoin(cx)));
                }
                for (key, message) in found {
                    let full_key = format!("{screen_name}@{width}x{height}:{key}");
                    if known_violations.contains(&full_key.as_str()) {
                        seen_known.insert(full_key);
                    } else {
                        unexpected.push(format!("{full_key}: {message}"));
                    }
                }
            }
        }
        let stale: Vec<String> = known_violations
            .iter()
            .filter(|key| !seen_known.contains(**key))
            .map(|key| format!("stale known violation, remove it: {key}"))
            .collect();
        assert!(
            unexpected.is_empty() && stale.is_empty(),
            "{}",
            unexpected
                .into_iter()
                .chain(stale)
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    #[gpui_kit::test]
    fn own_avatar_matches_the_teams_me_control(cx: &mut TestAppContext) {
        let (handle, _) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        visual.simulate_resize(size(px(1280.), px(800.)));
        collect(&mut visual, Some(OWN_AVATAR_DISC));
        let disc = visual
            .debug_bounds(OWN_AVATAR_DISC)
            .expect("own avatar rendered");
        let bar = visual
            .debug_bounds("shell/title-bar")
            .expect("title bar rendered");
        assert_eq!(
            f32::from(disc.size.width),
            TITLE_AVATAR_SIZE,
            "{}",
            describe(disc)
        );
        assert_eq!(
            f32::from(disc.size.height),
            TITLE_AVATAR_SIZE,
            "{}",
            describe(disc)
        );
        let top = f32::from(disc.top() - bar.top());
        let bottom = f32::from(bar.bottom() - disc.bottom());
        assert!(
            (top - bottom).abs() <= 1.,
            "own avatar {} is not centered in the title bar {}",
            describe(disc),
            describe(bar)
        );
    }

    const TITLE_SEARCH: &str = "shell/title-bar/search";
    const TITLE_NEIGHBOURS: &[&str] = &[
        "shell/title-bar/leading",
        "shell/title-bar/mode-pill",
        "shell/title-bar/trailing",
        "shell/title-bar/update",
        OWN_STATUS,
    ];
    const TITLE_SEARCH_SIZES: [(f32, f32, bool); 3] = [
        (1920., 1080., true),
        (1280., 800., true),
        (640., 480., false),
    ];

    #[gpui_kit::test]
    fn title_search_is_centered_on_the_window(cx: &mut TestAppContext) {
        let (handle, _) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        let shell = visual.update(|window, cx| {
            window
                .root::<gpui_kit::base::Root>()
                .flatten()
                .and_then(|root| root.read(cx).view().clone().downcast::<AppShell>().ok())
                .expect("app shell is the root view")
        });
        visual.update(|_, cx| {
            shell.update(cx, |shell, cx| {
                shell.set_update_status(crate::updater::UpdateStatus::Ready, cx)
            })
        });
        let mut found = Vec::new();
        for (width, height, centered) in TITLE_SEARCH_SIZES {
            visual.simulate_resize(size(px(width), px(height)));
            collect(&mut visual, Some(TITLE_SEARCH));
            let search = visual
                .debug_bounds(TITLE_SEARCH)
                .expect("title search rendered");
            let search_center = f32::from(search.center().x);
            eprintln!(
                "title search @{width}x{height}: {} center {search_center:.1}, window center {:.1}",
                describe(search),
                width / 2.
            );
            if centered && (search_center - width / 2.).abs() > 1. {
                found.push(format!(
                    "@{width}x{height}: search center {search_center:.1} is not the window center {:.1}",
                    width / 2.
                ));
            }
            if !contains(rectangle(0., 0., width, height), search) {
                found.push(format!(
                    "@{width}x{height}: search {} leaves the window",
                    describe(search)
                ));
            }
            for neighbour in TITLE_NEIGHBOURS {
                match visual.debug_bounds(neighbour) {
                    None => found.push(format!("@{width}x{height}: {neighbour} was not rendered")),
                    Some(bounds) if overlaps(search, bounds) => found.push(format!(
                        "@{width}x{height}: search {} overlaps {neighbour} {}",
                        describe(search),
                        describe(bounds)
                    )),
                    Some(_) => {}
                }
            }
        }
        assert!(found.is_empty(), "{}", found.join("\n"));
    }

    #[gpui_kit::test]
    fn call_chat_bubbles_stay_inside_the_panel(cx: &mut TestAppContext) {
        let (handle, app) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        for (width, height) in SIZES {
            visual.simulate_resize(size(px(width), px(height)));
            visual.update(|_, cx| {
                app.update(cx, |state, cx| {
                    let mut call = gallery_call();
                    call.conversation_id = Some(RECAP_MEETING_CHAT.to_owned());
                    call.chat_open = true;
                    state.call = Some(call);
                    cx.notify();
                })
            });
            collect(&mut visual, Some(CHAT_PANEL_TAG));
            let panel = visual
                .debug_bounds(CHAT_PANEL_TAG)
                .expect("chat panel rendered");
            let bubbles: Vec<(usize, Bounds<Pixels>)> = BUBBLE_TAGS
                .iter()
                .enumerate()
                .filter_map(|(index, tag)| visual.debug_bounds(tag).map(|bounds| (index, bounds)))
                .collect();
            assert!(!bubbles.is_empty(), "no bubbles rendered in the call chat");
            let found: Vec<String> = bubbles
                .into_iter()
                .filter(|(_, bounds)| {
                    !inside_horizontally(panel, *bounds) || !overlaps(panel, *bounds)
                })
                .map(|(index, bounds)| {
                    format!(
                        "call-chat@{width}x{height}: bubble {index} {} leaves the chat panel {}",
                        describe(bounds),
                        describe(panel)
                    )
                })
                .collect();
            visual.update(|_, cx| {
                app.update(cx, |state, cx| {
                    state.call = None;
                    cx.notify();
                })
            });
            assert!(found.is_empty(), "{}", found.join("\n"));
        }
    }

    fn inside_horizontally(outer: Bounds<Pixels>, inner: Bounds<Pixels>) -> bool {
        let tolerance = px(TOLERANCE);
        inner.left() >= outer.left() - tolerance && inner.right() <= outer.right() + tolerance
    }

    fn scroll_sidebar(visual: &mut VisualTestContext, delta: f32) {
        let sidebar = visual.debug_bounds(SIDEBAR).expect("sidebar rendered");
        visual.simulate_event(ScrollWheelEvent {
            position: sidebar.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(delta))),
            ..Default::default()
        });
    }

    #[gpui_kit::test]
    fn chat_list_line_shows_only_while_scrolled(cx: &mut TestAppContext) {
        let (handle, _) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        for (width, height) in SIZES {
            visual.simulate_resize(size(px(width), px(height)));
            let window = Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(width), px(height)),
            };
            collect(&mut visual, None);
            scroll_sidebar(&mut visual, 100_000.);
            let tags = collect(&mut visual, None);
            assert!(
                visual.debug_bounds(SIDEBAR_SCROLL_DIVIDER).is_none(),
                "chat list at its top must not show the line at {width}x{height}"
            );
            scroll_sidebar(&mut visual, -120.);
            let scrolled = collect(&mut visual, Some(SIDEBAR_SCROLL_DIVIDER));
            let sidebar = visual.debug_bounds(SIDEBAR).expect("sidebar rendered");
            let line = visual
                .debug_bounds(SIDEBAR_SCROLL_DIVIDER)
                .expect("scrolled chat list shows the line");
            assert!(
                line.top() > sidebar.top() + px(TOLERANCE),
                "the line sits under the Chats header, not on the island edge: {} in {}",
                describe(line),
                describe(sidebar)
            );
            let found: Vec<String> = violations(&scrolled, window, &[SIDEBAR_SCROLL_DIVIDER])
                .into_iter()
                .chain(violations(&tags, window, &[]))
                .map(|(key, message)| format!("chat-list@{width}x{height}:{key}: {message}"))
                .collect();
            assert!(found.is_empty(), "{}", found.join("\n"));
            scroll_sidebar(&mut visual, 100_000.);
            collect(&mut visual, None);
            assert!(
                visual.debug_bounds(SIDEBAR_SCROLL_DIVIDER).is_none(),
                "the line goes away once the chat list is back at its top"
            );
        }
    }

    #[gpui_kit::test]
    fn header_tabs_sit_above_the_header_line(cx: &mut TestAppContext) {
        let (handle, state) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        for (width, height) in SIZES {
            visual.simulate_resize(size(px(width), px(height)));
            let window = Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(width), px(height)),
            };
            visual.update(|_, cx| {
                state.update(cx, |state, cx| {
                    state.select(Selection::Chat(RECAP_MEETING_CHAT.to_owned()), cx)
                })
            });
            let tags = collect(&mut visual, Some(RECAP_TAB));
            let main = visual.debug_bounds(MAIN).expect("main island rendered");
            let tab = visual.debug_bounds(RECAP_TAB).expect("recap tab rendered");
            let line = visual
                .debug_bounds(HEADER_DIVIDER)
                .expect("the header line is always shown");
            let near = |first: Pixels, second: Pixels| f32::from(first - second).abs() <= TOLERANCE;
            assert!(
                tab.top() >= main.top() && near(tab.bottom(), line.top()),
                "tab {} must sit in the top bar above the line {}",
                describe(tab),
                describe(line)
            );
            let found: Vec<String> = violations(&tags, window, &[HEADER_DIVIDER, RECAP_TAB])
                .into_iter()
                .map(|(key, message)| format!("header@{width}x{height}:{key}: {message}"))
                .collect();
            assert!(found.is_empty(), "{}", found.join("\n"));
        }
    }

    #[gpui_kit::test]
    fn recap_tab_stays_inside_the_main_island(cx: &mut TestAppContext) {
        let (handle, state) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        for (width, height) in SIZES {
            visual.simulate_resize(size(px(width), px(height)));
            let window = Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(width), px(height)),
            };
            visual.update(|_, cx| {
                state.update(cx, |state, cx| {
                    state.select(Selection::Chat(RECAP_MEETING_CHAT.to_owned()), cx)
                })
            });
            collect(&mut visual, Some(RECAP_TAB));
            let tab = visual.debug_bounds(RECAP_TAB).expect("recap tab rendered");
            visual.simulate_click(tab.center(), Default::default());
            let tags = collect(&mut visual, Some(RECAP_REGION));
            let mut expected = GUARDED.to_vec();
            expected.extend([RECAP_TAB, RECAP_REGION]);
            let found: Vec<String> = violations(&tags, window, &expected)
                .into_iter()
                .map(|(key, message)| format!("recap@{width}x{height}:{key}: {message}"))
                .collect();
            assert!(found.is_empty(), "{}", found.join("\n"));
        }
    }

    #[gpui_kit::test]
    fn settings_dialog_keeps_its_inset_at_every_size(cx: &mut TestAppContext) {
        check_settings_dialog(cx, &SETTINGS_SECTIONS);
    }

    #[gpui_kit::test]
    fn settings_dialog_keeps_its_inset_in_the_teams_scheme(cx: &mut TestAppContext) {
        crate::theme::set_thread_scheme(Some("teams"));
        check_settings_dialog(cx, &[SettingsSection::Appearance, SettingsSection::General]);
        crate::theme::set_thread_scheme(None);
    }

    #[gpui_kit::test]
    fn settings_dialog_keeps_its_inset_in_the_light_theme(cx: &mut TestAppContext) {
        crate::theme::set_thread_shade(Some(crate::theme::Shade::Light));
        check_settings_dialog(cx, &[SettingsSection::Appearance, SettingsSection::General]);
        crate::theme::set_thread_shade(None);
    }

    fn check_settings_dialog(cx: &mut TestAppContext, sections: &[SettingsSection]) {
        let (handle, state) = mount_with(cx, true);
        let mut visual = VisualTestContext::from_window(handle, cx);
        for (width, height) in SIZES.into_iter().chain([(960., 640.)]) {
            visual.simulate_resize(size(px(width), px(height)));
            let window = Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(width), px(height)),
            };
            let inset = metrics(width, height).inset;
            let label_sizes = NAV_LABEL_SIZES.contains(&(width, height));
            for &section in sections {
                visual
                    .update(|_, cx| state.update(cx, |state, cx| state.open_settings(section, cx)));
                let tags = collect(&mut visual, Some(SETTINGS_TAGS[0]));
                let mut found: Vec<(String, String)> = violations(&tags, window, SETTINGS_TAGS)
                    .into_iter()
                    .filter(|(key, _)| key.starts_with("settings"))
                    .chain(inset_violations(&tags, INSET_RULES))
                    .collect();
                if label_sizes {
                    found.extend(nav_label_violations(&mut visual));
                }
                let scrolled_exempt = SCROLLED_INSET_ALLOWLIST
                    .iter()
                    .any(|(exempt, _)| *exempt == section);
                if !scrolled_exempt {
                    found.extend(scrolled_inset_violation(&mut visual));
                }
                let found: Vec<String> = found
                    .into_iter()
                    .map(|(key, message)| format!("{section:?}@{width}x{height}:{key}: {message}"))
                    .collect();
                assert!(found.is_empty(), "{}", found.join("\n"));
                let dialog = visual
                    .debug_bounds(SETTINGS_TAGS[0])
                    .expect("settings dialog");
                let edges = [
                    f32::from(dialog.left()),
                    f32::from(dialog.top()),
                    width - f32::from(dialog.right()),
                    height - f32::from(dialog.bottom()),
                ];
                assert!(
                    edges.iter().all(|edge| (edge - inset).abs() <= TOLERANCE),
                    "{section:?}@{width}x{height}: dialog edges {edges:?}, expected {inset}"
                );
            }
        }
    }

    const NAV_LABEL_SIZES: [(f32, f32); 3] = [(960., 640.), (1280., 800.), (1920., 1080.)];

    fn nav_label_violations(visual: &mut VisualTestContext) -> Vec<(String, String)> {
        let Some(nav) = visual.debug_bounds(SETTINGS_NAV) else {
            return vec![(SETTINGS_NAV.to_owned(), "not rendered".to_owned())];
        };
        let mut found = Vec::new();
        for section in SETTINGS_SECTIONS {
            let label_path: &'static str = Box::leak(label_selector(section).into_boxed_str());
            let item_path: &'static str = if section == SettingsSection::About {
                SETTINGS_NAV_ABOUT
            } else {
                Box::leak(item_selector(section).into_boxed_str())
            };
            let (Some(label_bounds), Some(item)) = (
                visual.debug_bounds(label_path),
                visual.debug_bounds(item_path),
            ) else {
                if section != SettingsSection::Agents {
                    found.push((label_path.to_owned(), "not rendered".to_owned()));
                }
                continue;
            };
            let text = label(section);
            let text_width = visual.update(|window, _| {
                let run = window.text_style().to_run(text.len());
                f32::from(
                    window
                        .text_system()
                        .shape_line(text.into(), px(NAV_LABEL_SIZE), &[run], None)
                        .width,
                )
            });
            let label_width = f32::from(label_bounds.size.width);
            let label_height = f32::from(label_bounds.size.height);
            if text_width > label_width + TOLERANCE
                && label_height < 2. * NAV_LINE_HEIGHT - TOLERANCE
            {
                found.push((
                    label_path.to_owned(),
                    format!("{text:?} needs {text_width} px, has {label_width} px on one line: truncated"),
                ));
            }
            let outside = |inner: Bounds<Pixels>, outer: Bounds<Pixels>| {
                inner.left() < outer.left() - px(TOLERANCE)
                    || inner.right() > outer.right() + px(TOLERANCE)
                    || inner.top() < outer.top() - px(TOLERANCE)
                    || inner.bottom() > outer.bottom() + px(TOLERANCE)
            };
            if outside(label_bounds, item) {
                found.push((
                    label_path.to_owned(),
                    format!("label {label_bounds:?} outside item {item:?}"),
                ));
            }
            if item.left() < nav.left() || item.right() > nav.right() {
                found.push((
                    item_path.to_owned(),
                    format!("item {item:?} outside nav {nav:?}"),
                ));
            }
        }
        found
    }

    fn scrolled_inset_violation(visual: &mut VisualTestContext) -> Option<(String, String)> {
        let body = visual.debug_bounds(SETTINGS_BODY)?;
        let content = visual.debug_bounds(SETTINGS_CONTENT)?;
        if content.bottom() <= body.bottom() {
            return None;
        }
        visual.simulate_event(ScrollWheelEvent {
            position: body.center(),
            delta: ScrollDelta::Pixels(point(px(0.), px(-100_000.))),
            ..Default::default()
        });
        collect(visual, None);
        let body = visual.debug_bounds(SETTINGS_BODY)?;
        let content = visual.debug_bounds(SETTINGS_CONTENT)?;
        inset_violation(&SCROLLED_INSET_RULE, body, content)
    }

    #[gpui_kit::test]
    fn members_popup_lines_reach_the_edges(cx: &mut TestAppContext) {
        let (handle, state) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        for (width, height) in SIZES {
            visual.simulate_resize(size(px(width), px(height)));
            let window = Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(width), px(height)),
            };
            visual.update(|_, cx| {
                state.update(cx, |state, cx| {
                    state.select(Selection::Chat(MEMBERS_CHAT.to_owned()), cx)
                })
            });
            collect(&mut visual, Some(MEMBERS_BUTTON));
            let button = visual
                .debug_bounds(MEMBERS_BUTTON)
                .expect("members button rendered");
            visual.simulate_click(button.center(), Default::default());
            let tags = collect(&mut visual, Some(MEMBERS_TAGS[0]));
            let found: Vec<String> = violations(&tags, window, MEMBERS_TAGS)
                .into_iter()
                .filter(|(key, _)| key.starts_with("chat-members"))
                .map(|(key, message)| format!("members@{width}x{height}:{key}: {message}"))
                .collect();
            visual.simulate_keystrokes("escape");
            visual.run_until_parked();
            assert!(found.is_empty(), "{}", found.join("\n"));
        }
    }

    const MENU_ITEMS_SCANNED: usize = 40;
    const MENU_CHAT: &str = "demo-chat-release";
    const THEME_SELECT: &str = "settings-theme";

    struct MenuRow {
        index: usize,
        item: Bounds<Pixels>,
        icon: Option<Bounds<Pixels>>,
        label: Option<Bounds<Pixels>>,
    }

    fn menu_rows(visual: &mut VisualTestContext) -> Vec<MenuRow> {
        (0..MENU_ITEMS_SCANNED)
            .filter_map(|index| {
                let item =
                    visual.debug_bounds(Box::leak(menu_item_selector(index).into_boxed_str()))?;
                Some(MenuRow {
                    index,
                    item,
                    icon: visual.debug_bounds(Box::leak(icon_selector(index).into_boxed_str())),
                    label: visual
                        .debug_bounds(Box::leak(menu_label_selector(index).into_boxed_str())),
                })
            })
            .collect()
    }

    fn menu_row_violations(rows: &[MenuRow], expected_gap: f32) -> Vec<(String, String)> {
        let mut found = Vec::new();
        for row in rows {
            let Some(icon) = row.icon else {
                continue;
            };
            let key = format!("{MENU}/item-{}", row.index);
            if let Some(label) = row.label {
                let gap = f32::from(label.left() - icon.right());
                if (gap - expected_gap).abs() > TOLERANCE {
                    found.push((
                        format!("{key}:icon-gap"),
                        format!(
                            "icon {} to label {} is {gap} px, expected {expected_gap}",
                            describe(icon),
                            describe(label)
                        ),
                    ));
                }
            }
            let offset = f32::from(icon.center().y - row.item.center().y);
            if offset.abs() > TOLERANCE {
                found.push((
                    format!("{key}:icon-off-centre"),
                    format!(
                        "icon {} sits {offset} px off the centre of item {}",
                        describe(icon),
                        describe(row.item)
                    ),
                ));
            }
        }
        found
    }

    fn listbox_violations(
        trigger: Bounds<Pixels>,
        menu: Bounds<Pixels>,
        items: Bounds<Pixels>,
        rows: &[MenuRow],
    ) -> Vec<(String, String)> {
        let mut found = Vec::new();
        let near = |value: f32, expected: f32| (value - expected).abs() <= TOLERANCE;
        if !near(f32::from(menu.left() - trigger.left()), 0.) {
            found.push((
                format!("{MENU}:not-aligned-to-trigger"),
                format!(
                    "list {} must start at the trigger's left edge {}",
                    describe(menu),
                    describe(trigger)
                ),
            ));
        }
        if !near(
            f32::from(menu.top() - trigger.bottom()),
            f32::from(DROPDOWN_OFFSET),
        ) {
            found.push((
                format!("{MENU}:trigger-offset"),
                format!(
                    "list {} must open {} px below the trigger {}",
                    describe(menu),
                    f32::from(DROPDOWN_OFFSET),
                    describe(trigger)
                ),
            ));
        }
        if menu.size.width < trigger.size.width - px(TOLERANCE) {
            found.push((
                format!("{MENU}:narrower-than-trigger"),
                format!(
                    "list {} is narrower than the trigger {}",
                    describe(menu),
                    describe(trigger)
                ),
            ));
        }
        if let (Some(first), Some(last)) = (rows.first(), rows.last()) {
            let top = f32::from(first.item.top() - items.top());
            let bottom = f32::from(items.bottom() - last.item.bottom());
            let padding = f32::from(MENU_LIST_PADDING);
            if !near(top, padding) || !near(bottom, padding) {
                found.push((
                    format!("{MENU}:uneven-padding"),
                    format!("list padding top {top} / bottom {bottom}, both must be {padding}"),
                ));
            }
            let expected_inset = f32::from(MENU_LIST_PADDING + OPTION_CHECK_INSET);
            match first.icon {
                Some(check) if near(f32::from(check.left() - menu.left()), expected_inset) => {}
                check => found.push((
                    format!("{MENU}:check-inset"),
                    format!(
                        "check column {check:?} must start {expected_inset} px inside the list {}",
                        describe(menu)
                    ),
                )),
            }
        }
        found
    }

    #[test]
    fn menu_rules_flag_gaps_offsets_and_misaligned_lists() {
        let row =
            |index: usize, item_top: f32, icon_left: f32, icon_top: f32, label_left: f32| MenuRow {
                index,
                item: rectangle(4., item_top, 120., 32.),
                icon: Some(rectangle(icon_left, icon_top, 20., 20.)),
                label: Some(rectangle(label_left, item_top + 6., 60., 20.)),
            };
        let rows = [row(0, 4., 10., 10., 36.), row(1, 38., 10., 46., 32.)];
        let keys: Vec<String> = menu_row_violations(&rows, f32::from(MENU_ICON_LABEL_GAP))
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        assert_eq!(
            keys,
            [
                "popup-menu/item-1:icon-gap",
                "popup-menu/item-1:icon-off-centre"
            ]
        );

        let trigger = rectangle(100., 50., 216., 32.);
        let option = |index: usize, top: f32| MenuRow {
            index,
            item: rectangle(104., top, 208., 32.),
            icon: Some(rectangle(110., top + 8., 16., 16.)),
            label: Some(rectangle(132., top + 6., 80., 20.)),
        };
        let aligned = [option(0, 88.), option(1, 122.)];
        let menu = rectangle(100., 84., 216., 74.);
        assert!(listbox_violations(trigger, menu, menu, &aligned).is_empty());
        let shifted = rectangle(96., 90., 200., 80.);
        let keys: Vec<String> = listbox_violations(trigger, shifted, shifted, &aligned)
            .into_iter()
            .map(|(key, _)| key)
            .collect();
        assert_eq!(
            keys,
            [
                "popup-menu:not-aligned-to-trigger",
                "popup-menu:trigger-offset",
                "popup-menu:narrower-than-trigger",
                "popup-menu:uneven-padding",
                "popup-menu:check-inset",
            ]
        );
    }

    #[gpui_kit::test]
    fn menu_items_keep_one_icon_gap_and_centred_icons(cx: &mut TestAppContext) {
        let (handle, _) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        visual.simulate_resize(size(px(1280.), px(800.)));
        let chat_row: &'static str = Box::leak(chat_row_selector(MENU_CHAT).into_boxed_str());
        collect(&mut visual, Some(chat_row));
        let mut found = Vec::new();
        let own_status = visual
            .debug_bounds(OWN_STATUS)
            .expect("own status button rendered");
        visual.simulate_click(own_status.center(), Default::default());
        collect(&mut visual, Some(MENU));
        let status_rows = menu_rows(&mut visual);
        assert!(
            status_rows.iter().any(|row| row.icon.is_some()),
            "status menu shows icons"
        );
        found.extend(menu_row_violations(
            &status_rows,
            f32::from(MENU_ICON_LABEL_GAP),
        ));
        visual.simulate_keystrokes("escape");
        collect(&mut visual, None);
        let row = visual.debug_bounds(chat_row).expect("chat row rendered");
        visual.simulate_mouse_down(row.center(), MouseButton::Right, Default::default());
        visual.simulate_mouse_up(row.center(), MouseButton::Right, Default::default());
        collect(&mut visual, Some(MENU));
        let chat_rows = menu_rows(&mut visual);
        assert!(
            chat_rows.iter().any(|row| row.icon.is_some()),
            "chat menu shows icons"
        );
        found.extend(menu_row_violations(
            &chat_rows,
            f32::from(MENU_ICON_LABEL_GAP),
        ));
        let found: Vec<String> = found
            .into_iter()
            .map(|(key, message)| format!("{key}: {message}"))
            .collect();
        assert!(found.is_empty(), "{}", found.join("\n"));
    }

    #[gpui_kit::test]
    fn settings_select_list_lines_up_with_its_trigger(cx: &mut TestAppContext) {
        let (handle, state) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        visual.simulate_resize(size(px(1280.), px(800.)));
        visual.update(|_, cx| {
            state.update(cx, |state, cx| {
                state.open_settings(SettingsSection::Appearance, cx)
            })
        });
        let select: &'static str = Box::leak(select_selector(THEME_SELECT).into_boxed_str());
        collect(&mut visual, Some(select));
        let trigger = visual.debug_bounds(select).expect("theme select rendered");
        visual.simulate_click(trigger.center(), Default::default());
        collect(&mut visual, Some(MENU));
        let menu = visual.debug_bounds(MENU).expect("list rendered");
        let items = visual
            .debug_bounds(ITEMS_SELECTOR)
            .expect("list items rendered");
        let rows = menu_rows(&mut visual);
        let found: Vec<String> = listbox_violations(trigger, menu, items, &rows)
            .into_iter()
            .chain(menu_row_violations(&rows, f32::from(OPTION_CHECK_GAP)))
            .map(|(key, message)| format!("{key}: {message}"))
            .collect();
        assert!(found.is_empty(), "{}", found.join("\n"));
    }

    const BADGE_SAMPLES: [(u32, bool); 5] =
        [(1, false), (42, false), (120, false), (7, true), (15, true)];

    struct BadgeSamples;

    impl gpui_kit::Render for BadgeSamples {
        fn render(
            &mut self,
            _: &mut gpui_kit::Window,
            _: &mut gpui_kit::Context<Self>,
        ) -> impl gpui_kit::IntoElement {
            use gpui_kit::{ParentElement as _, Styled as _};
            gpui_kit::div()
                .flex()
                .gap(px(12.))
                .p(px(12.))
                .children(BADGE_SAMPLES.map(|(count, rail)| {
                    if rail {
                        badge_pill(
                            count,
                            RAIL_BADGE,
                            crate::theme::accent(),
                            crate::theme::on_accent(),
                        )
                    } else {
                        count_badge(count, false)
                    }
                }))
        }
    }

    fn badge_violation(
        text: &str,
        pill: Bounds<Pixels>,
        label: Bounds<Pixels>,
        ink_offset: (f32, f32),
    ) -> Option<(String, String)> {
        let ink_x = f32::from(label.center().x) + ink_offset.0;
        let ink_y = f32::from(label.center().y) + ink_offset.1;
        let off_x = ink_x - f32::from(pill.center().x);
        let off_y = ink_y - f32::from(pill.center().y);
        (off_x.abs() > TOLERANCE || off_y.abs() > TOLERANCE).then(|| {
            (
                format!("count-badge/{text}:off-centre"),
                format!("text ink of {text:?} sits ({off_x:.2}, {off_y:.2}) px off the centre of its circle {}", describe(pill)),
            )
        })
    }

    #[test]
    fn badge_rule_flags_off_centre_text() {
        let pill = rectangle(0., 0., 18., 18.);
        assert!(badge_violation("1", pill, rectangle(6., 0., 6., 18.), (0., 0.)).is_none());
        assert!(badge_violation("1", pill, rectangle(6., 0., 6., 18.), (1., 0.)).is_some());
        assert!(badge_violation("1", pill, rectangle(6., 0., 6., 18.), (0., -1.)).is_some());
    }

    #[gpui_kit::test]
    fn count_badge_text_sits_in_the_centre_of_its_circle(cx: &mut TestAppContext) {
        cx.update(gpui_kit::init);
        cx.update(crate::theme::apply);
        let (_, visual) = cx.add_window_view(|_, _| BadgeSamples);
        collect(visual, None);
        let mut found = Vec::new();
        for (count, rail) in BADGE_SAMPLES {
            let text = crate::format::badge_text(count);
            let pill = visual
                .debug_bounds(Box::leak(badge_selector(&text).into_boxed_str()))
                .expect("badge rendered");
            let label = visual
                .debug_bounds(Box::leak(badge_text_selector(&text).into_boxed_str()))
                .expect("badge text rendered");
            let font_size = if rail {
                RAIL_BADGE.font_size
            } else {
                COUNT_BADGE.font_size
            };
            let offset = visual.update(|window, _| badge_ink_offset(window, &text, font_size));
            found.extend(badge_violation(&text, pill, label, offset));
        }
        let found: Vec<String> = found
            .into_iter()
            .map(|(key, message)| format!("{key}: {message}"))
            .collect();
        assert!(found.is_empty(), "{}", found.join("\n"));
    }

    const PINNED_CHAT: &str = "demo-chat-release";
    const PINNED_CHANNEL: &str = "demo-channel-1-1";

    #[gpui_kit::test]
    fn the_pin_banner_top_gap_matches_its_side_gaps(cx: &mut TestAppContext) {
        let (handle, state) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        let mut found = Vec::new();
        for (width, height) in SIZES {
            visual.simulate_resize(size(px(width), px(height)));
            visual.update(|_, cx| {
                state.update(cx, |state, cx| {
                    state.select(Selection::Chat(PINNED_CHAT.to_owned()), cx)
                })
            });
            collect(&mut visual, Some(PIN_BANNER_TAG));
            let banner = visual.debug_bounds(PIN_BANNER_TAG).expect("pin banner");
            let timeline = visual.debug_bounds(TIMELINE_TAG).expect("timeline");
            let divider = visual.debug_bounds(HEADER_DIVIDER).expect("header divider");
            let gaps = [
                ("top", f32::from(banner.top() - timeline.top())),
                ("left", f32::from(banner.left() - timeline.left())),
                ("right", f32::from(timeline.right() - banner.right())),
                ("below divider", f32::from(banner.top() - divider.bottom())),
            ];
            let side = gaps[1].1;
            if (side - BANNER_INSET).abs() > INSET_TOLERANCE
                || gaps
                    .iter()
                    .any(|(_, gap)| (gap - side).abs() > INSET_TOLERANCE)
            {
                found.push(format!(
                    "pin banner@{width}x{height}: gaps {gaps:?} must all be {BANNER_INSET}"
                ));
            }
        }
        assert!(found.is_empty(), "{}", found.join("\n"));
    }

    #[gpui_kit::test]
    fn channels_show_no_pin_banner_like_teams(cx: &mut TestAppContext) {
        let (handle, state) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        visual.simulate_resize(size(px(1280.), px(800.)));
        visual.update(|_, cx| {
            state.update(cx, |state, cx| {
                state.select(Selection::Channel(PINNED_CHANNEL.to_owned()), cx)
            })
        });
        collect(&mut visual, Some(TIMELINE_TAG));
        assert!(visual.debug_bounds(PIN_BANNER_TAG).is_none());
    }

    #[gpui_kit::test]
    fn the_channel_details_pane_lists_pinned_posts_inside_the_main_island(cx: &mut TestAppContext) {
        let (handle, state) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        visual.simulate_resize(size(px(1280.), px(800.)));
        visual.update(|_, cx| {
            state.update(cx, |state, cx| {
                state.select(Selection::Channel(PINNED_CHANNEL.to_owned()), cx)
            })
        });
        collect(&mut visual, Some(CHANNEL_DETAILS_BUTTON));
        let button = visual
            .debug_bounds(CHANNEL_DETAILS_BUTTON)
            .expect("channel details button");
        visual.simulate_click(button.center(), Default::default());
        collect(&mut visual, Some(CHANNEL_DETAILS_TAG));
        let pane = visual
            .debug_bounds(CHANNEL_DETAILS_TAG)
            .expect("details pane");
        let main = visual.debug_bounds(MAIN).expect("main island");
        let divider = visual.debug_bounds(HEADER_DIVIDER).expect("header divider");
        assert!(
            (f32::from(main.right() - pane.right())).abs() < INSET_TOLERANCE,
            "pane {pane:?} must end at the main island edge {main:?}"
        );
        assert!(
            (f32::from(pane.top() - divider.bottom())).abs() < INSET_TOLERANCE,
            "pane {pane:?} must start under the header divider {divider:?}"
        );
    }

    #[gpui_kit::test]
    fn the_channel_details_sections_follow_teams_order_with_equal_insets(cx: &mut TestAppContext) {
        let (handle, state) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        visual.simulate_resize(size(px(1280.), px(800.)));
        visual.update(|_, cx| {
            state.update(cx, |state, cx| {
                state.select(Selection::Channel(PINNED_CHANNEL.to_owned()), cx)
            })
        });
        collect(&mut visual, Some(CHANNEL_DETAILS_BUTTON));
        let button = visual
            .debug_bounds(CHANNEL_DETAILS_BUTTON)
            .expect("channel details button");
        visual.simulate_click(button.center(), Default::default());
        collect(&mut visual, Some(CHANNEL_DETAILS_SHARED_TAG));
        let pane = visual
            .debug_bounds(CHANNEL_DETAILS_TAG)
            .expect("details pane");
        let pinned = visual
            .debug_bounds(CHANNEL_DETAILS_PINNED_TAG)
            .expect("pinned section");
        let shared = visual
            .debug_bounds(CHANNEL_DETAILS_SHARED_TAG)
            .expect("shared section");
        assert!(
            pinned.bottom() <= shared.top(),
            "pinned {pinned:?} must come before shared {shared:?}"
        );
        for (name, section) in [("pinned", pinned), ("shared", shared)] {
            let left = f32::from(section.left() - pane.left());
            let right = f32::from(pane.right() - section.right());
            assert!(
                (left - right).abs() < INSET_TOLERANCE && left > 0.,
                "{name} section insets left {left} right {right} must match"
            );
        }
    }

    fn composer_button_insets(visual: &mut VisualTestContext) -> [f32; 6] {
        collect(visual, Some(COMPOSER_SEND_TAG));
        let mut bounds_of = |path: &'static str| {
            visual
                .debug_bounds(path)
                .unwrap_or_else(|| panic!("{path} rendered"))
        };
        let composer = bounds_of(COMPOSER_BOX_TAG);
        let leading = bounds_of(COMPOSER_LEADING_TAG);
        let send = bounds_of(COMPOSER_SEND_TAG);
        [
            f32::from(leading.left() - composer.left()),
            f32::from(leading.top() - composer.top()),
            f32::from(composer.bottom() - leading.bottom()),
            f32::from(composer.right() - send.right()),
            f32::from(send.top() - composer.top()),
            f32::from(composer.bottom() - send.bottom()),
        ]
    }

    #[gpui_kit::test]
    fn composer_buttons_keep_equal_insets(cx: &mut TestAppContext) {
        let (handle, _) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        visual.simulate_resize(size(px(1280.), px(800.)));
        let single_line = composer_button_insets(&mut visual);
        let left = single_line[0];
        assert!(
            single_line
                .iter()
                .all(|inset| (inset - left).abs() < TOLERANCE),
            "single line: insets left, top, bottom of the first button and right, top, bottom of send {single_line:?}"
        );
        let leading = visual
            .debug_bounds(COMPOSER_LEADING_TAG)
            .expect("leading buttons");
        let send = visual.debug_bounds(COMPOSER_SEND_TAG).expect("send button");
        visual.simulate_click(
            point((leading.right() + send.left()) / 2., send.center().y),
            Default::default(),
        );
        visual.simulate_keystrokes("o n e shift-enter t w o shift-enter t h r e e");
        let three_lines = composer_button_insets(&mut visual);
        assert!(
            three_lines[1] > single_line[1] + 30.,
            "three lines did not grow the composer: {three_lines:?}"
        );
        for inset in [
            three_lines[0],
            three_lines[2],
            three_lines[3],
            three_lines[5],
        ] {
            assert!(
                (inset - left).abs() < TOLERANCE,
                "three lines: insets left, top, bottom of the first button and right, top, bottom of send {three_lines:?}, expected {left}"
            );
        }
    }

    #[gpui_kit::test]
    fn the_format_bar_divider_runs_edge_to_edge_inside_the_composer(cx: &mut TestAppContext) {
        const FORMAT_TAGS: [&str; 6] = [
            COMPOSER_BOX_TAG,
            COMPOSER_FORMAT_BAR_TAG,
            COMPOSER_FORMAT_DIVIDER_TAG,
            COMPOSER_LEADING_TAG,
            COMPOSER_FORMAT_TAG,
            COMPOSER_SEND_TAG,
        ];
        let (handle, _) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        for (width, height) in SIZES {
            visual.simulate_resize(size(px(width), px(height)));
            let window = Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(width), px(height)),
            };
            collect(&mut visual, Some(COMPOSER_FORMAT_TAG));
            let toggle = visual
                .debug_bounds(COMPOSER_FORMAT_TAG)
                .expect("format button");
            visual.simulate_click(toggle.center(), Default::default());
            collect(&mut visual, Some(COMPOSER_FORMAT_DIVIDER_TAG));
            let tags: Vec<Tag> = FORMAT_TAGS
                .iter()
                .filter_map(|path| visual.debug_bounds(path).map(|bounds| (*path, bounds)))
                .collect();
            let found: Vec<String> = violations(&tags, window, &FORMAT_TAGS)
                .into_iter()
                .map(|(key, message)| format!("format@{width}x{height}:{key}: {message}"))
                .collect();
            let bar = visual.debug_bounds(COMPOSER_FORMAT_BAR_TAG).expect("bar");
            let leading = visual.debug_bounds(COMPOSER_LEADING_TAG).expect("leading");
            let toggle = visual
                .debug_bounds(COMPOSER_FORMAT_TAG)
                .expect("format button");
            visual.simulate_click(toggle.center(), Default::default());
            collect(&mut visual, None);
            assert!(found.is_empty(), "{}", found.join("\n"));
            assert!(
                bar.bottom() <= leading.top(),
                "bar {bar:?} over the buttons {leading:?}"
            );
            assert!(visual.debug_bounds(COMPOSER_FORMAT_BAR_TAG).is_none());
        }
    }

    #[gpui_kit::test]
    fn delivery_options_label_and_banner_line_up_with_the_composer(cx: &mut TestAppContext) {
        const DELIVERY_TAGS: [&str; 5] = [
            COMPOSER_BOX_TAG,
            COMPOSER_LEADING_TAG,
            COMPOSER_DELIVERY_TAG,
            COMPOSER_IMPORTANCE_LABEL_TAG,
            COMPOSER_URGENT_BANNER_TAG,
        ];
        const URGENT_OPTION: &str = "urgent-delivery-option";
        const BUTTON_ICON_INSET: f32 = 6.;
        let (handle, _) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        for (width, height) in SIZES {
            visual.simulate_resize(size(px(width), px(height)));
            let window = Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(width), px(height)),
            };
            collect(&mut visual, Some(COMPOSER_DELIVERY_TAG));
            let button = visual
                .debug_bounds(COMPOSER_DELIVERY_TAG)
                .expect("delivery button");
            visual.simulate_click(button.center(), Default::default());
            collect(&mut visual, Some(URGENT_OPTION));
            let menu = visual.debug_bounds(DELIVERY_MENU_TAG).expect("menu");
            assert!(
                contains(window, menu),
                "menu {} leaves the window",
                describe(menu)
            );
            let urgent = visual.debug_bounds(URGENT_OPTION).expect("urgent option");
            visual.simulate_click(urgent.center(), Default::default());
            collect(&mut visual, Some(COMPOSER_URGENT_BANNER_TAG));
            assert!(visual.debug_bounds(DELIVERY_MENU_TAG).is_none());
            let tags: Vec<Tag> = DELIVERY_TAGS
                .iter()
                .filter_map(|path| visual.debug_bounds(path).map(|bounds| (*path, bounds)))
                .collect();
            let found: Vec<String> = violations(&tags, window, &DELIVERY_TAGS)
                .into_iter()
                .map(|(key, message)| format!("delivery@{width}x{height}:{key}: {message}"))
                .collect();
            assert!(found.is_empty(), "{}", found.join("\n"));
            let composer = visual.debug_bounds(COMPOSER_BOX_TAG).expect("composer");
            let banner = visual
                .debug_bounds(COMPOSER_URGENT_BANNER_TAG)
                .expect("banner");
            let label = visual
                .debug_bounds(COMPOSER_IMPORTANCE_LABEL_TAG)
                .expect("label");
            let leading = visual.debug_bounds(COMPOSER_LEADING_TAG).expect("leading");
            assert!(
                (f32::from(banner.left() - composer.left())).abs() <= TOLERANCE
                    && (f32::from(banner.right() - composer.right())).abs() <= TOLERANCE,
                "banner {} not on the composer edges {}",
                describe(banner),
                describe(composer)
            );
            assert!(
                banner.bottom() <= composer.top(),
                "banner {} over the composer {}",
                describe(banner),
                describe(composer)
            );
            assert!(
                (inset(composer, label, Edge::Left)
                    - inset(composer, leading, Edge::Left)
                    - BUTTON_ICON_INSET)
                    .abs()
                    <= TOLERANCE,
                "label {} not on the icon edge of {}",
                describe(label),
                describe(leading)
            );
            let close = visual
                .debug_bounds(COMPOSER_URGENT_BANNER_TAG)
                .map(|banner| point(banner.right() - px(18.), banner.center().y))
                .expect("banner");
            visual.simulate_click(close, Default::default());
            collect(&mut visual, None);
            assert!(visual.debug_bounds(COMPOSER_URGENT_BANNER_TAG).is_none());
            assert!(visual.debug_bounds(COMPOSER_IMPORTANCE_LABEL_TAG).is_none());
        }
    }

    #[gpui_kit::test]
    fn importance_label_sits_inside_its_bubble_on_the_text_edge(cx: &mut TestAppContext) {
        let (handle, state) = mount_with(cx, true);
        let mut visual = VisualTestContext::from_window(handle, cx);
        for (width, height) in SIZES {
            visual.simulate_resize(size(px(width), px(height)));
            for chat in IMPORTANCE_CHATS {
                visual.update(|_, cx| {
                    state.update(cx, |state, cx| {
                        state.select(Selection::Chat(chat.to_owned()), cx)
                    })
                });
                collect(&mut visual, Some(TIMELINE_TAG));
                let timeline = visual.debug_bounds(TIMELINE_TAG).expect("timeline");
                visual.simulate_event(ScrollWheelEvent {
                    position: timeline.center(),
                    delta: ScrollDelta::Pixels(point(px(0.), px(-100_000.))),
                    ..Default::default()
                });
                collect(&mut visual, Some(IMPORTANCE_LABEL_TAG));
                let label = visual.debug_bounds(IMPORTANCE_LABEL_TAG).expect("label");
                let bubble = BUBBLE_TAGS
                    .iter()
                    .filter_map(|tag| visual.debug_bounds(tag))
                    .find(|bubble| contains(*bubble, label))
                    .unwrap_or_else(|| {
                        panic!("{chat}@{width}x{height}: label {label:?} outside every bubble")
                    });
                let left = inset(bubble, label, Edge::Left);
                let top = inset(bubble, label, Edge::Top);
                assert!(
                    (left - IMPORTANCE_LABEL_INSET_X).abs() <= TOLERANCE
                        && (top - bubble_padding_y(false)).abs() <= TOLERANCE,
                    "{chat}@{width}x{height}: label inset left {left} top {top} in {}",
                    describe(bubble)
                );
            }
        }
    }

    #[gpui_kit::test]
    fn agent_proposals_stay_inside_the_main_island(cx: &mut TestAppContext) {
        let (handle, state) = mount_with(cx, true);
        let mut visual = VisualTestContext::from_window(handle, cx);
        for (width, height) in SIZES {
            visual.simulate_resize(size(px(width), px(height)));
            let window = Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(width), px(height)),
            };
            for (chat, expected_tags) in [
                (PROPOSAL_CHAT, &AGENT_TAGS[..3]),
                (REACTION_CHAT, &AGENT_TAGS[3..]),
            ] {
                visual.update(|_, cx| {
                    state.update(cx, |state, cx| {
                        state.select(Selection::Chat(chat.to_owned()), cx)
                    })
                });
                let tags = collect(&mut visual, Some(expected_tags[0]));
                let mut expected = GUARDED.to_vec();
                expected.extend(expected_tags);
                let found: Vec<String> = violations(&tags, window, &expected)
                    .into_iter()
                    .map(|(key, message)| format!("{chat}@{width}x{height}:{key}: {message}"))
                    .collect();
                assert!(found.is_empty(), "{}", found.join("\n"));
            }
        }
    }
}

#[test]
fn lines_are_solid() {
    let mut files = Vec::new();
    for root in source_roots() {
        source_files(&root, &mut files);
    }
    let violations: Vec<String> = files
        .iter()
        .filter(|file| !file.ends_with("layout_guard.rs"))
        .flat_map(|file| {
            let source = fs::read_to_string(file).expect("read source file");
            source
                .lines()
                .enumerate()
                .filter(|(_, line)| {
                    line.contains(".border_dashed(") || line.contains(".border_dotted(")
                })
                .map(|(index, line)| {
                    format!(
                        "{}:{}: lines are always solid: {}",
                        file.display(),
                        index + 1,
                        line.trim()
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(violations.is_empty(), "{}", violations.join("\n"));
}

const STATE_RING_COLORS: &[&str] = &[
    "if focused",
    "if selected",
    "if speaking",
    "if current",
    "if open",
    "if state.failed",
];
const OUTLINE_ALLOWLIST: &[(&str, &str, &str)] = &[
    (
        "notify/ring_view.rs",
        "theme::green()",
        "state signal (pulsing ring around the caller while it rings), not a resting outline",
    ),
    (
        "notify/meeting_view.rs",
        "theme::border_strong()",
        "outlined button, not a surface",
    ),
    (
        "settings/mcp.rs",
        "theme::border_strong()",
        "outlined button, not a surface",
    ),
    (
        "settings/rows.rs",
        "theme::border_strong()",
        "outlined button, not a surface",
    ),
    (
        "views/card_widgets.rs",
        "border)",
        "message content drawn as Teams renders it",
    ),
    (
        "views/card_widgets.rs",
        "theme::border_strong()",
        "message content drawn as Teams renders it",
    ),
    ("views/shared_tab.rs", "theme::accent()", "input field"),
    ("views/card_input.rs", "if error.is_some()", "input field"),
    ("views/format_toolbar.rs", "if refused", "input field"),
    (
        "views/card_code_block.rs",
        "theme::code_header_border()",
        "message content drawn as Teams renders it",
    ),
    ("views/call_transcript.rs", "theme::border()", "input field"),
    (
        "views/conversation.rs",
        "theme::accent()",
        "state signal (speaking, sharing, drag, drop, failed), not a resting outline",
    ),
    (
        "views/attachment_tray.rs",
        "theme::accent()",
        "state signal (speaking, sharing, drag, drop, failed), not a resting outline",
    ),
    (
        "views/card_carousel.rs",
        "theme::border_strong()",
        "message content drawn as Teams renders it",
    ),
    (
        "views/card_table.rs",
        "grid_color",
        "message content drawn as Teams renders it",
    ),
    (
        "views/activity_panel.rs",
        "theme::surface()",
        "cutout ring in the background color, invisible against its surface",
    ),
    (
        "views/avatar.rs",
        "theme::surface()",
        "cutout ring in the background color, invisible against its surface",
    ),
    (
        "views/avatar.rs",
        "theme::text_muted()",
        "offline presence glyph, Teams icon",
    ),
    (
        "views/message_row.rs",
        "theme::border_strong()",
        "deleted message placeholder, Teams look",
    ),
    (
        "views/message_row.rs",
        "theme::red()",
        "state signal (speaking, sharing, drag, drop, failed), not a resting outline",
    ),
    (
        "views/message_row.rs",
        "if row.delivery",
        "state signal (speaking, sharing, drag, drop, failed), not a resting outline",
    ),
    (
        "views/new_meeting_dialog.rs",
        "theme::border()",
        "input field",
    ),
    (
        "views/call_mini_window.rs",
        "theme::border_strong()",
        "self view over live video, needs an edge on any picture",
    ),
    ("views/transfer_picker.rs", "theme::border()", "input field"),
    (
        "views/call_notice.rs",
        "theme::border_strong()",
        "outlined button, not a surface",
    ),
    (
        "views/composer/schedule.rs",
        "theme::accent()",
        "input field",
    ),
    (
        "views/call_mini.rs",
        "border)",
        "state signal (speaking, sharing, drag, drop, failed), not a resting outline",
    ),
    (
        "views/sidebar.rs",
        "theme::accent()",
        "state signal (speaking, sharing, drag, drop, failed), not a resting outline",
    ),
    (
        "views/adaptive_card.rs",
        "theme::border()",
        "message content drawn as Teams renders it",
    ),
    (
        "views/adaptive_card.rs",
        "border)",
        "message content drawn as Teams renders it",
    ),
    (
        "views/share_bar.rs",
        "theme::red()",
        "state signal (speaking, sharing, drag, drop, failed), not a resting outline",
    ),
    ("settings_controls.rs", "if invalid", "input field"),
];

fn outlines(source: &str) -> Vec<(usize, String)> {
    let pattern = Regex::new(r"\.border(_1\(\)|_2\(\)|\(px)").expect("pattern");
    pattern
        .find_iter(source)
        .map(|found| {
            let line_number = source[..found.start()].matches('\n').count() + 1;
            let after = &source[found.end()..];
            let color = after
                .find(".border_color(")
                .filter(|position| *position < 120)
                .map(|position| {
                    let start = &after[position + ".border_color(".len()..];
                    start
                        .lines()
                        .map(str::trim)
                        .find(|line| !line.is_empty())
                        .unwrap_or_default()
                        .to_owned()
                })
                .unwrap_or_default();
            (line_number, color)
        })
        .collect()
}

#[test]
fn outline_matcher_reads_the_border_color() {
    let source = "div()\n    .border_1()\n    .border_color(theme::amber())\n    .border_t_1()\n";
    assert_eq!(outlines(source), [(2, "theme::amber())".to_owned())]);
}

#[test]
fn surfaces_are_soft() {
    let mut matched_allowlist = HashSet::new();
    let mut violations = Vec::new();
    for root in source_roots() {
        let mut files = Vec::new();
        source_files(&root, &mut files);
        for file in files {
            let relative = file
                .strip_prefix(&root)
                .expect("inside src")
                .to_string_lossy()
                .replace('\\', "/");
            if relative == "layout_guard.rs" {
                continue;
            }
            let source = fs::read_to_string(&file).expect("read source file");
            for (line_number, color) in outlines(&source) {
                if STATE_RING_COLORS
                    .iter()
                    .any(|prefix| color.starts_with(prefix))
                {
                    continue;
                }
                match OUTLINE_ALLOWLIST.iter().position(|(allowed_file, prefix, _)| {
                    *allowed_file == relative && color.starts_with(prefix)
                }) {
                    Some(position) => {
                        matched_allowlist.insert(position);
                    }
                    None => violations.push(format!(
                        "{relative}:{line_number}: hard outline on a surface, use a soft fill or shadow (border_color {color})"
                    )),
                }
            }
        }
    }
    let stale = OUTLINE_ALLOWLIST
        .iter()
        .enumerate()
        .filter(|(position, _)| !matched_allowlist.contains(position))
        .map(|(_, (file, prefix, _))| {
            format!("stale outline allowlist entry, remove it: {file} {prefix}")
        });
    let report: Vec<String> = violations.into_iter().chain(stale).collect();
    assert!(report.is_empty(), "{}", report.join("\n"));
}

fn indentation(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn chain_lines<'a>(lines: &[&'a str], index: usize) -> Vec<&'a str> {
    let indent = indentation(lines[index]);
    let belongs = |line: &&str| line.trim().is_empty() || indentation(line) >= indent;
    let start = lines[..index]
        .iter()
        .rposition(|line| !belongs(line))
        .map_or(0, |position| position + 1);
    let end = lines[index..]
        .iter()
        .position(|line| !belongs(line))
        .map_or(lines.len(), |position| index + position);
    lines[start..end]
        .iter()
        .filter(|line| indentation(line) == indent)
        .copied()
        .collect()
}

fn shrinking_rows(row_height: &Regex, source: &str) -> Vec<usize> {
    let lines: Vec<&str> = source.lines().collect();
    (0..lines.len())
        .filter(|&index| row_height.is_match(lines[index]))
        .filter(|&index| {
            !chain_lines(&lines, index).iter().any(|line| {
                let line = line.trim();
                line.starts_with(".flex_none()") || line.starts_with(".flex_shrink_0()")
            })
        })
        .map(|index| index + 1)
        .collect()
}

fn row_height_pattern() -> Regex {
    Regex::new(r"^\s*\.h\(px\(((\w+_)?(ROW|ENTRY)_HEIGHT|row_height)\)\)").expect("pattern")
}

#[test]
fn matcher_finds_rows_without_flex_none() {
    let source = "fn rows() {\n    h_flex()\n        .h(px(ROW_HEIGHT))\n        .child(\n            div()\n                .flex_none(),\n        );\n    div()\n        .id(1)\n        .h(px(OWN_ROW_HEIGHT))\n        .flex_none();\n}\n";
    assert_eq!(shrinking_rows(&row_height_pattern(), source), [3]);
}

#[test]
fn fixed_height_rows_never_shrink() {
    let row_height = row_height_pattern();
    let mut files = Vec::new();
    for root in source_roots() {
        source_files(&root, &mut files);
    }
    let violations: Vec<String> = files
        .iter()
        .filter(|file| !file.ends_with("layout_guard.rs"))
        .flat_map(|file| {
            let source = fs::read_to_string(file).expect("read source file");
            shrinking_rows(&row_height, &source)
                .into_iter()
                .map(|line| format!("{}:{line}", file.display()))
                .collect::<Vec<_>>()
        })
        .collect();
    assert!(
        violations.is_empty(),
        "a fixed-height row needs .flex_none(), or a max_h scroll list shrinks it to its content: {violations:?}"
    );
}
