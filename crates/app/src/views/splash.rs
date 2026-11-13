use std::time::{Duration, Instant};

use gpui_kit::component::v_flex;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::title_bar::{TITLE_BAR_HEIGHT, app_icon_image};
use crate::backend::ConnectionState;
use crate::browser_sign_in::BrowserSignInState;
use crate::i18n::tr;
use crate::theme;

const MIN_VISIBLE: Duration = Duration::from_millis(400);
const STARTING_FOR: Duration = Duration::from_millis(300);
const HINT_AFTER: Duration = Duration::from_secs(8);
const GIVE_UP_AFTER: Duration = Duration::from_secs(30);
const FADE_OUT: Duration = Duration::from_millis(220);
pub const CONTENT_RISE: Duration = Duration::from_millis(260);
pub const CONTENT_RISE_OFFSET: f32 = 8.;
const LOGO_SIZE: f32 = 76.;
const LOGO_BREATH: f32 = 0.04;
const BREATH_PERIOD: Duration = Duration::from_millis(2400);
const BAR_WIDTH: f32 = 168.;
const BAR_HEIGHT: f32 = 3.;
const BAR_TOP_GAP: f32 = 32.;
const SWEEP_SHARE: f32 = 0.4;
const SWEEP_PERIOD: Duration = Duration::from_millis(1300);
const STATUS_TOP_GAP: f32 = 14.;
const STATUS_SIZE: f32 = 13.;
const STATUS_SWAP: Duration = Duration::from_millis(180);
const STATUS_SWAP_OFFSET: f32 = 4.;
const HINT_SIZE: f32 = 12.;
pub const STILL_RECHECK: Duration = Duration::from_millis(250);

pub struct StartupFacts<'a> {
    pub elapsed: Duration,
    pub connection: &'a ConnectionState,
    pub browser_sign_in: &'a BrowserSignInState,
    pub has_chats: bool,
    pub synced: bool,
    pub demo: bool,
    pub opening: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Loading {
        status: String,
        running: bool,
        hint: Option<String>,
    },
    Ready,
}

pub fn phase(facts: &StartupFacts) -> Phase {
    let content_ready = facts.has_chats || facts.synced || facts.demo;
    if facts.elapsed >= MIN_VISIBLE && content_ready && facts.opening.is_none() {
        return Phase::Ready;
    }
    let signing_in = matches!(facts.connection, ConnectionState::LoginRequired)
        || matches!(
            facts.browser_sign_in,
            BrowserSignInState::AwaitingSignIn | BrowserSignInState::Opening
        );
    if facts.elapsed >= GIVE_UP_AFTER && !(signing_in && !content_ready) {
        if !matches!(facts.browser_sign_in, BrowserSignInState::Failed(_)) {
            return Phase::Ready;
        }
    }
    if let Some(phase) = browser_phase(facts) {
        return phase;
    }
    let late_hint = (facts.elapsed >= HINT_AFTER && !content_ready)
        .then(|| tr("auth.splash.first_start_hint"));
    let loading = |status: String| Phase::Loading {
        status,
        running: true,
        hint: late_hint.clone(),
    };
    if matches!(facts.connection, ConnectionState::LoginRequired) {
        return Phase::Loading {
            status: tr("auth.splash.awaiting_browser"),
            running: false,
            hint: None,
        };
    }
    if facts.elapsed < STARTING_FOR {
        return loading(tr("auth.splash.starting"));
    }
    if content_ready {
        return loading(facts.opening.as_ref().map_or_else(
            || tr("auth.splash.syncing"),
            |title| tr("auth.splash.opening").replace("%{title}", title),
        ));
    }
    match facts.connection {
        ConnectionState::Connecting => loading(tr("auth.splash.signing_in")),
        ConnectionState::Online => loading(tr("auth.splash.loading_chats")),
        _ => loading(tr("auth.splash.cannot_reach")),
    }
}

fn browser_phase(facts: &StartupFacts) -> Option<Phase> {
    let late_browser_hint = (facts.elapsed >= HINT_AFTER)
        .then(|| tr("auth.splash.browser_still_loading_hint"));
    match facts.browser_sign_in {
        BrowserSignInState::Idle => None,
        BrowserSignInState::Opening => Some(Phase::Loading {
            status: tr("auth.splash.opening_browser"),
            running: true,
            hint: late_browser_hint,
        }),
        BrowserSignInState::AwaitingSignIn => Some(Phase::Loading {
            status: tr("auth.splash.awaiting_browser"),
            running: false,
            hint: late_browser_hint,
        }),
        BrowserSignInState::Finishing => Some(Phase::Loading {
            status: tr("auth.splash.finishing"),
            running: true,
            hint: None,
        }),
        BrowserSignInState::Failed(detail) => Some(Phase::Loading {
            status: tr("auth.splash.browser_failed"),
            running: false,
            hint: Some(detail.clone()),
        }),
    }
}

