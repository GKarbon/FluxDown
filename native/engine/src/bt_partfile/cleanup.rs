//! Crash-retryable removal of explicitly deselected completed outputs.
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::{
    MAX_HEADER_LEN, PARTS_MAGIC, PARTS_VERSION, PartsFileMeta, PartsHeader, RestoreTemporary,
    SegmentEntry, SelectedEntry, SidecarWriteRequest, boundary_segments, load_seed_layout,
    rel_path_string, resolve_rel_path,
};
use crate::download_manager::bt_files::cleanup::{canonical_claim, validate_cleanup_target};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PendingCleanup {
    pub(super) file_id: usize,
    pub(super) rel_path: String,
    len: u64,
    modified_secs: u64,
    modified_nanos: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    identity: Option<(u64, u64)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content: Option<[u8; 32]>,
}

impl PendingCleanup {
    fn capture(entry: &SelectedEntry, path: &Path) -> Result<Self, String> {
        let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
        let modified = metadata
            .modified()
            .map_err(|e| e.to_string())?
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?;
        if metadata.len() != entry.len {
            return Err("completed output length changed; refusing cleanup".into());
        }
        #[cfg(unix)]
        let identity = {
            use std::os::unix::fs::MetadataExt;
            Some((metadata.dev(), metadata.ino()))
        };
        #[cfg(not(unix))]
        let identity = None;
        #[cfg(unix)]
        let content = None;
        #[cfg(not(unix))]
        let content = {
            // Stable std has no Windows file ID; hashing avoids deleting a replacement with forged timestamps.
            use sha2::{Digest, Sha256};
            let mut input = File::open(path).map_err(|error| error.to_string())?;
            let mut hasher = Sha256::new();
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let length = input.read(&mut buffer).map_err(|error| error.to_string())?;
                if length == 0 {
                    break;
                }
                hasher.update(&buffer[..length]);
            }
            Some(hasher.finalize().into())
        };
        Ok(Self {
            file_id: entry.file_id,
            rel_path: entry.rel_path.clone(),
            len: entry.len,
            modified_secs: modified.as_secs(),
            modified_nanos: modified.subsec_nanos(),
            identity,
            content,
        })
    }

    pub(super) fn matches(&self, path: &Path) -> bool {
        let entry = SelectedEntry {
            file_id: self.file_id,
            rel_path: self.rel_path.clone(),
            len: self.len,
        };
        Self::capture(&entry, path).is_ok_and(|other| {
            other.modified_secs == self.modified_secs
                && other.modified_nanos == self.modified_nanos
                && other.identity == self.identity
                && other.content == self.content
        })
    }
}

pub(super) fn pending_path_owned(entry: &PendingCleanup, path: &Path) -> bool {
    validate_cleanup_target(path, &[]).is_ok_and(|exists| exists) && entry.matches(path)
}

fn read_header(path: &Path) -> Result<Option<(PartsHeader, u64)>, String> {
    let mut input = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let mut prefix = [0u8; 12];
    input.read_exact(&mut prefix).map_err(|e| e.to_string())?;
    if prefix[..8] != PARTS_MAGIC[..] {
        return Err("bad parts magic".into());
    }
    let len = u32::from_le_bytes(prefix[8..].try_into().map_err(|_| "bad parts header")?);
    if len == 0 || len > MAX_HEADER_LEN {
        return Err("bad parts header length".into());
    }
    let mut json = vec![0; len as usize];
    input.read_exact(&mut json).map_err(|e| e.to_string())?;
    let header: PartsHeader = serde_json::from_slice(&json).map_err(|e| e.to_string())?;
    if header.version != PARTS_VERSION {
        return Err("unsupported parts version".into());
    }
    Ok(Some((header, 12 + u64::from(len))))
}

pub(super) fn pending_for_write(req: &SidecarWriteRequest) -> Result<Vec<PendingCleanup>, String> {
    let Some((old, _)) = read_header(&req.sidecar_path)? else {
        return Ok(Vec::new());
    };
    if old.info_hash != req.info_hash_hex {
        return Err("parts belongs to another torrent".into());
    }
    Ok(old
        .pending_cleanup
        .into_iter()
        .filter(|entry| {
            // A newly completed mapping owns this exact path again; it is no longer pending deletion.
            !req.selected.iter().any(|(_, path)| {
                rel_path_string(path, &req.save_dir).is_ok_and(|rel| rel == entry.rel_path)
            })
        })
        .collect())
}

struct Source {
    path: PathBuf,
    offset: u64,
    len: u64,
}

