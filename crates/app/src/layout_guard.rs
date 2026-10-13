use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

const WIDGETS_FILE: &str = "views/widgets.rs";
const SCAN_ALLOWLIST: &[(&str, &str)] = &[
    ("views/toolbar_placement.rs", ".top(px(-SLOT_HEIGHT))"),
    ("views/conversation.rs", ".bottom(px(-1.))"),
];

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
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let pattern = offset_pattern();
    let mut files = Vec::new();
    source_files(&root, &mut files);
    let mut matched_allowlist = HashSet::new();
    let mut violations = Vec::new();
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

mod frames {
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::time::Duration;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        AppContext as _, Bounds, Pixels, TestAppContext, VisualTestContext, WindowOptions, point,
        px, size,
    };
    use store::Store;

    use std::time::Instant;

    use calling::{CallState, CallUpdate, CaptionEntry, CaptionState, RosterEntry, call_channel};
    use gpui_kit::Entity;

    use crate::app_state::{AppHandle, AppState, Mode, Selection};
    use crate::call::{ActiveCall, CallModel};
    use crate::demo;
    use crate::user_activity::UserInput;
    use crate::views::call_transcript::TranscriptPanel;
    use crate::views::recap_tab::RECAP_REGION;
    use crate::views::shell::{AppShell, Startup, bind_keys, install_globals, sidebar_hidden_in_call};
    use crate::views::shortcuts::{OpenActivity, OpenCallsTab, OpenChannelsTab, OpenSaved};

    const GUARDED: &[&str] = &[
        "shell",
        "shell/title-bar",
        "shell/title-bar/search",
        "shell/title-bar/own-status",
        "shell/title-bar/own-status/avatar",
        "shell/islands",
        "shell/islands/rail",
        "shell/islands/sidebar",
        "shell/islands/main",
        "shell/status-bar",
    ];
    const CALL_GUARDED: &[&str] = &[
        "shell/islands/main/call-header",
        "shell/islands/main/call-stage",
        "shell/islands/main/call-controls",
        "shell/islands/main/call-controls/mic",
        "shell/islands/main/call-controls/leave",
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
    const SIZES: [(f32, f32); 3] = [(640., 480.), (1280., 800.), (1920., 1080.)];
    const TOLERANCE: f32 = 0.5;
    const KNOWN_VIOLATIONS: &[&str] = &[];

    type Tag = (&'static str, Bounds<Pixels>);

    enum Screen {
        Tab(&'static str, Option<fn(&mut VisualTestContext)>),
        StatusMenu,
        Call,
        Prejoin,
        CallTranscript,
        CallPeople,
        CallShare,
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
            .chain(PREJOIN_TAGS)
            .chain(SHARE_GUARDED)
            .chain([&CALL_TRANSCRIPT, &MENU, &RECAP_TAB, &RECAP_REGION, &PEOPLE_PANEL])
            .filter_map(|path| visual.debug_bounds(path).map(|bounds| (*path, bounds)))
            .collect()
    }

    fn sharing_call() -> ActiveCall {
        let mut call = gallery_call();
        call.model.apply(CallUpdate::ScreenShare(Some("8:orgid:guard-1".to_owned())));
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

    fn mount(cx: &mut TestAppContext) -> (gpui_kit::AnyWindowHandle, Entity<AppState>) {
        cx.update(gpui_kit::init);
        cx.update(|cx| {
            crate::theme::apply(cx);
            bind_keys(cx);
            let store = Store::open_in_memory().expect("in-memory store");
            demo::seed(&store);
            let mode = Mode {
                demo: true,
                read_only: false,
                demo_sync: None,
            };
            let state = cx.new(|_| AppState::new(Arc::new(store), mode));
            install_globals(&state, UserInput::new(), cx);
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
        let (handle, app) = mount(cx);
        let mut visual = VisualTestContext::from_window(handle, cx);
        let mut seen_known = HashSet::new();
        let mut unexpected = Vec::new();
        for (width, height) in SIZES {
            visual.simulate_resize(size(px(width), px(height)));
            let window = Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(width), px(height)),
            };
            let screens = [
                Screen::Tab("chats", None),
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
                Screen::Prejoin,
                Screen::StatusMenu,
                Screen::Call,
                Screen::CallTranscript,
                Screen::CallPeople,
                Screen::CallShare,
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
                                state.transcript_panel = Some(cx.new(|cx| TranscriptPanel::new(app_entity, window, cx)));
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
                let is_call = matches!(screen_name, "call" | "call-people" | "call-transcript" | "call-share");
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
                if status_menu {
                    expected.push(MENU);
                }
                match screen_name {
                    "calls" => expected.extend(CALLS_TAGS),
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
                            cx.notify();
                        })
                    });
                }
                if screen_name == "prejoin" {
                    let state = visual.update(|_, cx| cx.global::<AppHandle>().0.clone());
                    visual.update(|_, cx| state.update(cx, |state, cx| state.cancel_prejoin(cx)));
                }
                for (key, message) in found {
                    let full_key = format!("{screen_name}@{width}x{height}:{key}");
                    if KNOWN_VIOLATIONS.contains(&full_key.as_str()) {
                        seen_known.insert(full_key);
                    } else {
                        unexpected.push(format!("{full_key}: {message}"));
                    }
                }
            }
        }
        let stale: Vec<String> = KNOWN_VIOLATIONS
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
}