pub struct Splash {
    started: Instant,
    hidden_at: Option<Instant>,
    pub recheck_scheduled: bool,
}

impl Default for Splash {
    fn default() -> Self {
        Splash {
            started: Instant::now(),
            hidden_at: None,
            recheck_scheduled: false,
        }
    }
}

impl Splash {
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    pub fn is_up(&self) -> bool {
        self.hidden_at.is_none()
    }

    pub fn hide(&mut self) {
        self.hidden_at.get_or_insert_with(Instant::now);
    }

    pub fn content_rising(&self) -> Option<Duration> {
        self.hidden_at
            .map(|hidden_at| hidden_at.elapsed())
            .filter(|since| *since < CONTENT_RISE)
    }

    pub fn render(&self, phase: &Phase, cx: &App) -> Option<AnyElement> {
        let fading = self.hidden_at.map(|hidden_at| hidden_at.elapsed());
        if fading.is_some_and(|since| since >= FADE_OUT) {
            return None;
        }
        let still = cx.reduce_motion();
        let app_icon = crate::active_appearance::current(cx).app_icon;
        let (status, running, hint) = match phase {
            Phase::Loading {
                status,
                running,
                hint,
            } => (status.clone(), *running, hint.clone()),
            Phase::Ready => (String::new(), true, None),
        };
        let overlay = v_flex()
            .id("startup-splash")
            .debug_selector(|| "shell/splash".into())
            .occlude()
            .absolute()
            .top(px(TITLE_BAR_HEIGHT))
            .left_0()
            .right_0()
            .bottom_0()
            .items_center()
            .justify_center()
            .bg(theme::background())
            .child(logo(app_icon, still))
            .child(bar(running, still))
            .child(status_line(status, still))
            .child(
                div()
                    .mt(px(6.))
                    .h(px(16.))
                    .text_size(px(HINT_SIZE))
                    .text_color(theme::text_faint())
                    .children(hint),
            );
        Some(match fading {
            Some(_) if !still => overlay
                .with_animation(
                    "startup-splash-fade",
                    Animation::new(FADE_OUT).with_easing(ease_out_quint()),
                    |overlay, delta| overlay.opacity(1. - delta),
                )
                .into_any_element(),
            _ => overlay.into_any_element(),
        })
    }
}

fn logo(app_icon: crate::appearance::AppIcon, still: bool) -> AnyElement {
    let image = img(app_icon_image(app_icon)).size(px(LOGO_SIZE));
    let frame = div()
        .size(px(LOGO_SIZE * (1. + LOGO_BREATH)))
        .flex()
        .items_center()
        .justify_center();
    if still {
        return frame.child(image).into_any_element();
    }
    frame
        .child(image.with_animation(
            "startup-splash-logo",
            Animation::new(BREATH_PERIOD).repeat(),
            |image, delta| {
                let wave = (1. - (delta * std::f32::consts::TAU).cos()) / 2.;
                image.size(px(LOGO_SIZE * (1. + LOGO_BREATH * wave)))
            },
        ))
        .into_any_element()
}

fn bar(running: bool, still: bool) -> Div {
    let sweep_width = BAR_WIDTH * SWEEP_SHARE;
    let sweep = || {
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .w(px(sweep_width))
            .rounded(px(BAR_HEIGHT))
            .bg(theme::accent())
    };
    div()
        .relative()
        .mt(px(BAR_TOP_GAP))
        .w(px(BAR_WIDTH))
        .h(px(BAR_HEIGHT))
        .rounded(px(BAR_HEIGHT))
        .overflow_hidden()
        .bg(theme::border_strong())
        .when(!running, |bar| bar.invisible())
        .when(running && still, |bar| {
            bar.child(sweep().left(px((BAR_WIDTH - sweep_width) / 2.)))
        })
        .when(running && !still, |bar| {
            bar.child(
                sweep().with_animation(
                    "startup-splash-sweep",
                    Animation::new(SWEEP_PERIOD)
                        .repeat()
                        .with_easing(ease_in_out),
                    move |sweep, delta| {
                        let head = (BAR_WIDTH + sweep_width) * delta;
                        let start = (head - sweep_width).max(0.);
                        sweep.left(px(start)).w(px(head.min(BAR_WIDTH) - start))
                    },
                ),
            )
        })
}

