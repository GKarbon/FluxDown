//! Existing-task BT files, shared by the docked details and native detail window.
use std::collections::HashSet;
use std::time::{Duration, Instant};

use fluxdown_protocol::{BtTaskFilesDto, SetBtTaskFilesParams};
use fluxdown_ui_components::{ControlExt as _, check_row, tabular_numbers};
use fluxdown_ui_theme::active_theme;
use gpui::{
    AnyElement, Context, IntoElement as _, ParentElement as _, SharedString, Styled as _, Window,
    div,
};
use gpui_component::{
    Disableable as _, WindowExt as _,
    button::{Button, ButtonVariants as _},
    h_flex,
    progress::Progress,
    v_flex,
};

use super::{DetailTab, TaskDetailView};
use crate::{
    controller::{DownloadsCommand, DownloadsResult},
    model::{RowKey, TaskState, format_bytes},
};

#[derive(Default)]
pub(super) struct TaskFilesState {
    data: Option<BtTaskFilesDto>,
    draft: Option<HashSet<i32>>,
    loading: bool,
    saving: bool,
    failed: bool,
    last_fetch: Option<Instant>,
    revision: u64,
}

impl TaskFilesState {
    fn selected(&self) -> HashSet<i32> {
        self.draft.clone().unwrap_or_else(|| {
            self.data
                .as_ref()
                .into_iter()
                .flat_map(|data| &data.files)
                .filter(|file| file.selected)
                .map(|file| file.index)
                .collect()
        })
    }

    fn can_clean(&self, state: TaskState, seeding: bool, online: bool) -> bool {
        online
            && matches!(
                state,
                TaskState::Paused | TaskState::Failed | TaskState::Completed
            )
            && !seeding
            && !self.saving
            && !self.changed(&self.selected())
            && self.data.as_ref().is_some_and(|data| {
                data.metadata_ready && data.files.iter().any(|file| !file.selected)
            })
    }

    fn changed(&self, selected: &HashSet<i32>) -> bool {
        self.data.as_ref().is_some_and(|data| {
            data.files
                .iter()
                .any(|file| file.selected != selected.contains(&file.index))
        })
    }
}

impl TaskDetailView {
    pub(super) fn tick_files(&mut self, cx: &mut Context<Self>) {
        if self
            .files
            .last_fetch
            .is_none_or(|at| at.elapsed() >= Duration::from_secs(2))
        {
            self.fetch_files(cx);
        }
    }

    pub(super) fn fetch_files(&mut self, cx: &mut Context<Self>) {
        if self.closed
            || !self.activity_online
            || self.tab != DetailTab::Files
            || !self.is_bt()
            || self.files.loading
            || self.files.saving
        {
            return;
        }
        self.files.loading = true;
        self.files.last_fetch = Some(Instant::now());
        let generation = self.task_generation;
        let revision = self.files.revision;
        let task_id = self.task_id.clone();
        let future = self.port.execute(DownloadsCommand::TaskBtFiles {
            task_id: task_id.clone(),
        });
        cx.spawn(async move |this, cx| {
            let result = future.await;
            let Ok(()) = this.update(cx, |this, cx| {
                if this.task_generation != generation || this.files.revision != revision {
                    return;
                }
                this.files.loading = false;
                match result {
                    Ok(DownloadsResult::TaskBtFiles(data)) if data.task_id == task_id => {
                        this.files.data = Some(data);
                        this.files.failed = false;
                    }
                    _ => this.files.failed = true,
                }
                cx.notify();
            }) else {
                return; // The detail view was closed while the request was running.
            };
        })
        .detach();
        cx.notify();
    }

    fn select_all_files(&mut self, selected: bool, cx: &mut Context<Self>) {
        if !self.activity_online || self.files.saving || self.files.failed {
            return;
        }
        let Some(data) = self.files.data.as_ref().filter(|data| data.metadata_ready) else {
            return;
        };
        self.files.draft = Some(if selected {
            data.files.iter().map(|file| file.index).collect()
        } else {
            HashSet::new()
        });
        cx.notify();
    }

    fn save_files(&mut self, cx: &mut Context<Self>) {
        let selected = self.files.selected();
        if !self.activity_online
            || self.files.saving
            || selected.is_empty()
            || !self.files.changed(&selected)
        {
            return;
        }
        let mut indices: Vec<_> = selected.into_iter().collect();
        indices.sort_unstable();
        self.change_files(
            DownloadsCommand::SetBtFiles(SetBtTaskFilesParams {
                task_id: self.task_id.clone(),
                indices,
            }),
            cx,
        );
    }

    fn clean_files_enabled(&self) -> bool {
        self.store
            .get(&RowKey::Local(self.task_id.clone()))
            .is_some_and(|row| {
                self.files
                    .can_clean(row.state, row.is_seeding(), self.activity_online)
            })
    }

