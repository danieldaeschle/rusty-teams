use chrono::{DateTime, Utc};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::{Disableable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use teams_core::{CloudFile, DriveEntry, DriveFolder, QuickSite};

use crate::i18n::tr;
use crate::app_state::AppState;
use crate::runtime;
use crate::theme::{self, ElevatedSurface as _};
use crate::views::shared_tab::{entry_badge, modified_text};
use crate::views::widgets::symbol;

pub const CLOUD_PICKER_TAG: &str = "composer/cloud-picker";
pub const ATTACH_CLOUD_LABEL: &str = "Attach cloud files";
const PANEL_WIDTH: f32 = 1052.;
const PANEL_HEIGHT: f32 = 742.;
const PANEL_MARGIN: f32 = 32.;
const NAV_WIDTH: f32 = 210.;
const NAV_ROW_HEIGHT: f32 = 32.;
const ROW_HEIGHT: f32 = 48.;
const WHEN_WIDTH: f32 = 150.;
const WHO_WIDTH: f32 = 170.;
const CHECK_WIDTH: f32 = 32.;
const PADDING: f32 = 24.;
const BACKDROP_OPACITY: f32 = 0.55;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Place {
    Home,
    MyFiles,
    Shared,
    Site(usize),
}

impl Place {
    fn columns(self) -> [&'static str; 3] {
        match self {
            Place::Home => ["Name", "Opened", "Owner"],
            Place::Shared => ["Name", "Date shared", "Shared by"],
            Place::MyFiles | Place::Site(_) => ["Name", "Modified", "Modified By"],
        }
    }
}

struct Crumb {
    name: String,
    folder: DriveFolder,
}

enum Rows {
    Loading,
    Failed,
    Ready(Vec<DriveEntry>),
}

pub enum CloudPickerEvent {
    Attach(Vec<CloudFile>),
    Close,
}

pub struct CloudPicker {
    app: Entity<AppState>,
    place: Place,
    sites: Vec<QuickSite>,
    crumbs: Vec<Crumb>,
    rows: Rows,
    selected: Vec<DriveEntry>,
    generation: u64,
    focus_handle: FocusHandle,
}

impl EventEmitter<CloudPickerEvent> for CloudPicker {}

impl Focusable for CloudPicker {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

fn recent_row((file, owner): (CloudFile, Option<String>)) -> Option<DriveEntry> {
    Some(DriveEntry {
        drive_id: file.drive_id?,
        id: file.item_id?,
        name: file.name,
        web_url: file.web_url,
        size: file.size,
        modified_at: file.last_opened,
        modified_by: owner,
        child_count: None,
        download_url: None,
    })
}

fn demo_rows() -> Vec<DriveEntry> {
    let now = Utc::now();
    ["Q4 planning.pptx", "Budget 2027.xlsx", "Release notes.docx"]
        .iter()
        .enumerate()
        .map(|(index, name)| DriveEntry {
            drive_id: "demo-drive".into(),
            id: format!("demo-{index}"),
            name: (*name).into(),
            web_url: format!("https://contoso.sharepoint.com/demo/{index}"),
            size: 20_000,
            modified_at: Some(now - chrono::Duration::hours(3 * index as i64 + 1)),
            modified_by: Some("Megan Bowen".into()),
            child_count: None,
            download_url: None,
        })
        .collect()
}

pub fn opened_ago(time: DateTime<Utc>, now: DateTime<Utc>) -> Option<String> {
    let minutes = (now - time).num_minutes().max(0);
    match minutes {
        0 => Some("Just now".into()),
        1..60 => Some(format!("{minutes}m ago")),
        60..1440 => Some(format!("{}h ago", minutes / 60)),
        _ => None,
    }
}

impl CloudPicker {
    pub fn new(app: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let mut picker = CloudPicker {
            app,
            place: Place::Home,
            sites: Vec::new(),
            crumbs: Vec::new(),
            rows: Rows::Loading,
            selected: Vec::new(),
            generation: 0,
            focus_handle: cx.focus_handle(),
        };
        picker.load_sites(cx);
        picker.open_place(Place::Home, cx);
        picker
    }

    fn load_sites(&mut self, cx: &mut Context<Self>) {
        let Some(engine) = self.app.read(cx).engine.clone() else {
            return;
        };
        let receiver = runtime::spawn(async move { engine.picker_sites().await });
        cx.spawn(async move |this, cx| {
            if let Ok(Ok(sites)) = receiver.await {
                this.update(cx, |this, cx| {
                    this.sites = sites;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    fn open_place(&mut self, place: Place, cx: &mut Context<Self>) {
        self.place = place;
        self.crumbs.clear();
        self.generation += 1;
        let generation = self.generation;
        self.rows = Rows::Loading;
        cx.notify();
        if self.app.read(cx).mode.demo {
            self.rows = Rows::Ready(if place == Place::Home {
                demo_rows()
            } else {
                Vec::new()
            });
            return;
        }
        let Some(engine) = self.app.read(cx).engine.clone() else {
            self.rows = Rows::Failed;
            return;
        };
        let site = match place {
            Place::Site(index) => self.sites.get(index).map(|site| site.id.clone()),
            _ => None,
        };
        let receiver = runtime::spawn(async move {
            match place {
                Place::Home => Ok((
                    None,
                    engine
                        .picker_recent()
                        .await?
                        .into_iter()
                        .filter_map(recent_row)
                        .collect(),
                )),
                Place::Shared => Ok((None, engine.picker_shared().await?)),
                Place::MyFiles | Place::Site(_) => {
                    let root = engine.picker_root(site.as_deref()).await?;
                    let entries = engine.picker_folder(&root).await?;
                    Ok((Some(root), entries))
                }
            }
        });
        cx.spawn(async move |this, cx| {
            let result: Option<teams_core::Result<(Option<DriveFolder>, Vec<DriveEntry>)>> =
                receiver.await.ok();
            this.update(cx, |this, cx| {
                if this.generation != generation {
                    return;
                }
                match result {
                    Some(Ok((root, entries))) => {
                        if let Some(folder) = root {
                            this.crumbs.push(Crumb {
                                name: this.root_name(),
                                folder,
                            });
                        }
                        this.rows = Rows::Ready(entries);
                    }
                    _ => this.rows = Rows::Failed,
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn root_name(&self) -> String {
        match self.place {
            Place::Site(_) => "Documents".into(),
            _ => "My files".into(),
        }
    }

    fn open_folder(&mut self, name: String, folder: DriveFolder, cx: &mut Context<Self>) {
        self.crumbs.push(Crumb {
            name,
            folder: folder.clone(),
        });
        self.list_folder(folder, cx);
    }

    fn back_to(&mut self, depth: usize, cx: &mut Context<Self>) {
        self.crumbs.truncate(depth + 1);
        if let Some(crumb) = self.crumbs.last() {
            let folder = crumb.folder.clone();
            self.list_folder(folder, cx);
        }
    }

    fn list_folder(&mut self, folder: DriveFolder, cx: &mut Context<Self>) {
        self.generation += 1;
        let generation = self.generation;
        self.rows = Rows::Loading;
        cx.notify();
        let Some(engine) = self.app.read(cx).engine.clone() else {
            self.rows = Rows::Failed;
            return;
        };
        let receiver = runtime::spawn(async move { engine.picker_folder(&folder).await });
        cx.spawn(async move |this, cx| {
            let result = receiver.await;
            this.update(cx, |this, cx| {
                if this.generation == generation {
                    this.rows = match result {
                        Ok(Ok(entries)) => Rows::Ready(entries),
                        _ => Rows::Failed,
                    };
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    fn click_row(&mut self, entry: DriveEntry, cx: &mut Context<Self>) {
        if entry.is_folder() {
            self.open_folder(entry.name.clone(), entry.folder(), cx);
            return;
        }
        match self
            .selected
            .iter()
            .position(|picked| picked.id == entry.id)
        {
            Some(index) => {
                self.selected.remove(index);
            }
            None => self.selected.push(entry),
        }
        cx.notify();
    }

    fn attach(&mut self, cx: &mut Context<Self>) {
        let files = self
            .selected
            .iter()
            .map(CloudFile::from)
            .collect::<Vec<_>>();
        if !files.is_empty() {
            cx.emit(CloudPickerEvent::Attach(files));
        }
    }

    fn title(&self) -> String {
        match self.place {
            Place::Home => "Home".into(),
            Place::Shared => "Shared with you".into(),
            Place::MyFiles | Place::Site(_) => self
                .crumbs
                .last()
                .map_or_else(|| self.root_name(), |crumb| crumb.name.clone()),
        }
    }

    fn nav_row(
        &self,
        id: impl Into<ElementId>,
        glyph: &'static str,
        label: String,
        place: Place,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let active = self.place == place;
        h_flex()
            .id(id)
            .h(px(NAV_ROW_HEIGHT))
            .flex_none()
            .px(px(10.))
            .gap(px(10.))
            .items_center()
            .rounded(px(6.))
            .cursor_pointer()
            .text_size(px(13.))
            .text_color(if active {
                theme::text_strong()
            } else {
                theme::text()
            })
            .when(active, |row| {
                row.bg(theme::selection()).font_weight(FontWeight::SEMIBOLD)
            })
            .hover(|row| row.bg(theme::row_hover()))
            .child(symbol(
                glyph,
                18.,
                if active {
                    theme::accent_text()
                } else {
                    theme::text_soft()
                },
            ))
            .child(div().flex_1().min_w_0().truncate().child(label))
            .on_click(cx.listener(move |this, _, _, cx| this.open_place(place, cx)))
    }

    fn navigation(&self, cx: &mut Context<Self>) -> Div {
        let section = |label: &'static str| {
            div()
                .pt(px(20.))
                .pb(px(6.))
                .px(px(10.))
                .text_size(px(13.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme::text())
                .child(label)
        };
        let mut sites = Vec::with_capacity(self.sites.len());
        for (index, site) in self.sites.iter().enumerate() {
            sites.push(self.nav_row(
                ("cloud-picker-site", index),
                "domain",
                site.name.clone(),
                Place::Site(index),
                cx,
            ));
        }
        v_flex()
            .w(px(NAV_WIDTH))
            .flex_none()
            .h_full()
            .px(px(8.))
            .py(px(PADDING))
            .child(
                div()
                    .px(px(10.))
                    .pb(px(16.))
                    .text_size(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_strong())
                    .child("OneDrive"),
            )
            .child(self.nav_row("cloud-picker-home", "home", "Home".into(), Place::Home, cx))
            .child(self.nav_row(
                "cloud-picker-mine",
                "folder",
                "My files".into(),
                Place::MyFiles,
                cx,
            ))
            .child(self.nav_row(
                "cloud-picker-shared",
                "group",
                "Shared".into(),
                Place::Shared,
                cx,
            ))
            .when(!self.sites.is_empty(), |nav| {
                nav.child(section("Quick access")).child(
                    v_flex()
                        .id("cloud-picker-sites")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scrollbar()
                        .children(sites),
                )
            })
    }

    fn breadcrumb(&self, cx: &mut Context<Self>) -> Div {
        let last = self.crumbs.len().saturating_sub(1);
        let mut trail = h_flex().gap(px(4.)).items_center().min_w_0();
        for (depth, crumb) in self.crumbs.iter().enumerate() {
            if depth > 0 {
                trail = trail.child(symbol("chevron_right", 16., theme::text_soft()));
            }
            let current = depth == last;
            trail = trail.child(
                div()
                    .id(("cloud-picker-crumb", depth))
                    .truncate()
                    .text_size(px(if current { 20. } else { 14. }))
                    .font_weight(if current {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_color(if current {
                        theme::text_strong()
                    } else {
                        theme::text_soft()
                    })
                    .when(!current, |crumb| {
                        crumb
                            .cursor_pointer()
                            .hover(|crumb| crumb.text_color(theme::text()))
                            .on_click(cx.listener(move |this, _, _, cx| this.back_to(depth, cx)))
                    })
                    .child(crumb.name.clone()),
            );
        }
        trail
    }

    fn header(&self, cx: &mut Context<Self>) -> Div {
        let site = match self.place {
            Place::Site(index) => self.sites.get(index).map(|site| site.name.clone()),
            _ => None,
        };
        let title = if self.crumbs.len() > 1 {
            self.breadcrumb(cx)
        } else {
            h_flex().child(
                div()
                    .text_size(px(20.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme::text_strong())
                    .child(self.title()),
            )
        };
        v_flex()
            .gap(px(2.))
            .children(site.map(|name| {
                div()
                    .text_size(px(13.))
                    .text_color(theme::text_soft())
                    .child(name)
            }))
            .child(title)
    }

    fn column_header(&self) -> Div {
        let [name, when, who] = self.place.columns();
        h_flex()
            .h(px(40.))
            .flex_none()
            .px(px(12.))
            .gap(px(12.))
            .items_center()
            .border_b_1()
            .border_color(theme::border())
            .text_size(px(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme::text_soft())
            .child(div().w(px(CHECK_WIDTH)).flex_none())
            .child(div().flex_1().min_w_0().child(name))
            .child(div().w(px(WHEN_WIDTH)).flex_none().child(when))
            .child(div().w(px(WHO_WIDTH)).flex_none().child(who))
    }

    fn when_text(&self, entry: &DriveEntry, now: DateTime<Utc>) -> String {
        match (self.place, entry.modified_at) {
            (Place::Home, Some(opened)) => {
                opened_ago(opened, now).unwrap_or_else(|| modified_text(entry))
            }
            _ => modified_text(entry),
        }
    }

    fn row(
        &self,
        index: usize,
        entry: &DriveEntry,
        now: DateTime<Utc>,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let picked = self.selected.iter().any(|selected| selected.id == entry.id);
        let folder = entry.is_folder();
        let clicked = entry.clone();
        h_flex()
            .id(("cloud-picker-row", index))
            .h(px(ROW_HEIGHT))
            .flex_none()
            .px(px(12.))
            .gap(px(12.))
            .items_center()
            .border_b_1()
            .border_color(theme::border())
            .cursor_pointer()
            .text_size(px(13.))
            .text_color(theme::text_soft())
            .when(picked, |row| row.bg(theme::selection()))
            .hover(|row| row.bg(theme::row_hover()))
            .child(div().w(px(CHECK_WIDTH)).flex_none().when(!folder, |cell| {
                cell.child(Checkbox::new(("cloud-picker-check", index)).checked(picked))
            }))
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(10.))
                    .items_center()
                    .child(entry_badge(entry))
                    .child(
                        div()
                            .truncate()
                            .text_color(theme::text())
                            .child(entry.name.clone()),
                    ),
            )
            .child(
                div()
                    .w(px(WHEN_WIDTH))
                    .flex_none()
                    .truncate()
                    .child(self.when_text(entry, now)),
            )
            .child(
                div()
                    .w(px(WHO_WIDTH))
                    .flex_none()
                    .truncate()
                    .child(entry.modified_by.clone().unwrap_or_default()),
            )
            .on_click(cx.listener(move |this, _, _, cx| this.click_row(clicked.clone(), cx)))
    }

    fn table(&self, cx: &mut Context<Self>) -> AnyElement {
        let note = |text: &'static str| {
            div()
                .p(px(16.))
                .text_size(px(13.))
                .text_color(theme::text_soft())
                .child(text)
                .into_any_element()
        };
        let now = Utc::now();
        let body: Vec<AnyElement> = match &self.rows {
            Rows::Loading => vec![note("Loading ...")],
            Rows::Failed => vec![note("Couldn't load files.")],
            Rows::Ready(entries) if entries.is_empty() => vec![note("This folder is empty.")],
            Rows::Ready(entries) => entries
                .iter()
                .enumerate()
                .map(|(index, entry)| self.row(index, entry, now, cx).into_any_element())
                .collect(),
        };
        v_flex()
            .flex_1()
            .min_h_0()
            .rounded(px(10.))
            .bg(theme::surface_raised())
            .overflow_hidden()
            .child(self.column_header())
            .child(
                v_flex()
                    .id("cloud-picker-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scrollbar()
                    .children(body),
            )
            .into_any_element()
    }
}

impl Render for CloudPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let width = px(PANEL_WIDTH).min(viewport.width - px(2. * PANEL_MARGIN));
        let height = px(PANEL_HEIGHT).min(viewport.height - px(2. * PANEL_MARGIN));
        let can_attach = !self.selected.is_empty();
        let panel = h_flex()
            .id(CLOUD_PICKER_TAG)
            .debug_selector(|| CLOUD_PICKER_TAG.into())
            .w(width)
            .h(height)
            .rounded(px(12.))
            .dialog_surface()
            .text_color(theme::text())
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(self.navigation(cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .p(px(PADDING))
                    .gap(px(16.))
                    .child(self.header(cx))
                    .child(self.table(cx))
                    .child(
                        h_flex()
                            .justify_end()
                            .gap(px(8.))
                            .child(
                                Button::new("cloud-picker-attach")
                                    .primary()
                                    .label(tr("common.attach"))
                                    .disabled(!can_attach)
                                    .on_click(cx.listener(|this, _, _, cx| this.attach(cx))),
                            )
                            .child(Button::new("cloud-picker-cancel").label(tr("common.cancel")).on_click(
                                cx.listener(|_, _, _, cx| cx.emit(CloudPickerEvent::Close)),
                            )),
                    ),
            );
        deferred(
            anchored().position(point(px(0.), px(0.))).child(
                div()
                    .id("cloud-picker-backdrop")
                    .track_focus(&self.focus_handle)
                    .w(viewport.width)
                    .h(viewport.height)
                    .flex()
                    .items_center()
                    .justify_center()
                    .bg(black().opacity(BACKDROP_OPACITY))
                    .occlude()
                    .on_key_down(cx.listener(|_, event: &KeyDownEvent, _, cx| {
                        if event.keystroke.key == "escape" {
                            cx.emit(CloudPickerEvent::Close);
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, _, cx| cx.emit(CloudPickerEvent::Close)),
                    )
                    .child(panel),
            ),
        )
        .with_priority(3)
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone as _, Utc};

    use super::{Place, opened_ago};

    #[test]
    fn home_shows_hours_ago_like_the_onedrive_picker() {
        let now = Utc.with_ymd_and_hms(2026, 11, 10, 22, 0, 0).unwrap();
        assert_eq!(
            opened_ago(now - chrono::Duration::hours(9), now).as_deref(),
            Some("9h ago")
        );
        assert_eq!(
            opened_ago(now - chrono::Duration::minutes(5), now).as_deref(),
            Some("5m ago")
        );
        assert_eq!(opened_ago(now - chrono::Duration::days(3), now), None);
    }

    #[test]
    fn each_place_has_the_picker_columns() {
        assert_eq!(Place::Home.columns(), ["Name", "Opened", "Owner"]);
        assert_eq!(
            Place::Shared.columns(),
            ["Name", "Date shared", "Shared by"]
        );
        assert_eq!(
            Place::Site(0).columns(),
            ["Name", "Modified", "Modified By"]
        );
    }
}
