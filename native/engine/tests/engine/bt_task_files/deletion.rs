use super::fixture::manager;
use fluxdown_engine::bt_partfile::{
    PartsFileMeta, SidecarWriteRequest, sidecar_path, write_sidecar,
};
use fluxdown_engine::db::Db;

#[tokio::test]
async fn bt_files_paused_completed_outputs_delete_without_removing_unowned_siblings() {
    let root =
        std::env::temp_dir().join(format!("fluxdown-bt-owned-delete-{}", uuid::Uuid::new_v4()));
    let save = root.join("save");
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.seed_builtin_queues().await.unwrap();
    let mut downloads = manager(db.clone(), &root, "invalid-tracker-url");
    for (id, batch, claimed) in [
        ("single", false, false),
        ("batch", true, false),
        ("claimed-single", false, true),
        ("claimed-batch", true, true),
    ] {
        let container = save.join(id);
        let owned = container.join("nested/owned.bin");
        let unowned = container.join("keep.txt");
        std::fs::create_dir_all(owned.parent().unwrap()).unwrap();
        std::fs::write(&owned, b"owned").unwrap();
        std::fs::write(&unowned, b"unrelated user data").unwrap();
        db.insert_task(
            id,
            "magnet:?xt=urn:btih:0123456789012345678901234567890123456789",
            id,
            &save.to_string_lossy(),
            2,
            5,
            "",
            "",
            "",
            0,
        )
        .await
        .unwrap();
        db.save_bt_selected_files(id, &[], false).await.unwrap();
        write_sidecar(&SidecarWriteRequest {
            sidecar_path: sidecar_path(&root.join("data").to_string_lossy(), id),
            info_hash_hex: "0123456789012345678901234567890123456789".into(),
            piece_length: 16_384,
            total_length: 5,
            files: vec![PartsFileMeta {
                relative_path: "nested/owned.bin".into(),
                len: 5,
                offset_in_torrent: 0,
                piece_range: 0..1,
                padding: false,
            }],
            selected: vec![(0, owned.clone())],
            save_dir: save.clone(),
            stage_dir: root.join("stage"),
        })
        .unwrap();
        if claimed {
            db.insert_task(
                &format!("other-{id}"),
                "https://example.invalid/keep",
                id,
                &save.to_string_lossy(),
                2,
                5,
                "",
                "",
                "",
                0,
            )
            .await
            .unwrap();
        }
        // Old top-level artifact records must never cause recursive deletion.
        db.add_task_artifact(id, id).await.unwrap();
        #[cfg(unix)]
        if !claimed {
            use std::os::unix::fs::PermissionsExt;
            let directory = owned.parent().unwrap();
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o500)).unwrap();
            if batch {
                let result = downloads
                    .delete_tasks_batch(&[id.to_owned()], true)
                    .await
                    .unwrap();
                assert_eq!(result.failed, 1);
            } else {
                downloads.delete_task(id, true).await;
            }
            assert!(db.load_task_by_id(id).await.unwrap().is_some());
            assert!(
                sidecar_path(&root.join("data").to_string_lossy(), id).exists(),
                "failed cleanup retains ownership for retry"
            );
            std::fs::set_permissions(directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        if batch {
            let result = downloads
                .delete_tasks_batch(&[id.to_owned()], true)
                .await
                .unwrap();
            assert_eq!(result.deleted, 1);
            assert_eq!(result.failed, 0);
        } else {
            downloads.delete_task(id, true).await;
        }
        assert!(db.load_task_by_id(id).await.unwrap().is_none());
        assert_eq!(owned.exists(), claimed);
        assert_eq!(container.join("nested").exists(), claimed);
        assert_eq!(std::fs::read(&unowned).unwrap(), b"unrelated user data");
    }
    downloads.shutdown().await;
    std::fs::remove_dir_all(root).unwrap();
}
