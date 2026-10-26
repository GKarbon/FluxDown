use super::fixture::manager;
use fluxdown_engine::db::Db;
use fluxdown_engine::download_manager::NewTaskSpec;
use librqbit::{CreateTorrentOptions, create_torrent};

#[tokio::test]
async fn cleanup_only_removes_unselected_staging_and_is_retryable() {
    let root = std::env::temp_dir().join(format!("fluxdown-bt-clean-{}", uuid::Uuid::new_v4()));
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("a.bin"), vec![1u8; 32_768]).unwrap();
    std::fs::write(source.join("b.bin"), vec![2u8; 65_536]).unwrap();
    let torrent = create_torrent(
        &source,
        CreateTorrentOptions {
            piece_length: Some(16_384),
            ..Default::default()
        },
        &librqbit::spawn_utils::BlockingSpawner::new(1),
    )
    .await
    .unwrap()
    .as_bytes()
    .unwrap();
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.seed_builtin_queues().await.unwrap();
    let mut downloads = manager(db.clone(), &root, "invalid-tracker-url");
    let id = downloads
        .create_task(NewTaskSpec {
            torrent_file_bytes: torrent.to_vec(),
            start_paused: true,
            save_dir: root.join("save").to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await
        .unwrap();
    let files = downloads.bt_task_files(&id).await.unwrap();
    let a = files
        .files
        .iter()
        .find(|file| file.path == "a.bin")
        .unwrap()
        .index;
    downloads.set_bt_task_files(&id, &[a]).await.unwrap();
    let stage =
        fluxdown_engine::bt_downloader::bt_stage_dir(&root.join("save").to_string_lossy(), &id);
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::write(stage.join("a.bin"), b"selected data").unwrap();
    std::fs::write(stage.join("b.bin"), b"unselected data").unwrap();
    std::fs::write(stage.join("stranger.bin"), b"stranger").unwrap();
    std::fs::create_dir_all(root.join("save")).unwrap();
    std::fs::write(root.join("save/final.bin"), b"completed output").unwrap();
    #[cfg(unix)]
    {
        std::fs::remove_file(stage.join("b.bin")).unwrap();
        std::os::unix::fs::symlink(root.join("save/final.bin"), stage.join("b.bin")).unwrap();
        assert!(downloads.clean_bt_unselected(&id, &[a]).await.is_err());
        assert_eq!(
            std::fs::read(root.join("save/final.bin")).unwrap(),
            b"completed output"
        );
        std::fs::remove_file(stage.join("b.bin")).unwrap();
        std::fs::hard_link(root.join("save/final.bin"), stage.join("b.bin")).unwrap();
        assert!(downloads.clean_bt_unselected(&id, &[a]).await.is_err());
        std::fs::remove_file(stage.join("b.bin")).unwrap();
        std::fs::write(stage.join("b.bin"), b"unselected data").unwrap();
    }
    let owner = downloads
        .create_task(NewTaskSpec {
            url: "http://127.0.0.1:9/b.bin".into(),
            file_name: "b.bin".into(),
            save_dir: stage.to_string_lossy().into_owned(),
            start_paused: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(downloads.clean_bt_unselected(&id, &[a]).await.is_err());
    assert_eq!(
        std::fs::read(stage.join("b.bin")).unwrap(),
        b"unselected data"
    );
    db.update_task_file_name(&owner, "main.bin", false)
        .await
        .unwrap();
    db.add_task_artifact(&owner, "b.bin").await.unwrap();
    assert!(downloads.clean_bt_unselected(&id, &[a]).await.is_err());
    assert_eq!(
        std::fs::read(stage.join("b.bin")).unwrap(),
        b"unselected data"
    );
    downloads.delete_task(&owner, false).await;
    db.update_task_status(&id, 1, "").await.unwrap();
    assert!(downloads.clean_bt_unselected(&id, &[a]).await.is_err());
    assert!(stage.join("b.bin").exists());
    db.update_task_status(&id, 2, "").await.unwrap();
    assert!(
        downloads.clean_bt_unselected(&id, &[]).await.is_err(),
        "a stale all-files confirmation must not clean a subset"
    );
    assert!(stage.join("b.bin").exists());
    downloads.clean_bt_unselected(&id, &[a]).await.unwrap();
    downloads.clean_bt_unselected(&id, &[a]).await.unwrap();
    assert!(!stage.join("b.bin").exists());
    assert_eq!(
        std::fs::read(stage.join("a.bin")).unwrap(),
        b"selected data"
    );
    assert_eq!(
        std::fs::read(stage.join("stranger.bin")).unwrap(),
        b"stranger"
    );
    assert_eq!(
        std::fs::read(root.join("save/final.bin")).unwrap(),
        b"completed output"
    );
    assert_eq!(db.load_task_by_id(&id).await.unwrap().unwrap().status, 2);
    #[cfg(unix)]
    {
        let alias = root.join("save-alias");
        std::os::unix::fs::symlink(root.join("save"), &alias).unwrap();
        let alias_id = downloads
            .create_task(NewTaskSpec {
                torrent_file_bytes: torrent.to_vec(),
                start_paused: true,
                save_dir: alias.to_string_lossy().into_owned(),
                ..Default::default()
            })
            .await
            .unwrap();
        downloads.set_bt_task_files(&alias_id, &[a]).await.unwrap();
        let alias_stage =
            fluxdown_engine::bt_downloader::bt_stage_dir(&alias.to_string_lossy(), &alias_id);
        std::fs::create_dir_all(&alias_stage).unwrap();
        std::fs::write(alias_stage.join("a.bin"), b"selected via trusted alias").unwrap();
        std::fs::write(alias_stage.join("b.bin"), b"remove").unwrap();
        downloads
            .clean_bt_unselected(&alias_id, &[a])
            .await
            .unwrap();
        assert!(!alias_stage.join("b.bin").exists());
        assert_eq!(
            std::fs::read(alias_stage.join("a.bin")).unwrap(),
            b"selected via trusted alias"
        );
    }
    downloads.shutdown().await;
    std::fs::remove_dir_all(root).unwrap();
}
