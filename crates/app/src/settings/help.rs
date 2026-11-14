use std::path::{Path, PathBuf};

use app_base::downloads::reveal_in_file_manager;
use app_base::support::{
    self, BundleFileEntry, SupportBundleEntry, ensure_dir, format_bytes, list_bundle_files,
    list_support_bundles,
};
use app_ui_kit::render::{layout_blocks, render_blocks, strip_image_placeholders};
use gpui_kit::assets::IconName;
use gpui_kit::component::scroll::ScrollableElement as _;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::rows::{ROW_INSET, caption, control_row, text_button, toggle_row};
use crate::app_state::AppState;
use crate::i18n::tr;
use crate::theme::{self, ElevatedSurface as _};
use crate::views::widgets::icon;

const COPY_ICON_SIZE: f32 = 14.;
const PREVIEW_LIMIT: usize = 256 * 1024;
const PREVIEW_RADIUS: f32 = 10.;
const PREVIEW_PADDING: f32 = 12.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreviewKind {
    Markdown,
    Text,
}

struct FilePreview {
    name: String,
    text: String,
    kind: PreviewKind,
    truncated: bool,
}

pub struct HelpView {
    app: Entity<AppState>,
    bundles: Vec<SupportBundleEntry>,
    expanded: Option<PathBuf>,
    expanded_files: Vec<BundleFileEntry>,
    preview: Option<FilePreview>,
}

impl HelpView {
    pub fn new(app: Entity<AppState>, cx: &mut Context<Self>) -> Self {
        let mut view = HelpView {
            app,
            bundles: Vec::new(),
            expanded: None,
            expanded_files: Vec::new(),
            preview: None,
        };
        view.reload_bundles();
        let _ = cx;
        view
    }

    pub(crate) fn is_previewing(&self) -> bool {
        self.preview.is_some()
    }

    pub(crate) fn reload_bundles(&mut self) {
        let Some(root) = support::support_bundles_dir() else {
            self.bundles.clear();
            self.expanded = None;
            self.expanded_files.clear();
            return;
        };
        self.bundles = list_support_bundles(&root).unwrap_or_default();
        if let Some(expanded) = self.expanded.clone() {
            if self.bundles.iter().any(|entry| entry.directory == expanded) {
                self.expanded_files = list_bundle_files(&expanded).unwrap_or_default();
            } else {
                self.expanded = None;
                self.expanded_files.clear();
            }
        }
    }

    fn toggle_expand(&mut self, directory: PathBuf, cx: &mut Context<Self>) {
        if self.expanded.as_ref() == Some(&directory) {
            self.expanded = None;
            self.expanded_files.clear();
        } else {
            self.expanded_files = list_bundle_files(&directory).unwrap_or_default();
            self.expanded = Some(directory);
        }
        cx.notify();
    }

    fn delete_bundle(&mut self, directory: PathBuf, cx: &mut Context<Self>) {
        let parent = support::support_bundles_dir();
        if let Err(error) = support::delete_support_bundle(&directory, parent.as_deref()) {
            app_base::diagnostics::record(format!("support delete: {error}"));
            return;
        }
        if self.expanded.as_ref() == Some(&directory) {
            self.expanded = None;
            self.expanded_files.clear();
        }
        self.preview = None;
        self.reload_bundles();
        cx.notify();
    }

    fn reveal_path(path: &Path) {
        let _ = ensure_dir(path);
        reveal_in_file_manager(path);
    }

    fn open_preview(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let name = path
            .file_name()
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let kind = if name.ends_with(".md") {
            PreviewKind::Markdown
        } else {
            PreviewKind::Text
        };
        match read_preview_text(&path) {
            Ok(ReadPreview {
                text,
                truncated,
                binary,
            }) => {
                self.preview = Some(FilePreview {
                    name,
                    text: if binary {
                        tr("settings.help.preview_binary")
                    } else {
                        text
                    },
                    kind: if binary { PreviewKind::Text } else { kind },
                    truncated: truncated && !binary,
                });
            }
            Err(error) => {
                app_base::diagnostics::record(format!("support preview: {error}"));
                self.preview = Some(FilePreview {
                    name,
                    text: tr("settings.help.preview_error"),
                    kind: PreviewKind::Text,
                    truncated: false,
                });
            }
        }
        cx.notify();
    }

