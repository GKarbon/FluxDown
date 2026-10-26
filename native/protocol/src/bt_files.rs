//! Existing-task BitTorrent file details and exact selection updates.
use serde::{Deserialize, Serialize};

/// One non-padding metadata file; unknown verified progress is `null`, never an invented zero.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BtTaskFileDto {
    pub index: i32,
    pub path: String,
    pub size: i64,
    pub downloaded_bytes: Option<u64>,
    pub selected: bool,
}

/// Metadata is pending when `metadataReady` is false; file indices retain padding gaps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BtTaskFilesDto {
    pub task_id: String,
    pub metadata_ready: bool,
    pub files: Vec<BtTaskFileDto>,
}

/// Requested set; an empty list retains the original all-files sentinel. Changes retain data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SetBtTaskFilesParams {
    pub task_id: String,
    pub indices: Vec<i32>,
}

#[cfg(test)]
mod tests {
    use super::{BtTaskFilesDto, SetBtTaskFilesParams};
    #[test]
    fn nullable_progress_and_all_files_sentinel_keep_their_wire_meaning() {
        let raw = serde_json::json!({"taskId":"t", "metadataReady":true, "files":[{"index":2,"path":"nested/中文.bin","size":7,"downloadedBytes":null,"selected":false}]});
        let files: BtTaskFilesDto = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(files).unwrap(), raw);
        let command: SetBtTaskFilesParams =
            serde_json::from_value(serde_json::json!({"taskId":"t","indices":[]})).unwrap();
        assert!(command.indices.is_empty());
        assert!(crate::method::is_slow_daemon_method(
            crate::method::DAEMON_TASK_SET_BT_FILES
        ));
    }
}
