use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::*;

use crate::app_state::AppState;
use crate::notice::Notice;

struct NoticeToast;
struct UpdateFailedToast;

/// Mirrors `AppState::notice` as a toast bottom right; the state keeps the 6 s lifetime.
#[derive(Default)]
pub struct NoticeToasts {
    shown: Option<u64>,
    update_failure: Option<String>,
}

fn toast_placement() -> Anchor {
    Anchor::BottomRight
}

impl NoticeToasts {
    pub fn sync(
        &mut self,
        app: &Entity<AppState>,
        notice: Option<&Notice>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let current = notice.map(|notice| notice.id);
        if current == self.shown {
            return;
        }
        if let Some(previous) = self.shown.take() {
            window.defer(cx, move |window, cx| {
                window.remove_notification1::<NoticeToast>(previous, cx)
            });
        }
        let Some(notice) = notice else {
            return;
        };
        self.shown = Some(notice.id);
        let mut toast = Notification::new()
            .id1::<NoticeToast>(notice.id)
            .placement(toast_placement())
            .message(notice.text.clone())
            .autohide(false);
        if let Some(action) = notice.action.as_ref() {
            let label = SharedString::from(action.label.clone());
            let app = app.clone();
            toast = toast.action(move |_, _, _| {
                let app = app.clone();
                Button::new("notice-toast-action")
                    .primary()
                    .label(label.clone())
                    .on_click(move |_, _, cx| {
                        app.update(cx, |state, cx| state.run_notice_action(cx))
                    })
            });
        }
        let dismiss = app.clone();
        let notice_id = notice.id;
        toast = toast.on_close(move |_, cx| {
            dismiss.update(cx, |state, cx| {
                if state
                    .notice
                    .as_ref()
                    .is_some_and(|notice| notice.id == notice_id)
                {
                    state.dismiss_notice(cx);
                }
            })
        });
        window.defer(cx, move |window, cx| window.push_notification(toast, cx));
    }

    pub fn update_failed(&mut self, message: Option<&str>, window: &mut Window, cx: &mut App) {
        if message == self.update_failure.as_deref() {
            return;
        }
        self.update_failure = message.map(str::to_owned);
        let Some(message) = message else {
            return;
        };
        let toast = Notification::error(format!("Update failed: {message}"))
            .id::<UpdateFailedToast>()
            .placement(toast_placement());
        window.defer(cx, move |window, cx| window.push_notification(toast, cx));
    }
}