    fn close_preview(&mut self, cx: &mut Context<Self>) {
        self.preview = None;
        cx.notify();
    }

    fn copy_path_button(id: &'static str, path: String) -> Stateful<Div> {
        div()
            .id(id)
            .cursor_pointer()
            .p(px(4.))
            .rounded(px(6.))
            .hover(|button| button.bg(theme::row_hover()))
            .tooltip(|window, cx| {
                Tooltip::new(tr("settings.help.copy_path_tooltip")).build(window, cx)
            })
            .on_click(move |_, _, cx| {
                cx.stop_propagation();
                cx.write_to_clipboard(ClipboardItem::new_string(path.clone()));
            })
            .child(icon(IconName::Copy, COPY_ICON_SIZE, theme::text_muted()))
    }

    fn path_row(
        &self,
        title: impl Into<SharedString>,
        path: String,
        copy_id: &'static str,
        open_id: &'static str,
        cx: &App,
    ) -> Div {
        let title = title.into();
        let open_path = PathBuf::from(path.clone());
        v_flex()
            .w_full()
            .px(px(ROW_INSET))
            .py(px(12.))
            .gap(px(8.))
            .rounded(px(8.))
            .child(
                h_flex()
                    .w_full()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(14.))
                            .line_height(px(20.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text())
                            .child(title),
                    )
                    .child(Self::copy_path_button(copy_id, path.clone()))
                    .child(text_button(open_id, tr("common.open")).on_click(move |_, _, _| {
                        Self::reveal_path(&open_path);
                    })),
            )
            .child(
                div()
                    .w_full()
                    .min_w(px(0.))
                    .font_family(cx.theme().mono_font_family.clone())
                    .text_size(px(12.))
                    .line_height(px(18.))
                    .text_color(theme::text_muted())
                    .whitespace_normal()
                    .child(path),
            )
    }

    fn bundle_row(&self, entry: &SupportBundleEntry, cx: &mut Context<Self>) -> Div {
        let directory = entry.directory.clone();
        let toggle_dir = directory.clone();
        let delete_dir = directory.clone();
        let expanded = self.expanded.as_ref() == Some(&directory);
        v_flex()
            .w_full()
            .child(
                h_flex()
                    .w_full()
                    .px(px(ROW_INSET))
                    .py(px(10.))
                    .gap(px(12.))
                    .items_center()
                    .rounded(px(8.))
                    .when(expanded, |row| row.bg(theme::popup_hover()))
                    .child(
                        h_flex()
                            .id(SharedString::from(format!(
                                "settings-help-bundle-{}",
                                entry.name
                            )))
                            .flex_1()
                            .min_w(px(0.))
                            .gap(px(8.))
                            .items_center()
                            .cursor_pointer()
                            .hover(|row| row.bg(theme::popup_hover()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.toggle_expand(toggle_dir.clone(), cx);
                            }))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .text_size(px(14.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme::text())
                                    .child(entry.name.clone()),
                            ),
                    )
                    .child(
                        text_button(
                            SharedString::from(format!("settings-help-delete-{}", entry.name)),
                            tr("common.delete"),
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.delete_bundle(delete_dir.clone(), cx);
                        })),
                    ),
            )
            .when(expanded, |column| column.child(self.expanded_panel(cx)))
    }

    fn expanded_panel(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .w_full()
            .px(px(ROW_INSET + 8.))
            .pb(px(8.))
            .gap(px(4.))
            .children(self.expanded_files.iter().map(|file| {
                let path = file.path.clone();
                let label = format!("{} · {}", file.name, format_bytes(file.size_bytes));
                div()
                    .id(SharedString::from(format!(
                        "settings-help-file-{}",
                        file.name
                    )))
                    .w_full()
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|row| row.bg(theme::popup_hover()))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.open_preview(path.clone(), cx);
                    }))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme::text_muted())
                            .child(label),
                    )
            }))
    }

    fn packages_section(&self, cx: &mut Context<Self>) -> Div {
        v_flex()
            .w_full()
            .child(caption(tr("settings.help.caption_packages")))
            .when(self.bundles.is_empty(), |section| {
                section.child(
                    div()
                        .px(px(ROW_INSET))
                        .py(px(8.))
                        .text_size(px(13.))
                        .text_color(theme::text_muted())
                        .child(tr("settings.help.packages_empty")),
                )
            })
            .children(
                self.bundles
                    .iter()
                    .map(|entry| self.bundle_row(entry, cx))
                    .collect::<Vec<_>>(),
            )
    }

    fn preview_body(&self, preview: &FilePreview, cx: &App) -> AnyElement {
        match preview.kind {
            PreviewKind::Markdown => {
                // Full document markdown (headings, tables, lists) — not the light chat dialect.
                let html = teams_core::Draft::from_markdown(&preview.text).to_html();
                let blocks =
                    strip_image_placeholders(layout_blocks(&teams_core::html_to_spans(&html)));
                div()
                    .w_full()
                    .min_w(px(0.))
                    .text_size(px(13.))
                    .text_color(theme::text())
                    .whitespace_normal()
                    .child(render_blocks(&blocks, "settings-help-preview", false, cx))
                    .into_any_element()
            }
            PreviewKind::Text => div()
                .w_full()
                .min_w(px(0.))
                .font_family(cx.theme().mono_font_family.clone())
                .text_size(px(12.))
                .line_height(px(18.))
                .text_color(theme::text_muted())
                .whitespace_normal()
                .child(preview.text.clone())
                .into_any_element(),
        }
    }

    fn preview_panel(&self, cx: &mut Context<Self>) -> Div {
        let preview = self.preview.as_ref().expect("preview open");
        v_flex()
            .w_full()
            .h_full()
            .min_h_0()
            .gap(px(8.))
            .px(px(ROW_INSET))
            .pb(px(ROW_INSET))
            .child(
                h_flex()
                    .w_full()
                    .flex_none()
                    .gap(px(8.))
                    .items_center()
                    .child(
                        text_button("settings-help-preview-back", tr("common.close")).on_click(
                            cx.listener(|this, _, _, cx| this.close_preview(cx)),
                        ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(px(14.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme::text())
                            .child(preview.name.clone()),
                    ),
            )
            .when(preview.truncated, |column| {
                column.child(
                    div()
                        .flex_none()
                        .text_size(px(12.))
                        .text_color(theme::text_muted())
                        .child(tr("settings.help.preview_truncated")),
                )
            })
            .child(
                div()
                    .id("settings-help-preview-scroll")
                    .w_full()
                    .flex_1()
                    .min_h_0()
                    .p(px(PREVIEW_PADDING))
                    .rounded(px(PREVIEW_RADIUS))
                    .dialog_surface()
                    .overflow_y_scrollbar()
                    .child(self.preview_body(preview, cx)),
            )
    }

    fn help_home(&self, cx: &mut Context<Self>) -> Div {
        let info = self.app.read(cx).client_info();
        let summary = info.markdown();
        let copy_summary = summary.clone();
        let logs_path = info.logs_root.clone();
        let bundles_path = info.support_bundles_root.clone();
        let create_app = self.app.clone();
        let issue_app = self.app.clone();
        v_flex()
            .w_full()
            .gap(px(4.))
            .child(caption(tr("settings.help.caption_client")))
            .child(control_row(
                tr("settings.help.version"),
                format!("{} ({})", info.version, info.build),
                div(),
            ))
            .child(control_row(
                tr("settings.help.platform"),
                format!("{} / {} / {}", info.os, info.arch, info.session),
                div(),
            ))
            .child(control_row(
                tr("settings.help.locale"),
                format!("{} · {}", info.ui_locale, info.timezone),
                div(),
            ))
            .child(control_row(
                tr("settings.help.signed_in"),
                if info.signed_in {
                    tr("settings.help.yes")
                } else {
                    tr("settings.help.no")
                },
                div(),
            ))
            .child({
                let debug_on = self.app.read(cx).diagnostics_enabled();
                let toggle_app = self.app.clone();
                toggle_row(
                    "settings-help-debug-log",
                    tr("settings.help.debug_logging"),
                    if debug_on {
                        tr("settings.help.debug_logging_on_detail")
                    } else {
                        tr("settings.help.debug_logging_off_detail")
                    },
                    theme::text_muted(),
                    debug_on,
                )
                .on_click(cx.listener(move |_, _, _, cx| {
                    toggle_app.update(cx, |state, cx| {
                        state.set_diagnostics_enabled(!debug_on, cx);
                    });
                    cx.notify();
                }))
            })
            .child({
                let clear_app = self.app.clone();
                control_row(
                    tr("settings.help.clear_logs"),
                    tr("settings.help.clear_logs_detail"),
                    text_button("settings-help-clear-logs", tr("settings.help.clear_logs_button"))
                        .on_click(cx.listener(move |_, _, _, cx| {
                            clear_app.update(cx, |state, cx| state.clear_diagnostic_logs(cx));
                            cx.notify();
                        })),
                )
            })
            .child(caption(tr("settings.help.caption_paths")))
            .child(self.path_row(
                tr("settings.help.logs_path"),
                logs_path,
                "settings-help-copy-logs",
                "settings-help-open-logs-root",
                cx,
            ))
            .child(self.path_row(
                tr("settings.help.bundles_path"),
                bundles_path,
                "settings-help-copy-bundles",
                "settings-help-open-bundles-root",
                cx,
            ))
            .child(self.packages_section(cx))
            .child(caption(tr("settings.help.caption_actions")))
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme::text_muted())
                    .pb(px(8.))
                    .px(px(ROW_INSET))
                    .child(tr("settings.help.detail")),
            )
            .child(control_row(
                tr("settings.help.create_bundle"),
                tr("settings.help.create_bundle_detail"),
                text_button("settings-help-bundle", tr("settings.help.create_bundle_button"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        create_app.update(cx, |state, _| {
                            match state.create_support_bundle() {
                                Ok(bundle) => AppState::reveal_support_bundle(&bundle),
                                Err(error) => app_base::diagnostics::record(format!(
                                    "support bundle: {error}"
                                )),
                            }
                        });
                        this.reload_bundles();
                        cx.notify();
                    })),
            ))
            .child(control_row(
                tr("settings.help.open_issue"),
                tr("settings.help.open_issue_detail"),
                text_button("settings-help-issue", tr("settings.help.open_issue_button")).on_click(
                    cx.listener(move |this, _, _, cx| {
                        issue_app.update(cx, |state, cx| {
                            if let Err(error) = state.open_github_issue_with_bundle(cx) {
                                app_base::diagnostics::record(format!("support issue: {error}"));
                            }
                        });
                        this.reload_bundles();
                        cx.notify();
                    }),
                ),
            ))
            .child(control_row(
                tr("settings.help.copy_summary"),
                tr("settings.help.copy_summary_detail"),
                text_button("settings-help-copy", tr("settings.help.copy_button")).on_click(
                    move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_summary.clone()));
                    },
                ),
            ))
            .child(control_row(
                tr("settings.help.open_logs"),
                tr("settings.help.open_logs_detail"),
                text_button("settings-help-logs", tr("common.open")).on_click(move |_, _, _| {
                    AppState::reveal_logs_folder();
                }),
            ))
    }
}

struct ReadPreview {
    text: String,
    truncated: bool,
    binary: bool,
}

fn read_preview_text(path: &Path) -> std::io::Result<ReadPreview> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path)?;
    let mut buf = vec![0u8; PREVIEW_LIMIT + 1];
    let read = file.read(&mut buf)?;
    let truncated = read > PREVIEW_LIMIT;
    let slice = &buf[..read.min(PREVIEW_LIMIT)];
    match std::str::from_utf8(slice) {
        Ok(text) => Ok(ReadPreview {
            text: text.to_owned(),
            truncated,
            binary: false,
        }),
        Err(_) => Ok(ReadPreview {
            text: String::new(),
            truncated: false,
            binary: true,
        }),
    }
}

impl Render for HelpView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let previewing = self.preview.is_some();
        v_flex()
            .w_full()
            .when(previewing, |root| root.h_full().min_h_0())
            .debug_selector(|| "settings/help".into())
            .child(if previewing {
                self.preview_panel(cx)
            } else {
                self.help_home(cx)
            })
    }
}
