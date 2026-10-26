//! Existing-task BT file inspection and selection. Download generations own completion plans.
use std::collections::HashSet;
use std::path::PathBuf;

use super::{DownloadManager, is_bt_url, is_safe_file_name};
use crate::bt_downloader::{TorrentSource, bt_stage_dir, probe_torrent_meta};

pub(crate) mod cleanup;

/// One non-padding torrent file, indexed in the original metadata.
#[derive(Debug, Clone)]
pub struct BtTaskFile {
    pub index: i32,
    pub path: String,
    pub size: i64,
    pub downloaded_bytes: Option<u64>,
    pub selected: bool,
}

/// File details; unresolved metadata has `metadata_ready == false` and no files.
#[derive(Debug, Clone)]
pub struct BtTaskFiles {
    pub task_id: String,
    pub metadata_ready: bool,
    pub files: Vec<BtTaskFile>,
}

/// File commands reject invalid input before changing the selection or user data.
#[derive(Debug, thiserror::Error)]
pub enum BtFilesError {
    #[error("task not found")]
    NotFound,
    #[error("invalid BT file selection: {0}")]
    Invalid(String),
    #[error("BT task is busy: {0}")]
    Busy(String),
    #[error("BT files operation failed: {0}")]
    Operation(String),
}

fn operation(error: impl std::fmt::Display) -> BtFilesError {
    BtFilesError::Operation(error.to_string())
}

fn validate_selection(files: &[BtTaskFile], indices: &[i32]) -> Result<Vec<i32>, BtFilesError> {
    if indices.is_empty() {
        return Ok(files.iter().map(|file| file.index).collect());
    }
    let valid: HashSet<i32> = files.iter().map(|f| f.index).collect();
    if indices.iter().any(|i| !valid.contains(i)) {
        return Err(BtFilesError::Invalid(
            "unknown or padding file index".into(),
        ));
    }
    let mut selected = indices.to_vec();
    selected.sort_unstable();
    selected.dedup();
    Ok(selected)
}

/// An append's disk-copy work, held outside the daemon actor while it runs.
pub struct BtFilePreparation {
    sidecar: PathBuf,
    save: PathBuf,
    stage: PathBuf,
    files: Vec<BtTaskFile>,
    info_hash: String,
    cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
    closing: tokio_util::sync::CancellationToken,
    _inflight: crate::bt_downloader::InflightGuard,
    _guard: tokio::sync::OwnedMutexGuard<()>,
}

impl BtFilePreparation {
    /// Copy retained bytes to staging. Call from a blocking worker, never the actor.
    pub fn restore(self) -> Result<(), BtFilesError> {
        crate::bt_partfile::restore_download_stage(
            &self.sidecar,
            &self.save,
            &self.stage,
            &self.files,
            &self.info_hash,
            &self.cancellation,
            &self.closing,
        )
        .map_err(operation)
    }
}

impl DownloadManager {
    /// Prepare append-only disk work without blocking commands for other tasks.
    pub async fn prepare_bt_task_file_change(
        &mut self,
        task_id: &str,
        indices: &[i32],
    ) -> Result<Option<BtFilePreparation>, BtFilesError> {
        let task = self
            .db
            .load_task_by_id(task_id)
            .await
            .map_err(operation)?
            .ok_or(BtFilesError::NotFound)?;
        if task.status != 3 {
            return Ok(None);
        }
        let details = self.bt_task_files(task_id).await?;
        let indices = validate_selection(&details.files, indices)?;
        let previous: Vec<_> = details
            .files
            .iter()
            .filter(|f| f.selected)
            .map(|f| f.index)
            .collect();
        if previous == indices {
            return Ok(None);
        }
        if self.active_tasks.contains_key(task_id) || self.pending_pauses.contains_key(task_id) {
            return Err(BtFilesError::Busy("completion is still finalizing".into()));
        }
        self.ensure_bt_session().await.map_err(operation)?;
        let source = match self
            .db
            .load_torrent_file_bytes(task_id)
            .await
            .map_err(operation)?
        {
            Some(bytes) => TorrentSource::TorrentFileBytes(bytes),
            None => TorrentSource::Magnet(task.url.clone()),
        };
        let info_hash = source
            .info_hash_hex()
            .ok_or_else(|| BtFilesError::Invalid("missing info hash".into()))?;
        let bt = self
            .bt_session
            .clone()
            .ok_or_else(|| BtFilesError::Busy("BT session unavailable".into()))?;
        let guard = bt
            .file_change_lock(task_id)
            .await
            .try_lock_owned()
            .map_err(|_| BtFilesError::Busy("file command is in progress".into()))?;
        self.db
            .set_config(&format!("bt_preserve_stage_{task_id}"), "true")
            .await
            .map_err(operation)?;
        let cancellation = bt.file_change_cancellation(task_id).await;
        cancellation.store(false, std::sync::atomic::Ordering::Release);
        Ok(Some(BtFilePreparation {
            sidecar: crate::bt_partfile::sidecar_path(&self.app_data_dir, task_id),
            stage: bt_stage_dir(&task.save_dir, task_id),
            save: task.save_dir.into(),
            files: details.files,
            info_hash,
            cancellation,
            closing: bt.closing_token(),
            _inflight: bt.inflight_guard(),
            _guard: guard,
        }))
    }