    fn confirm_clean_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.clean_files_enabled() {
            return;
        }
        let title = self.t(cx, "btTaskFilesClean");
        let description = self.t(cx, "btTaskFilesCleanConfirm");
        let cancel = self.t(cx, "cancel");
        let generation = self.task_generation;
        let selected = self.files.selected();
        let view = cx.weak_entity();
        window.open_alert_dialog(cx, move |dialog, _, cx| {
            let view = view.clone();
            let selected = selected.clone();
            dialog
                .title(fluxdown_ui_components::dialog_title(title.clone(), cx))
                .description(description.clone())
                .footer(fluxdown_ui_components::dialog_footer(
                    Some(cancel.clone()),
                    title.clone(),
                    fluxdown_ui_components::DialogIntent::Destructive,
                    cx,
                ))
                .on_ok(move |_, _, cx| {
                    view.update(cx, |this, cx| {
                        if this.task_generation != generation
                            || !this.clean_files_enabled()
                            || this.files.selected() != selected
                        {
                            return false;
                        }
                        let mut indices: Vec<_> = selected.iter().copied().collect();
                        indices.sort_unstable();
                        this.change_files(
                            DownloadsCommand::CleanBtUnselected {
                                task_id: this.task_id.clone(),
                                indices,
                            },
                            cx,
                        );
                        true
                    })
                    .unwrap_or(false)
                })
        });
    }

    fn change_files(&mut self, command: DownloadsCommand, cx: &mut Context<Self>) {
        self.files.saving = true;
        // An earlier poll must not overwrite the authoritative read after a file command.
        self.files.revision = self.files.revision.wrapping_add(1);
        self.files.loading = false;
        let generation = self.task_generation;
        let task_id = self.task_id.clone();
        let port = self.port.clone();
        let future = self.port.execute(command);
        cx.spawn(async move |this, cx| {
            let result = match future.await {
                Ok(_) => {
                    port.execute(DownloadsCommand::TaskBtFiles {
                        task_id: task_id.clone(),
                    })
                    .await
                }
                Err(error) => Err(error),
            };
            let Ok(()) = this.update(cx, |this, cx| {
                if this.task_generation != generation {
                    return;
                }
                this.files.saving = false;
                match result {
                    Ok(DownloadsResult::TaskBtFiles(data)) if data.task_id == task_id => {
                        this.files.data = Some(data);
                        this.files.draft = None;
                        this.files.failed = false;
                        this.last_error = None;
                    }
                    _ => this.last_error = Some(this.strings.action_failed.clone()),
                }
                cx.notify();
            }) else {
                return; // The detail view was closed while the command was running.
            };
        })
        .detach();
        cx.notify();
    }

    pub(super) fn render_files(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = active_theme(cx);
        let tokens = theme.tokens().clone();
        let fill = theme.extended().colors.progress_fill;
        let selected = self.files.selected();
        let changed = self.files.changed(&selected);
        let mut content = v_flex().gap(tokens.spacing.sm);
        if self.files.failed {
            content = content.child(
                h_flex()
                    .gap(tokens.spacing.sm)
                    .items_center()
                    .child(
                        div()
                            .text_color(tokens.colors.destructive)
                            .child(self.t(cx, "btTaskFilesReadError")),
                    )
                    .child(
                        Button::new("bt-files-retry")
                            .ghost()
                            .control(cx)
                            .label(self.t(cx, "detailActivityRetry"))
                            .disabled(self.files.loading)
                            .on_click(cx.listener(|this, _, _, cx| this.fetch_files(cx))),
                    ),
            );
        }
        let Some(data) = self.files.data.as_ref().filter(|data| data.metadata_ready) else {
            return content
                .child(
                    div()
                        .text_color(tokens.colors.muted_foreground)
                        .child(self.t(cx, "btTaskFilesMetadataPending")),
                )
                .into_any_element();
        };
        content = content
            .child(
                h_flex()
                    .gap(tokens.spacing.sm)
                    .child(
                        Button::new("bt-files-select-all")
                            .ghost()
                            .control(cx)
                            .label(self.t(cx, "btFileSelectAll"))
                            .disabled(
                                !self.activity_online
                                    || self.files.saving
                                    || self.files.failed
                                    || data.files.is_empty(),
                            )
                            .on_click(
                                cx.listener(|this, _, _, cx| this.select_all_files(true, cx)),
                            ),
                    )
                    .child(
                        Button::new("bt-files-deselect-all")
                            .ghost()
                            .control(cx)
                            .label(self.t(cx, "deselectAll"))
                            .disabled(
                                !self.activity_online
                                    || self.files.saving
                                    || self.files.failed
                                    || data.files.is_empty(),
                            )
                            .on_click(
                                cx.listener(|this, _, _, cx| this.select_all_files(false, cx)),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .flex_wrap()
                    .gap(tokens.spacing.sm)
                    .child(
                        Button::new("bt-files-apply")
                            .primary()
                            .control(cx)
                            .label(self.t(cx, "btTaskFilesApply"))
                            .loading(self.files.saving)
                            .disabled(
                                !self.activity_online
                                    || self.files.saving
                                    || !changed
                                    || selected.is_empty(),
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.save_files(cx))),
                    )
                    .child(
                        Button::new("bt-files-cancel")
                            .ghost()
                            .control(cx)
                            .label(self.t(cx, "cancel"))
                            .disabled(self.files.saving || !changed)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.files.draft = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("bt-files-clean")
                            .outline()
                            .control(cx)
                            .label(self.t(cx, "btTaskFilesClean"))
                            .disabled(!self.clean_files_enabled())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.confirm_clean_files(window, cx)
                            })),
                    ),
            );
        if selected.is_empty() {
            content = content.child(
                div()
                    .text_color(tokens.colors.muted_foreground)
                    .child(self.t(cx, "btTaskFilesRequireSelection")),
            );
        }
        for file in &data.files {
            let index = file.index;
            let size = u64::try_from(file.size).unwrap_or(0);
            let progress = file.downloaded_bytes.map_or_else(
                || self.t(cx, "btTaskFilesProgressUnknown").to_string(),
                format_bytes,
            );
            let state = self.t(
                cx,
                if file.selected {
                    "btTaskFilesSelected"
                } else {
                    "btTaskFilesStopped"
                },
            );
            let mut label = v_flex()
                .flex_1()
                .min_w_0()
                .gap(tokens.spacing.xs)
                .child(div().child(SharedString::from(file.path.clone())))
                .child(
                    div()
                        .text_size(tokens.typography.xs.size)
                        .font_features(tabular_numbers())
                        .text_color(tokens.colors.muted_foreground)
                        .child(SharedString::from(format!(
                            "{progress} / {} · {state}",
                            format_bytes(size)
                        ))),
                );
            if let Some(done) = file.downloaded_bytes {
                let percent = if size == 0 {
                    100.
                } else {
                    (done.min(size) as f64 / size as f64 * 100.) as f32
                };
                label = label.child(
                    Progress::new((
                        "bt-file-progress",
                        usize::try_from(index).unwrap_or_default(),
                    ))
                    .w_full()
                    .color(fill)
                    .value(percent)
                    .accessibility_label(SharedString::from(file.path.clone())),
                );
            }
            let this = cx.weak_entity();
            content = content.child(check_row(
                ("bt-task-file", usize::try_from(index).unwrap_or_default()),
                selected.contains(&index),
                label,
                move |checked, _, cx| {
                    let Ok(()) = this.update(cx, |this, cx| {
                        if this.files.saving {
                            return;
                        }
                        let mut selected = this.files.selected();
                        if checked {
                            selected.insert(index);
                        } else {
                            selected.remove(&index);
                        }
                        this.files.draft = Some(selected);
                        cx.notify();
                    }) else {
                        return; // The detail view was closed before this click was handled.
                    };
                },
                cx,
            ));
        }
        content.into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use fluxdown_protocol::{BtTaskFileDto, BtTaskFilesDto};

    use super::TaskFilesState;
    use crate::model::TaskState;

    fn state() -> TaskFilesState {
        TaskFilesState {
            data: Some(BtTaskFilesDto {
                task_id: "bt".into(),
                metadata_ready: true,
                files: vec![
                    BtTaskFileDto {
                        index: 0,
                        path: "selected".into(),
                        size: 10,
                        downloaded_bytes: Some(5),
                        selected: true,
                    },
                    BtTaskFileDto {
                        index: 2,
                        path: "unselected".into(),
                        size: 10,
                        downloaded_bytes: None,
                        selected: false,
                    },
                ],
            }),
            ..TaskFilesState::default()
        }
    }

    #[test]
    fn cleanup_requires_stopped_online_task_and_applied_selection() {
        let mut files = state();
        for task in [TaskState::Paused, TaskState::Failed, TaskState::Completed] {
            assert!(files.can_clean(task, false, true));
            assert!(!files.can_clean(task, false, false));
            assert!(!files.can_clean(task, true, true));
        }
        for task in [TaskState::Pending, TaskState::Downloading] {
            assert!(!files.can_clean(task, false, true));
        }
        files.draft = Some([0, 2].into_iter().collect());
        assert!(!files.can_clean(TaskState::Paused, false, true));
        files.draft = Some([0].into_iter().collect());
        assert!(files.can_clean(TaskState::Paused, false, true));
        files.saving = true;
        assert!(!files.can_clean(TaskState::Paused, false, true));
    }

    #[test]
    fn cleanup_requires_ready_metadata_with_unselected_files() {
        let mut files = state();
        files.data.as_mut().expect("fixture").metadata_ready = false;
        assert!(!files.can_clean(TaskState::Paused, false, true));
        let data = files.data.as_mut().expect("fixture");
        data.metadata_ready = true;
        data.files.get_mut(1).expect("unselected fixture").selected = true;
        assert!(!files.can_clean(TaskState::Paused, false, true));
        files.data = None;
        assert!(!files.can_clean(TaskState::Paused, false, true));
    }
    #[test]
    fn cleanup_is_disabled_when_applied_selection_changes_under_a_draft() {
        let mut files = state();
        files.draft = Some([0].into_iter().collect());
        assert!(files.can_clean(TaskState::Paused, false, true));
        let data = files.data.as_mut().expect("fixture");
        data.files.first_mut().expect("selected fixture").selected = false;
        data.files.get_mut(1).expect("unselected fixture").selected = true;
        assert!(!files.can_clean(TaskState::Paused, false, true));
    }
}
