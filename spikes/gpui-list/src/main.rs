mod data;

use std::{cell::RefCell, rc::Rc};

use data::{ChatMessage, Row};
use gpui_kit::component::{
    h_flex,
    input::{InputEvent, Textarea, TextareaState},
    message_scroller::{MessageScroller, MessageScrollerState},
    sidebar::{Sidebar, SidebarGroup, SidebarMenu, SidebarMenuItem},
    text::TextView,
    notification::Notification, v_flex, ActiveTheme, IconName, StyledExt as _, WindowExt as _,
};
use gpui_kit::*;

const MESSAGE_COUNT: usize = 5_000;

struct ChatApp {
    rows: Rc<RefCell<Vec<Row>>>,
    scroller: Entity<MessageScrollerState>,
    composer: Entity<TextareaState>,
    selected_chat: SharedString,
    _subscriptions: Vec<Subscription>,
}

impl ChatApp {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let rows = synthetic_rows();
        let scroller = cx.new(|cx| MessageScrollerState::new(rows.len(), cx));
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, 6)
                .submit_on_enter(true)
                .placeholder("Message - Enter sends, Shift+Enter newline")
        });
        let subscription = cx.subscribe_in(&composer, window, Self::on_composer_event);
        scroller.update(cx, |state, cx| state.scroll_to_end(cx));
        Self {
            rows: Rc::new(RefCell::new(rows)),
            scroller,
            composer,
            selected_chat: "Chat 1".into(),
            _subscriptions: vec![subscription],
        }
    }

    fn on_composer_event(
        &mut self,
        composer: &Entity<TextareaState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let InputEvent::PressEnter { shift: false, .. } = event else {
            return;
        };
        let text = composer.read(cx).value().trim().to_string();
        if text.is_empty() {
            return;
        }
        self.rows.borrow_mut().push(Row::Message(ChatMessage {
            author: "Me".into(),
            time: "now".into(),
            markdown: text,
        }));
        composer.update(cx, |state, cx| state.set_value("", window, cx));
        self.scroller.update(cx, |state, cx| {
            state.append(1, cx);
            state.scroll_to_end(cx);
        });
        window.push_notification(Notification::success("Sent"), cx);
    }

    fn sidebar(&self, cx: &mut Context<Self>) -> Sidebar<SidebarGroup<SidebarMenu>> {
        let chat_item = |label: &'static str, cx: &mut Context<Self>| {
            SidebarMenuItem::new(label)
                .icon(IconName::Inbox)
                .active(self.selected_chat == label)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selected_chat = label.into();
                    cx.notify();
                }))
        };
        let favorites = SidebarMenu::new().children([chat_item("Chat 1", cx), chat_item("Channel A / General", cx)]);
        let chats = SidebarMenu::new().children([chat_item("Chat 2", cx), chat_item("Chat 3", cx), chat_item("Chat 4", cx)]);
        let teams = SidebarMenu::new().children([
            SidebarMenuItem::new("Team Red")
                .icon(IconName::Folder)
                .default_open(true)
                .click_to_toggle(true)
                .children([chat_item("Red / General", cx), chat_item("Red / Dev", cx)]),
            SidebarMenuItem::new("Team Blue")
                .icon(IconName::Folder)
                .click_to_toggle(true)
                .children([chat_item("Blue / General", cx), chat_item("Blue / Ops", cx)]),
        ]);
        Sidebar::new("sidebar")
            .w(px(260.))
            .child(SidebarGroup::new("Favorites").child(favorites))
            .child(SidebarGroup::new("Chats").child(chats))
            .child(SidebarGroup::new("Teams").child(teams))
    }
}

fn synthetic_rows() -> Vec<Row> {
    data::synthetic_rows(MESSAGE_COUNT)
}

fn render_row(row: &Row, row_index: usize, cx: &App) -> AnyElement {
    match row {
        Row::DaySeparator(label) => div()
            .w_full()
            .py_2()
            .text_center()
            .text_xs()
            .text_color(cx.theme().muted_foreground)
            .child(label.clone())
            .into_any_element(),
        Row::Message(message) => v_flex()
            .w_full()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().font_semibold().child(message.author.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(cx.theme().muted_foreground)
                            .child(message.time.clone()),
                    ),
            )
            .child(TextView::markdown(("message", row_index), message.markdown.clone()))
            .into_any_element(),
    }
}

impl Render for ChatApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = self.rows.clone();
        let theme_background = cx.theme().background;
        h_flex()
            .size_full()
            .bg(theme_background)
            .child(self.sidebar(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(
                        div().flex_1().min_h_0().child(
                            MessageScroller::new("messages", self.scroller.clone(), move |row_index, _, cx| {
                                match rows.borrow().get(row_index) {
                                    Some(row) => render_row(row, row_index, cx),
                                    None => div().into_any_element(),
                                }
                            })
                            .with_bottom_fade(theme_background),
                        ),
                    )
                    .child(
                        div()
                            .p_3()
                            .border_t_1()
                            .border_color(cx.theme().border)
                            .child(Textarea::new(&self.composer)),
                    ),
            )
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                cx.new(|cx| ChatApp::new(window, cx))
            })
            .expect("failed to open window");
            cx.activate(true);
        });
}