    /// Read metadata and verified per-file bytes without starting a torrent.
    pub async fn bt_task_files(&self, task_id: &str) -> Result<BtTaskFiles, BtFilesError> {
        self.bt_task_files_future(task_id.to_owned()).await
    }

    /// Capture a file query for execution outside the command actor.
    pub fn bt_task_files_future(
        &self,
        task_id: String,
    ) -> futures_util::future::BoxFuture<'static, Result<BtTaskFiles, BtFilesError>> {
        Box::pin(read_task_files(
            self.db.clone(),
            self.bt_session.clone(),
            self.app_data_dir.clone(),
            task_id,
        ))
    }

    /// Change selected files, preserving bytes and the task's paused/running intent.
    /// An empty RPC list retains the original all-files meaning; UI requires a choice.
    pub async fn set_bt_task_files(
        &mut self,
        task_id: &str,
        indices: &[i32],
    ) -> Result<(), BtFilesError> {
        let preparation = self.prepare_bt_task_file_change(task_id, indices).await?;
        if let Some(preparation) = preparation {
            tokio::task::spawn_blocking(move || preparation.restore())
                .await
                .map_err(operation)??;
        }
        self.commit_bt_task_files(task_id, indices).await
    }

    /// Commit a file change after [`Self::prepare_bt_task_file_change`] has completed.
    pub async fn commit_bt_task_files(
        &mut self,
        task_id: &str,
        indices: &[i32],
    ) -> Result<(), BtFilesError> {
        let details = self.bt_task_files(task_id).await?;
        if !details.metadata_ready {
            return Err(BtFilesError::Busy("metadata is not ready".into()));
        }
        let indices = validate_selection(&details.files, indices)?;
        let bt = self.bt_session.clone();
        let lock = if let Some(bt) = &bt {
            Some(bt.file_change_lock(task_id).await)
        } else {
            None
        };
        let _guard = match &lock {
            Some(lock) => Some(lock.try_lock().map_err(|_| {
                BtFilesError::Busy("completion or another file command is in progress".into())
            })?),
            None => None,
        };
        let task = self
            .db
            .load_task_by_id(task_id)
            .await
            .map_err(operation)?
            .ok_or(BtFilesError::NotFound)?;
        if task.status == 3
            && (self.active_tasks.contains_key(task_id)
                || self.pending_pauses.contains_key(task_id))
        {
            return Err(BtFilesError::Busy("completion is still finalizing".into()));
        }
        let handle = if let Some(bt) = &bt {
            bt.cached_handle(task_id).await
        } else {
            None
        };
        if handle.is_none()
            && (self.active_tasks.contains_key(task_id)
                || self.pending_pauses.contains_key(task_id)
                || bt.as_ref().is_some_and(|bt| bt.has_inflight_adds()))
        {
            return Err(BtFilesError::Busy(
                "torrent is still starting or stopping".into(),
            ));
        }
        if handle.as_ref().is_some_and(|h| {
            matches!(
                h.stats().state,
                librqbit::TorrentStatsState::Initializing { .. }
            )
        }) {
            return Err(BtFilesError::Busy(
                "initial file check is in progress".into(),
            ));
        }
        if handle.is_some()
            && self
                .db
                .load_bt_selected_files(task_id)
                .await
                .map_err(operation)?
                .is_none()
        {
            return Err(BtFilesError::Busy(
                "initial file selection is still pending".into(),
            ));
        }
        let previous: Vec<i32> = details
            .files
            .iter()
            .filter(|f| f.selected)
            .map(|f| f.index)
            .collect();
        if previous == indices {
            return Ok(());
        }
        let was_running = matches!(task.status, 0 | 1 | 5);
        if task.status == 3 {
            if self
                .db
                .load_torrent_file_bytes(task_id)
                .await
                .map_err(operation)?
                .is_none()
            {
                let hash = TorrentSource::Magnet(task.url.clone())
                    .info_hash_hex()
                    .ok_or_else(|| BtFilesError::Invalid("missing info hash".into()))?;
                let bytes = tokio::fs::read(
                    crate::bt_partfile::persistence_dir(&self.app_data_dir)
                        .join(format!("{hash}.torrent")),
                )
                .await
                .map_err(operation)?;
                self.db
                    .save_torrent_file_bytes(task_id, &bytes)
                    .await
                    .map_err(operation)?;
            }
            if let Some(bt) = &bt {
                bt.detach_for_file_change(task_id)
                    .await
                    .map_err(operation)?;
            }
            // A prior completed container is still owned by this task. Reuse it
            // when adding files instead of deduplicating into a new directory,
            // which would strand previously completed nested files.
            if is_safe_file_name(&task.file_name)
                && tokio::fs::metadata(PathBuf::from(&task.save_dir).join(&task.file_name))
                    .await
                    .is_ok_and(|metadata| metadata.is_dir())
            {
                self.db
                    .set_config(&format!("bt_completion_top_{task_id}"), &task.file_name)
                    .await
                    .map_err(operation)?;
            }
            self.db
                .update_task_status(task_id, 2, "")
                .await
                .map_err(operation)?;
        } else {
            self.pause_task_keep_slot(task_id).await;
        }
        if previous.iter().any(|index| !indices.contains(index)) {
            self.db
                .set_config(&format!("bt_preserve_stage_{task_id}"), "true")
                .await
                .map_err(operation)?;
        }
        self.db
            .save_bt_selected_files(task_id, &indices, indices.len() == details.files.len())
            .await
            .map_err(operation)?;
        if task.status != 3
            && let (Some(bt), Some(handle)) = (&bt, &handle)
        {
            let only = indices
                .iter()
                .filter_map(|i| usize::try_from(*i).ok())
                .collect();
            // Re-add newly wanted files: a selective check may not have checked their retained bytes.
            let adds_files = indices.iter().any(|index| !previous.contains(index));
            let result = if adds_files || bt.is_fastresume_tainted(task_id).await {
                bt.detach_for_file_change(task_id).await.map_err(operation)
            } else {
                bt.session()
                    .update_only_files(handle, &only)
                    .await
                    .map_err(operation)
            };
            if let Err(error) = result {
                self.db
                    .save_bt_selected_files(
                        task_id,
                        &previous,
                        previous.len() == details.files.len(),
                    )
                    .await
                    .map_err(operation)?;
                return Err(operation(format!("{error:#}")));
            }
        }
        if !indices.is_empty() && (was_running || task.status == 3) {
            self.resume_task(task_id).await;
        } else {
            self.drain_queue().await;
        }
        self.send_tasks_snapshot().await;
        Ok(())
    }
}

