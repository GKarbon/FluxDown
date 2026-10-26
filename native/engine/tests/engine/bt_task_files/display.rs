use super::fixture::{manager, swarm, wait_status};
use fluxdown_engine::db::Db;
use fluxdown_engine::download_manager::NewTaskSpec;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bt_files_partial_completion_keeps_root_and_legacy_flat_outputs() {
    let swarm = swarm().await;
    let save = swarm.root.join("save");
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
            save_dir: save.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await
        .unwrap();
    let details = downloads.bt_task_files(&id).await.unwrap();
    let a = details
        .files
        .iter()
        .find(|f| f.path == "a.bin")
        .unwrap()
        .index;
    let c = details
        .files
        .iter()
        .find(|f| f.path == "c.bin")
        .unwrap()
        .index;
    downloads.set_bt_task_files(&id, &[a]).await.unwrap();
    downloads.resume_task(&id).await;
    wait_status(&mut downloads, &db, &mut done, &id, 3).await;
    let completed = db.load_task_by_id(&id).await.unwrap().unwrap();
    assert_eq!(completed.file_name, "file-test");
    assert!(save.join(&completed.file_name).is_dir());
    assert_eq!(
        std::fs::read(save.join("file-test/a.bin")).unwrap(),
        std::fs::read(swarm.content.join("a.bin")).unwrap()
    );
    downloads.shutdown().await;
    drop(downloads);
    drain.abort();

    // A historical flat completion: the same valid sidecar, with its selected output at save/a.bin.
    let sidecar =
        fluxdown_engine::bt_partfile::sidecar_path(&swarm.root.join("data").to_string_lossy(), &id);
    let bytes = std::fs::read(&sidecar).unwrap();
    let length = u32::from_le_bytes(bytes.get(8..12).unwrap().try_into().unwrap());
    let end = 12 + usize::try_from(length).unwrap();
    let mut header: serde_json::Value =
        serde_json::from_slice(bytes.get(12..end).unwrap()).unwrap();
    let entry = header
        .get_mut("selected")
        .unwrap()
        .as_array_mut()
        .unwrap()
        .first_mut()
        .unwrap();
    *entry.get_mut("rel_path").unwrap() = "a.bin".into();
    let header = serde_json::to_vec(&header).unwrap();
    let mut legacy = b"FXPARTS1".to_vec();
    legacy.extend_from_slice(&u32::try_from(header.len()).unwrap().to_le_bytes());
    legacy.extend_from_slice(&header);
    legacy.extend_from_slice(bytes.get(end..).unwrap());
    std::fs::rename(save.join("file-test/a.bin"), save.join("a.bin")).unwrap();
    std::fs::remove_dir(save.join("file-test")).unwrap();
    std::fs::write(&sidecar, legacy).unwrap();
    db.update_task_file_info(&id, "a.bin", completed.total_bytes)
        .await
        .unwrap();
    std::fs::write(save.join("stranger.txt"), b"unowned").unwrap();

    let mut reopened = manager(db.clone(), &swarm.root, &swarm.tracker_url);
    let mut done = reopened.take_done_rx().unwrap();
    let mut progress = reopened.take_progress_rx().unwrap();
    let drain = tokio::spawn(async move { while progress.recv().await.is_some() {} });
    reopened.set_bt_task_files(&id, &[a, c]).await.unwrap();
    wait_status(&mut reopened, &db, &mut done, &id, 3).await;
    let completed = db.load_task_by_id(&id).await.unwrap().unwrap();
    assert_eq!(completed.file_name, "file-test");
    for name in ["a.bin", "c.bin"] {
        assert_eq!(
            std::fs::read(save.join("file-test").join(name)).unwrap(),
            std::fs::read(swarm.content.join(name)).unwrap()
        );
    }
    assert_eq!(
        std::fs::read(save.join("a.bin")).unwrap(),
        std::fs::read(swarm.content.join("a.bin")).unwrap(),
        "legacy complete outputs are retained"
    );
    reopened.delete_task(&id, true).await;
    assert!(
        !save.join("a.bin").exists(),
        "legacy artifact ownership survives the transition"
    );
    assert!(!save.join("file-test").exists());
    assert_eq!(
        std::fs::read(save.join("stranger.txt")).unwrap(),
        b"unowned"
    );
    reopened.shutdown().await;
    drain.abort();
    swarm.seeder.stop().await;
    swarm.tracker_task.abort();
    std::fs::remove_dir_all(swarm.root).unwrap();
}