fn replace_sidecar(path: &Path, header: &PartsHeader, sources: &[Source]) -> Result<(), String> {
    let json = serde_json::to_vec(header).map_err(|e| e.to_string())?;
    let len = u32::try_from(json.len()).map_err(|_| "parts header too large")?;
    if len > MAX_HEADER_LEN {
        return Err("parts header too large".into());
    }
    let temporary = path.with_file_name(format!(".fluxdown-parts-{}", uuid::Uuid::new_v4()));
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let _guard = RestoreTemporary(temporary.clone());
    out.write_all(PARTS_MAGIC).map_err(|e| e.to_string())?;
    out.write_all(&len.to_le_bytes())
        .map_err(|e| e.to_string())?;
    out.write_all(&json).map_err(|e| e.to_string())?;
    for source in sources {
        let mut input = File::open(&source.path).map_err(|e| e.to_string())?;
        input
            .seek(SeekFrom::Start(source.offset))
            .map_err(|e| e.to_string())?;
        let copied =
            std::io::copy(&mut input.take(source.len), &mut out).map_err(|e| e.to_string())?;
        if copied != source.len {
            return Err("short read retaining shared piece".into());
        }
    }
    out.sync_all().map_err(|e| e.to_string())?;
    drop(out);
    std::fs::rename(&temporary, path).map_err(|e| e.to_string())?;
    // Persist the journal before removing any complete output.
    #[cfg(unix)]
    if let Some(parent) = path.parent() {
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(unix)]
fn persist_unlink(path: &Path) -> Result<(), String> {
    let mut parent = path.parent().ok_or("output has no parent")?;
    loop {
        match File::open(parent) {
            Ok(directory) => return directory.sync_all().map_err(|error| error.to_string()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                parent = parent.parent().ok_or("output has no existing ancestor")?;
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// Caller must stop this task and hold its file lock throughout this blocking operation.
pub(crate) fn clean_unselected_outputs(
    sidecar: &Path,
    save_dir: &Path,
    torrent: &[u8],
    selected: &HashSet<usize>,
    protected: &[PathBuf],
) -> Result<(), String> {
    let Some((mut header, _)) = read_header(sidecar)? else {
        return Ok(());
    };
    let save_dir = std::fs::canonicalize(save_dir).map_err(|e| e.to_string())?;
    let layout = load_seed_layout(sidecar, &save_dir)?.ok_or("parts disappeared")?;
    let parsed = librqbit::torrent_from_bytes(torrent).map_err(|e| e.to_string())?;
    let info = parsed.info.data.validate().map_err(|e| e.to_string())?;
    let files: Vec<_> = info
        .iter_file_details_ext()
        .map(|file| PartsFileMeta {
            relative_path: PathBuf::from(file.details.filename.to_string()),
            len: file.details.len,
            offset_in_torrent: file.offset,
            piece_range: file.pieces,
            padding: file.details.attrs().padding,
        })
        .collect();
    if header.info_hash != parsed.info_hash.as_string()
        || header.piece_length != info.lengths().default_piece_length()
        || header.total_length != info.lengths().total_length()
    {
        return Err("parts metadata does not match torrent".into());
    }
    for id in selected {
        if files.get(*id).is_none_or(|file| file.padding) {
            return Err("invalid selected file index".into());
        }
    }
    for segment in &header.segments {
        if files.get(segment.file_id).is_none_or(|file| {
            file.padding
                || segment
                    .offset
                    .checked_add(segment.len)
                    .is_none_or(|end| end > file.len)
        }) {
            return Err("invalid boundary segment".into());
        }
    }
    for segments in layout.parts.values() {
        if segments.windows(2).any(|pair| {
            pair[0]
                .offset
                .checked_add(pair[0].len)
                .is_none_or(|end| end > pair[1].offset)
        }) {
            return Err("overlapping boundary segments".into());
        }
    }
    let mut ids = HashSet::new();
    let mut paths = HashSet::new();
    for entry in &header.selected {
        if files
            .get(entry.file_id)
            .is_none_or(|f| f.padding || f.len != entry.len)
            || !ids.insert(entry.file_id)
            || !paths.insert(entry.rel_path.clone())
        {
            return Err("invalid completed output mapping".into());
        }
    }
    for entry in &header.pending_cleanup {
        if files
            .get(entry.file_id)
            .is_none_or(|f| f.padding || f.len != entry.len)
            || !paths.insert(entry.rel_path.clone())
        {
            return Err("invalid pending output mapping".into());
        }
    }
    // Protect wanted mappings as well, even if the caller has no sidecar view yet.
    let mut protected = protected.to_vec();
    for entry in &header.selected {
        if selected.contains(&entry.file_id) {
            protected.push(
                canonical_claim(&resolve_rel_path(&entry.rel_path, &save_dir)?)
                    .map_err(|error| format!("{error:?}"))?,
            );
        }
    }
    for entry in &header.pending_cleanup {
        if selected.contains(&entry.file_id) {
            protected.push(
                canonical_claim(&resolve_rel_path(&entry.rel_path, &save_dir)?)
                    .map_err(|error| format!("{error:?}"))?,
            );
        }
    }
    // Preflight ALL candidates before the first journal or output mutation.
    let mut pending = header.pending_cleanup.clone();
    for entry in &header.selected {
        if selected.contains(&entry.file_id) {
            continue;
        }
        let path = resolve_rel_path(&entry.rel_path, &save_dir)?;
        if validate_cleanup_target(&path, &protected)? {
            pending.push(PendingCleanup::capture(entry, &path)?);
        }
    }
    for entry in &pending {
        if selected.contains(&entry.file_id) {
            continue;
        }
        let path = resolve_rel_path(&entry.rel_path, &save_dir)?;
        if validate_cleanup_target(&path, &protected)? && !entry.matches(&path) {
            return Err("pending output was replaced; refusing cleanup".into());
        }
    }
    let needed = boundary_segments(
        &files,
        selected,
        u64::from(header.piece_length),
        header.total_length,
    );
    let old_selected = header.selected.clone();
    let mut segments = Vec::new();
    let mut sources = Vec::new();
    let mut cursor = 0u64;
    for (id, offset, len) in needed {
        if let Some(entry) = old_selected.iter().find(|entry| entry.file_id == id) {
            let path = resolve_rel_path(&entry.rel_path, &save_dir)?;
            // An already missing output contributes no bytes; recheck/redownload will fill it.
            if path.exists() {
                segments.push(SegmentEntry {
                    file_id: id,
                    offset,
                    len,
                    blob_offset: cursor,
                });
                sources.push(Source { path, offset, len });
                cursor += len;
            }
        } else if let Some(entry) = header.pending_cleanup.iter().find(|entry| {
            entry.file_id == id
                && resolve_rel_path(&entry.rel_path, &save_dir)
                    .is_ok_and(|path| pending_path_owned(entry, &path))
        }) {
            let path = resolve_rel_path(&entry.rel_path, &save_dir)?;
            segments.push(SegmentEntry {
                file_id: id,
                offset,
                len,
                blob_offset: cursor,
            });
            sources.push(Source { path, offset, len });
            cursor += len;
        } else if let Some(old) = layout.parts.get(&id) {
            for segment in old {
                let start = offset.max(segment.offset);
                let end = (offset + len).min(segment.offset + segment.len);
                if start < end {
                    segments.push(SegmentEntry {
                        file_id: id,
                        offset: start,
                        len: end - start,
                        blob_offset: cursor,
                    });
                    sources.push(Source {
                        path: sidecar.to_path_buf(),
                        offset: segment.blob_pos + start - segment.offset,
                        len: end - start,
                    });
                    cursor += end - start;
                }
            }
        }
    }
    header
        .selected
        .retain(|entry| selected.contains(&entry.file_id));
    header.segments = segments;
    header.pending_cleanup = pending;
    replace_sidecar(sidecar, &header, &sources)?;
    // The journal now owns retained boundary bytes, so deleting full outputs is safe.
    let mut error = None;
    let mut remaining = Vec::new();
    for entry in &header.pending_cleanup {
        if selected.contains(&entry.file_id) {
            remaining.push(entry.clone());
            continue;
        }
        let path = resolve_rel_path(&entry.rel_path, &save_dir)?;
        let result = (|| {
            if !validate_cleanup_target(&path, &protected)? {
                #[cfg(unix)]
                persist_unlink(&path)?;
                return Ok(());
            }
            if !entry.matches(&path) {
                return Err("pending output was replaced; refusing cleanup".into());
            }
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
            // Do not retire a journal entry until the unlink itself is durable.
            #[cfg(unix)]
            persist_unlink(&path)?;
            Ok(())
        })();
        if let Err(failure) = result {
            error.get_or_insert(failure);
            remaining.push(entry.clone());
        }
    }
    if remaining.len() != header.pending_cleanup.len() {
        header.pending_cleanup = remaining;
        // Reuse the just-written blob without buffering whole shared pieces in memory.
        let (_, base) = read_header(sidecar)?.ok_or("parts disappeared")?;
        let sources: Vec<_> = header
            .segments
            .iter()
            .map(|segment| Source {
                path: sidecar.to_path_buf(),
                offset: base + segment.blob_offset,
                len: segment.len,
            })
            .collect();
        replace_sidecar(sidecar, &header, &sources)?;
    }
    match error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