async fn read_task_files(
    db: crate::db::Db,
    bt_session: Option<std::sync::Arc<crate::bt_downloader::SharedBtSession>>,
    app_data_dir: String,
    task_id: String,
) -> Result<BtTaskFiles, BtFilesError> {
    let task_id = task_id.as_str();
    let task = db
        .load_task_by_id(task_id)
        .await
        .map_err(operation)?
        .ok_or(BtFilesError::NotFound)?;
    if !is_bt_url(&task.url) {
        return Err(BtFilesError::Invalid("task is not BitTorrent".into()));
    }
    let handle = if let Some(bt) = &bt_session {
        bt.cached_handle(task_id).await
    } else {
        None
    };
    let selection = db
        .load_bt_selected_files(task_id)
        .await
        .map_err(operation)?;
    let selected = |index: i32| {
        selection
            .as_ref()
            .is_none_or(|v| v.is_empty() || v.contains(&index))
    };
    let mut result = BtTaskFiles {
        task_id: task_id.into(),
        metadata_ready: false,
        files: Vec::new(),
    };
    if let Some(handle) = handle {
        let progress = handle.stats().file_progress;
        if let Ok(files) = handle.with_metadata(|meta| {
            meta.file_infos
                .iter()
                .enumerate()
                .filter(|(_, f)| !f.attrs.padding)
                .filter_map(|(i, f)| {
                    let index = i32::try_from(i).ok()?;
                    Some(BtTaskFile {
                        index,
                        path: f.relative_filename.to_string_lossy().into_owned(),
                        size: i64::try_from(f.len).unwrap_or(i64::MAX),
                        downloaded_bytes: progress.get(i).map(|n| (*n).min(f.len)),
                        selected: selected(index),
                    })
                })
                .collect()
        }) {
            result.metadata_ready = true;
            result.files = files;
            return Ok(result);
        }
    }
    let bytes = crate::bt_downloader::cached_torrent_bytes(&db, task_id, &task.url, &app_data_dir)
        .await
        .map_err(operation)?;
    if let Some(bytes) = bytes {
        let probe_id = task_id.to_owned();
        let meta = tokio::task::spawn_blocking(move || probe_torrent_meta(probe_id, bytes))
            .await
            .map_err(operation)?;
        if !meta.error.is_empty() {
            return Err(operation(meta.error));
        }
        result.metadata_ready = true;
        result.files = meta
            .files
            .into_iter()
            .map(|f| BtTaskFile {
                index: f.index,
                selected: selected(f.index),
                path: f.path,
                size: f.size,
                downloaded_bytes: None,
            })
            .collect();
    }
    if result.metadata_ready {
        let sidecar = crate::bt_partfile::sidecar_path(&app_data_dir, task_id);
        let save = PathBuf::from(&task.save_dir);
        if let Some(layout) = tokio::task::spawn_blocking(move || {
            crate::bt_partfile::load_sidecar_layout(&sidecar, &save)
        })
        .await
        .map_err(operation)?
        .map_err(operation)?
        {
            for file in &mut result.files {
                if let Ok(index) = usize::try_from(file.index)
                    && let Some(path) = layout.selected_path(index)
                    && let Ok(metadata) = tokio::fs::metadata(path).await
                    && metadata.len() == u64::try_from(file.size).map_err(operation)?
                {
                    file.downloaded_bytes = Some(metadata.len());
                }
            }
        }
    }
    Ok(result)
}

/// A cached add may publish after a pause; never reuse a handle for a different persisted range.
pub(super) fn handle_selection_matches(
    handle: &crate::bt_downloader::BtHandle,
    wanted: &[i32],
) -> bool {
    let only = handle.only_files();
    handle
        .with_metadata(|meta| {
            let all: HashSet<usize> = meta
                .file_infos
                .iter()
                .enumerate()
                .filter(|(_, file)| !file.attrs.padding)
                .map(|(index, _)| index)
                .collect();
            let actual: HashSet<_> = only
                .as_ref()
                .map(|indices| {
                    indices
                        .iter()
                        .copied()
                        .filter(|index| all.contains(index))
                        .collect()
                })
                .unwrap_or_else(|| all.clone());
            let expected: HashSet<_> = if wanted.is_empty() {
                all
            } else {
                wanted
                    .iter()
                    .filter_map(|index| usize::try_from(*index).ok())
                    .collect()
            };
            actual == expected
        })
        .unwrap_or(false)
}
