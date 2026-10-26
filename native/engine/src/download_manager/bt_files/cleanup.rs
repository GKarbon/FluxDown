//! Explicit stopped-task cleanup, including unselected completed outputs.
use std::collections::HashSet;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::{BtFilesError, TorrentSource, bt_stage_dir, is_bt_url, is_safe_file_name, operation};

struct Candidate {
    path: PathBuf,
    keep: Vec<(u64, u64)>,
    len: u64,
}

/// Validate the whole plan before changing any file; retries preserve required boundary bytes.
pub(super) fn clean_stage(
    stage: &Path,
    torrent: &[u8],
    selected: &HashSet<usize>,
    protected: &[PathBuf],
) -> Result<(), BtFilesError> {
    if !stage.exists() {
        return Ok(());
    }
    // Resolve the user-configured parent once (macOS /var is a system alias),
    // then reject links inside the task-owned stage.
    let parent = stage
        .parent()
        .ok_or_else(|| operation("missing staging parent"))?;
    let stage = std::fs::canonicalize(parent).map_err(operation)?.join(
        stage
            .file_name()
            .ok_or_else(|| operation("missing staging name"))?,
    );
    let stage = stage.as_path();
    safe_path(stage)?;
    let parsed = librqbit::torrent_from_bytes(torrent).map_err(operation)?;
    let info = parsed.info.data.validate().map_err(operation)?;
    let files: Vec<_> = info.iter_file_details_ext().collect();
    let needed: Vec<_> = files
        .iter()
        .enumerate()
        .filter(|(id, _)| selected.contains(id))
        .map(|(_, file)| file.pieces.clone())
        .collect();
    let piece_length = u64::from(info.lengths().default_piece_length());
    let mut protected = protected.to_vec();
    for (id, file) in files.iter().enumerate() {
        if selected.contains(&id) {
            protected.push(canonical_claim(
                &stage.join(file.details.filename.to_string()),
            )?);
        }
    }
    let mut candidates = Vec::new();
    for (id, file) in files.iter().enumerate() {
        if selected.contains(&id) || file.details.attrs().padding {
            continue;
        }
        let relative = PathBuf::from(file.details.filename.to_string());
        if !crate::bt_downloader::torrent_relative_path_is_safe(&relative, cfg!(windows)) {
            return Err(BtFilesError::Invalid("unsafe torrent path".into()));
        }
        let path = stage.join(relative);
        let metadata = match std::fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => return Err(operation(error)),
        };
        validate_cleanup_target(&path, &protected).map_err(operation)?;
        let mut keep = Vec::new();
        if !file.pieces.is_empty() {
            let first = file.pieces.start;
            let last = file.pieces.end - 1;
            for piece in [first, last] {
                if piece == last && first == last && !keep.is_empty() {
                    continue;
                }
                if !needed.iter().any(|range| range.contains(&piece)) {
                    continue;
                }
                let start = (u64::from(piece) * piece_length).max(file.offset) - file.offset;
                let end = ((u64::from(piece) + 1) * piece_length)
                    .min(file.offset + file.details.len)
                    - file.offset;
                let end = end.min(metadata.len());
                if start < end {
                    keep.push((start, end - start));
                }
            }
        }
        candidates.push(Candidate {
            path,
            keep,
            len: file.details.len,
        });
    }
    for candidate in candidates {
        safe_path(&candidate.path)?;
        if candidate.keep.is_empty() {
            std::fs::remove_file(&candidate.path).map_err(operation)?;
            continue;
        }
        // Sparse replacement retains only bytes needed by the selected files' shared pieces.
        let temporary = candidate
            .path
            .with_file_name(format!(".fluxdown-clean-{}", uuid::Uuid::new_v4()));
        let mut created = false;
        let result = (|| -> std::io::Result<()> {
            let mut input = std::fs::File::open(&candidate.path)?;
            let mut output = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)?;
            created = true;
            #[cfg(windows)]
            crate::bt_sparse::mark_sparse(&temporary)?;
            for (offset, len) in candidate.keep {
                input.seek(SeekFrom::Start(offset))?;
                output.seek(SeekFrom::Start(offset))?;
                let copied = std::io::copy(&mut Read::by_ref(&mut input).take(len), &mut output)?;
                if copied != len {
                    return Err(std::io::ErrorKind::UnexpectedEof.into());
                }
            }
            output.set_len(candidate.len)?;
            output.flush()?;
            output.sync_all()?;
            drop(output);
            drop(input);
            std::fs::rename(&temporary, &candidate.path)
        })();
        if let Err(error) = result {
            if !created {
                return Err(operation(error));
            }
            match std::fs::remove_file(&temporary) {
                Ok(()) => {}
                Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => {}
                Err(cleanup) => {
                    return Err(operation(format!("{error}; temporary cleanup: {cleanup}")));
                }
            }
            return Err(operation(error));
        }
    }
    Ok(())
}