fn status_line(status: String, still: bool) -> AnyElement {
    let line = div()
        .text_size(px(STATUS_SIZE))
        .text_color(theme::text_muted())
        .child(status.clone());
    let slot = div()
        .mt(px(STATUS_TOP_GAP))
        .h(px(STATUS_SIZE * 1.4))
        .relative();
    if still {
        return slot.child(line).into_any_element();
    }
    slot.child(line.relative().with_animation(
        ElementId::Name(format!("startup-splash-status-{status}").into()),
        Animation::new(STATUS_SWAP).with_easing(ease_out_quint()),
        |line, delta| {
            line.top(px(STATUS_SWAP_OFFSET * (1. - delta)))
                .opacity(delta)
        },
    ))
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{ConnectionState, Phase, StartupFacts, phase};
    use crate::browser_sign_in::BrowserSignInState;

    fn facts(elapsed_ms: u64, connection: &ConnectionState) -> StartupFacts<'_> {
        facts_with(elapsed_ms, connection, &BrowserSignInState::Idle)
    }

    fn facts_with<'a>(
        elapsed_ms: u64,
        connection: &'a ConnectionState,
        browser_sign_in: &'a BrowserSignInState,
    ) -> StartupFacts<'a> {
        StartupFacts {
            elapsed: Duration::from_millis(elapsed_ms),
            connection,
            browser_sign_in,
            has_chats: false,
            synced: false,
            demo: false,
            opening: None,
        }
    }

    fn status(phase: Phase) -> String {
        match phase {
            Phase::Loading { status, .. } => status,
            Phase::Ready => "ready".to_owned(),
        }
    }

    #[test]
    fn a_warm_cache_shows_the_app_after_the_minimum() {
        let online = ConnectionState::Connecting;
        let warm = StartupFacts {
            has_chats: true,
            ..facts(100, &online)
        };
        assert!(status(phase(&warm)).contains("Rusty Teams") || !status(phase(&warm)).is_empty());
        let warm = StartupFacts {
            has_chats: true,
            ..facts(450, &online)
        };
        assert_eq!(phase(&warm), Phase::Ready);
    }

    #[test]
    fn the_open_chat_holds_the_splash_until_its_messages_show() {
        let online = ConnectionState::Online;
        let opening = StartupFacts {
            has_chats: true,
            opening: Some("Release planning".to_owned()),
            ..facts(900, &online)
        };
        assert!(status(phase(&opening)).contains("Release planning"));
    }

    #[test]
    fn a_first_start_walks_through_sign_in_and_chats_with_a_late_hint() {
        let connecting = ConnectionState::Connecting;
        assert!(!status(phase(&facts(500, &connecting))).is_empty());
        let online = ConnectionState::Online;
        assert!(!status(phase(&facts(2000, &online))).is_empty());
        let Phase::Loading { hint, .. } = phase(&facts(9000, &online)) else {
            panic!("still loading")
        };
        assert!(hint.is_some());
        let synced = StartupFacts {
            synced: true,
            ..facts(9000, &online)
        };
        assert_eq!(phase(&synced), Phase::Ready);
    }

    #[test]
    fn sign_in_stops_the_bar_and_never_gives_up_without_content() {
        let login = ConnectionState::LoginRequired;
        let Phase::Loading { running, .. } = phase(&facts(60_000, &login)) else {
            panic!("still loading")
        };
        assert!(!running);
        let offline = ConnectionState::Failed("down".to_owned());
        assert_eq!(phase(&facts(31_000, &offline)), Phase::Ready);
    }

    #[test]
    fn browser_awaiting_sign_in_stops_the_bar() {
        let connecting = ConnectionState::Connecting;
        let awaiting = BrowserSignInState::AwaitingSignIn;
        let Phase::Loading { running, status, .. } =
            phase(&facts_with(2_000, &connecting, &awaiting))
        else {
            panic!("still loading")
        };
        assert!(!running);
        assert!(!status.is_empty());
    }

    #[test]
    fn browser_opening_keeps_the_bar_running() {
        let connecting = ConnectionState::Connecting;
        let opening = BrowserSignInState::Opening;
        let Phase::Loading { running, .. } = phase(&facts_with(2_000, &connecting, &opening))
        else {
            panic!("still loading")
        };
        assert!(running);
    }
}
