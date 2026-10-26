use super::fixture::{manager, swarm, wait_status};
use fluxdown_engine::db::Db;
use fluxdown_engine::download_manager::NewTaskSpec;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn magnet_metadata_survives_restart_without_peers_or_rqbit_cache() {
    let swarm = swarm().await;
    let hash = librqbit::torrent_from_bytes(&swarm.torrent)
        .unwrap()
        .info_hash
        .as_string();
    let db = Db::connect(&format!(
        "sqlite://{}?mode=rwc",
        swarm.root.join("tasks.sqlite").display()
    ))
    .await
    .unwrap();
    db.seed_builtin_queues().await.unwrap();
    db.set_config("bt_seed_enabled", "0").await.unwrap();
    let mut downloads = manager(db.clone(), &swarm.root, &swarm.tracker_url);
    let mut done = downloads.take_done_rx().unwrap();
    let mut progress = downloads.take_progress_rx().unwrap();
    let drain = tokio::spawn(async move { while progress.recv().await.is_some() {} });
    let id = downloads
        .create_task(NewTaskSpec {
            url: format!("magnet:?xt=urn:btih:{hash}&dn=old-hint"),
            start_paused: true,
            unattended_selection: true,
            save_dir: swarm.root.join("save").to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(!downloads.bt_task_files(&id).await.unwrap().metadata_ready);
    downloads.resume_task(&id).await;
    wait_status(&mut downloads, &db, &mut done, &id, 3).await;
    downloads.shutdown().await;
    drop(downloads);
    swarm.seeder.stop().await;
    swarm.tracker_task.abort();
    drain.abort();
    let cache =
        fluxdown_engine::bt_partfile::persistence_dir(&swarm.root.join("data").to_string_lossy())
            .join(format!("{hash}.torrent"));
    if cache.exists() {
        std::fs::remove_file(cache).unwrap();
    }
    let mut reopened = manager(db.clone(), &swarm.root, "invalid-tracker-url");
    let files = reopened.bt_task_files(&id).await.unwrap();
    assert!(
        files.metadata_ready,
        "resolved magnet metadata must outlive the session cache"
    );
    assert_eq!(files.files.len(), 4);
    assert!(files.files.iter().all(|f| f.selected));
    reopened.shutdown().await;
    std::fs::remove_dir_all(swarm.root).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn legacy_metadata_cache_is_pinned_and_wrong_info_hash_is_rejected() {
    let swarm = swarm().await;
    let hash = librqbit::torrent_from_bytes(&swarm.torrent)
        .unwrap()
        .info_hash
        .as_string();
    let db = Db::connect(&format!(
        "sqlite://{}?mode=rwc",
        swarm.root.join("tasks.sqlite").display()
    ))
    .await
    .unwrap();
    db.seed_builtin_queues().await.unwrap();
    let mut downloads = manager(db.clone(), &swarm.root, &swarm.tracker_url);
    let id = downloads
        .create_task(NewTaskSpec {
            url: format!("magnet:?xt=urn:btih:{hash}&dn=old-hint"),
            unattended_selection: true,
            start_paused: true,
            save_dir: swarm.root.join("save").to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await
        .unwrap();
    let cache_dir =
        fluxdown_engine::bt_partfile::persistence_dir(&swarm.root.join("data").to_string_lossy());
    std::fs::create_dir_all(&cache_dir).unwrap();
    let path = cache_dir.join(format!("{hash}.torrent"));
    std::fs::write(&path, &swarm.torrent).unwrap();
    assert!(downloads.bt_task_files(&id).await.unwrap().metadata_ready);
    std::fs::remove_file(path).unwrap();
    assert!(
        downloads.bt_task_files(&id).await.unwrap().metadata_ready,
        "a verified legacy cache must survive eviction"
    );
    let wrong_hash = "0123456789012345678901234567890123456789";
    let other = downloads
        .create_task(NewTaskSpec {
            url: format!("magnet:?xt=urn:btih:{wrong_hash}"),
            start_paused: true,
            save_dir: swarm.root.join("save").to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await
        .unwrap();
    std::fs::write(
        cache_dir.join(format!("{wrong_hash}.torrent")),
        &swarm.torrent,
    )
    .unwrap();
    assert!(
        downloads.bt_task_files(&other).await.is_err(),
        "another torrent's cached file list must never be displayed"
    );
    assert_eq!(db.load_task_by_id(&id).await.unwrap().unwrap().status, 2);
    // Historical records could incorrectly persist the magnet hint as a custom rename.
    db.save_bt_custom_name(&id, "old-hint").await.unwrap();
    let mut done = downloads.take_done_rx().unwrap();
    let mut progress = downloads.take_progress_rx().unwrap();
    let drain = tokio::spawn(async move { while progress.recv().await.is_some() {} });
    downloads.resume_task(&id).await;
    wait_status(&mut downloads, &db, &mut done, &id, 3).await;
    assert_eq!(
        db.load_task_by_id(&id).await.unwrap().unwrap().file_name,
        "file-test"
    );
    downloads.shutdown().await;
    drain.abort();
    swarm.seeder.stop().await;
    swarm.tracker_task.abort();
    std::fs::remove_dir_all(swarm.root).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bt_files_corrupt_metadata_resume_keeps_writer_stopped_and_can_retry() {
    use super::fixture::settle;
    use std::time::Duration;
    let swarm = swarm().await;
    let db = Db::connect(&format!(
        "sqlite://{}?mode=rwc",
        swarm.root.join("tasks.sqlite").display()
    ))
    .await
    .unwrap();
    db.seed_builtin_queues().await.unwrap();
    db.set_config("bt_seed_enabled", "0").await.unwrap();
    let mut downloads = manager(db.clone(), &swarm.root, &swarm.tracker_url);
    let mut done = downloads.take_done_rx().unwrap();
    let mut progress = downloads.take_progress_rx().unwrap();
    let drain = tokio::spawn(async move { while progress.recv().await.is_some() {} });
    let id = downloads
        .create_task(NewTaskSpec {
            torrent_file_bytes: swarm.torrent.to_vec(),
            start_paused: true,
            unattended_selection: true,
            save_dir: swarm.root.join("save").to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await
        .unwrap();
    downloads.resume_task(&id).await;
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            settle(&mut downloads, &mut done).await;
            if downloads
                .bt_task_files(&id)
                .await
                .unwrap()
                .files
                .iter()
                .any(|f| f.downloaded_bytes.is_some_and(|n| n >= 65_536))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    downloads.pause_task(&id).await;
    wait_status(&mut downloads, &db, &mut done, &id, 2).await;
    let stage = fluxdown_engine::bt_downloader::bt_stage_dir(
        &swarm.root.join("save").to_string_lossy(),
        &id,
    );
    let snapshot = || {
        ["a.bin", "nested/b.bin", "c.bin", "empty.bin"].map(|name| {
            let path = stage.join(name);
            if path.exists() {
                Some(std::fs::read(path).unwrap())
            } else {
                None
            }
        })
    };
    let before = snapshot();
    db.save_torrent_file_bytes(&id, b"corrupt local metadata")
        .await
        .unwrap();
    downloads.resume_task(&id).await;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            settle(&mut downloads, &mut done).await;
            if db.load_task_by_id(&id).await.unwrap().unwrap().status == 4 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("metadata failure must settle after pause flushing");
    let failed = db.load_task_by_id(&id).await.unwrap().unwrap();
    assert_eq!(
        failed.status, 4,
        "cache rejection must leave a stable reported error, not a preparing orphan"
    );
    assert!(failed.error_message.contains("metadata"));
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert_eq!(
        snapshot(),
        before,
        "failed metadata validation must not unpause a cached writer"
    );
    db.save_torrent_file_bytes(&id, &swarm.torrent)
        .await
        .unwrap();
    downloads.resume_task(&id).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            settle(&mut downloads, &mut done).await;
            if db.load_task_by_id(&id).await.unwrap().unwrap().status != 4 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("retry must leave the prior metadata error after a new generation starts");
    wait_status(&mut downloads, &db, &mut done, &id, 3).await;
    assert_eq!(
        std::fs::read(swarm.root.join("save/file-test/a.bin")).unwrap(),
        std::fs::read(swarm.content.join("a.bin")).unwrap()
    );
    downloads.shutdown().await;
    drain.abort();
    swarm.seeder.stop().await;
    swarm.tracker_task.abort();
    std::fs::remove_dir_all(swarm.root).unwrap();
}