/// Only recorded, ordinary, unshared paths may be explicitly removed; missing paths are retryable.
pub(crate) fn validate_cleanup_target(path: &Path, protected: &[PathBuf]) -> Result<bool, String> {
    (|| -> Result<bool, BtFilesError> {
        let metadata = match std::fs::symlink_metadata(path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(operation(error)),
        };
        safe_path(path)?;
        if !metadata.is_file() {
            return Err(operation("cleanup target is not a regular file"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            if metadata.nlink() != 1 {
                return Err(operation("cleanup target has other hard links"));
            }
        }
        let canonical = std::fs::canonicalize(path).map_err(operation)?;
        if protected
            .iter()
            .any(|other| canonical.starts_with(other) || other.starts_with(&canonical))
        {
            return Err(BtFilesError::Busy(
                "cleanup target is claimed by another task or selected output".into(),
            ));
        }
        Ok(true)
    })()
    .map_err(|error| error.to_string())
}

pub(crate) fn safe_path(path: &Path) -> Result<(), BtFilesError> {
    for ancestor in path.ancestors() {
        let metadata = std::fs::symlink_metadata(ancestor).map_err(operation)?;
        if metadata.file_type().is_symlink() {
            return Err(operation("cleanup refuses symbolic links"));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(operation("cleanup refuses reparse points"));
            }
        }
    }
    Ok(())
}

pub(crate) fn canonical_claim(path: &Path) -> Result<PathBuf, BtFilesError> {
    let mut existing = path;
    let mut missing = Vec::new();
    loop {
        match std::fs::canonicalize(existing) {
            Ok(mut resolved) => {
                for component in missing.into_iter().rev() {
                    resolved.push(component);
                }
                return Ok(resolved);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(
                    existing
                        .file_name()
                        .ok_or_else(|| operation("invalid ownership path"))?,
                );
                existing = existing
                    .parent()
                    .ok_or_else(|| operation("invalid ownership root"))?;
            }
            Err(error) => return Err(operation(error)),
        }
    }
}

impl super::DownloadManager {
    /// Explicitly remove unselected downloaded data, including completed outputs of a stopped task.
    /// Selected files and their required shared-piece boundary bytes are retained.
    pub async fn clean_bt_unselected(
        &mut self,
        task_id: &str,
        expected: &[i32],
    ) -> Result<(), BtFilesError> {
        let task = self
            .db
            .load_task_by_id(task_id)
            .await
            .map_err(operation)?
            .ok_or(BtFilesError::NotFound)?;
        if !matches!(task.status, 2..=4)
            || matches!(task.seeding_status, 1 | 8)
            || self.active_tasks.contains_key(task_id)
            || self.pending_pauses.contains_key(task_id)
        {
            return Err(BtFilesError::Busy(
                "stop the task and wait for all writes to finish".into(),
            ));
        }
        let details = self.bt_task_files(task_id).await?;
        if !details.metadata_ready {
            return Err(BtFilesError::Busy("metadata is not ready".into()));
        }
        let actual: Vec<_> = details
            .files
            .iter()
            .filter(|file| file.selected)
            .map(|file| file.index)
            .collect();
        if super::validate_selection(&details.files, expected)? != actual {
            return Err(BtFilesError::Busy(
                "file selection changed; review the cleanup range again".into(),
            ));
        }
        let bt = self.bt_session.clone();
        if let Some(bt) = &bt
            && bt.cached_handle(task_id).await.is_none()
            && bt.has_inflight_adds()
        {
            return Err(BtFilesError::Busy(
                "torrent storage is still stopping".into(),
            ));
        }
        let _inflight = bt.as_ref().map(|bt| bt.inflight_guard());
        let _guard = if let Some(bt) = &bt {
            let guard = bt
                .file_change_lock(task_id)
                .await
                .try_lock_owned()
                .map_err(|_| BtFilesError::Busy("another file command is in progress".into()))?;
            if let Some(handle) = bt.cached_handle(task_id).await
                && !handle.is_paused()
                && !(task.status == 3 && handle.stats().finished)
                && !matches!(handle.stats().state, librqbit::TorrentStatsState::Error)
            {
                return Err(BtFilesError::Busy("torrent is not fully stopped".into()));
            }
            bt.detach_for_file_change(task_id)
                .await
                .map_err(operation)?;
            Some(guard)
        } else {
            None
        };
        let source = self
            .db
            .load_torrent_file_bytes(task_id)
            .await
            .map_err(operation)?;
        let bytes = match source {
            Some(bytes) => bytes,
            None => {
                let hash = TorrentSource::Magnet(task.url.clone())
                    .info_hash_hex()
                    .ok_or_else(|| BtFilesError::Invalid("missing info hash".into()))?;
                tokio::fs::read(
                    crate::bt_partfile::persistence_dir(&self.app_data_dir)
                        .join(format!("{hash}.torrent")),
                )
                .await
                .map_err(operation)?
            }
        };
        // No cached handle after restart: its persisted bitmap must also be invalidated.
        let hash = TorrentSource::TorrentFileBytes(bytes.clone())
            .info_hash_hex()
            .ok_or_else(|| BtFilesError::Invalid("missing info hash".into()))?;
        match tokio::fs::remove_file(
            crate::bt_partfile::persistence_dir(&self.app_data_dir).join(format!("{hash}.bitv")),
        )
        .await
        {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(operation(error)),
        }
        let selected: HashSet<usize> = details
            .files
            .iter()
            .filter(|file| file.selected)
            .filter_map(|file| usize::try_from(file.index).ok())
            .collect();
        let stage = bt_stage_dir(&task.save_dir, task_id);
        let tasks = self.db.load_all_tasks().await.map_err(operation)?;
        let ids: Vec<_> = tasks.iter().map(|task| task.task_id.clone()).collect();
        let facts = self
            .db
            .load_deletion_facts(&ids, &[])
            .await
            .map_err(operation)?;
        let app_data = self.app_data_dir.clone();
        let current_id = task_id.to_owned();
        tokio::task::spawn_blocking(move || {
            let mut protected = Vec::new();
            for other in tasks {
                let save = PathBuf::from(&other.save_dir);
                if other.task_id != current_id
                    && let Some(names) = facts.artifacts.get(&other.task_id)
                {
                    protected.extend(
                        names
                            .iter()
                            .filter(|name| is_safe_file_name(name))
                            .map(|name| save.join(name)),
                    );
                }
                if other.task_id != current_id {
                    if is_safe_file_name(&other.file_name) {
                        protected.push(save.join(&other.file_name));
                    }
                    if is_bt_url(&other.url) {
                        protected.push(bt_stage_dir(&other.save_dir, &other.task_id));
                    }
                }
                if is_bt_url(&other.url)
                    && let Some(layout) = crate::bt_partfile::load_sidecar_layout(
                        &crate::bt_partfile::sidecar_path(&app_data, &other.task_id),
                        &save,
                    )
                    .map_err(operation)?
                {
                    if other.task_id == current_id {
                        protected.extend(
                            selected
                                .iter()
                                .filter_map(|id| layout.selected_path(*id))
                                .map(PathBuf::from),
                        );
                    } else {
                        protected.extend(layout.selected_paths().map(PathBuf::from));
                    }
                }
            }
            let protected: Vec<_> = protected
                .iter()
                .map(|path| canonical_claim(path))
                .collect::<Result<_, _>>()?;
            crate::bt_partfile::clean_unselected_outputs(
                &crate::bt_partfile::sidecar_path(&app_data, &current_id),
                &PathBuf::from(&task.save_dir),
                &bytes,
                &selected,
                &protected,
            )
            .map_err(operation)?;
            let mut protected = protected;
            if let Some(layout) = crate::bt_partfile::load_sidecar_layout(
                &crate::bt_partfile::sidecar_path(&app_data, &current_id),
                &PathBuf::from(&task.save_dir),
            )
            .map_err(operation)?
            {
                for path in layout.selected_paths() {
                    protected.push(canonical_claim(path)?);
                }
            }
            clean_stage(&stage, &bytes, &selected, &protected)
        })
        .await
        .map_err(operation)??;
        self.send_tasks_snapshot().await;
        Ok(())
    }
}
