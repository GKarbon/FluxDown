//! Existing-task file commands through the public manager, with a loopback-only swarm.
#![cfg(feature = "bt")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod cleanup;
mod deletion;
mod display;
mod fixture;
mod inputs;
mod metadata;
use fixture::{manager, settle, wait_status};

use std::time::Duration;

use fluxdown_engine::db::Db;
use fluxdown_engine::download_manager::{BtFilesError, NewTaskSpec};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn bt_files_reselect_pause_clean_append_all_and_restart_preserve_data() {
    let fixture::Swarm {
        root,
        content,
        torrent,
        seeder,
        tracker_url,
        tracker_task,
    } = fixture::swarm().await;
    let db = Db::connect(&format!(
        "sqlite://{}?mode=rwc",
        root.join("tasks.sqlite").display()
    ))
    .await
    .unwrap();
    db.seed_builtin_queues().await.unwrap();
    db.set_config("bt_seed_enabled", "false").await.unwrap();
    let mut downloads = manager(db.clone(), &root, &tracker_url);
    let mut done = downloads.take_done_rx().unwrap();
    let mut progress = downloads.take_progress_rx().unwrap();
    let progress_task = tokio::spawn(async move { while progress.recv().await.is_some() {} });
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
    assert!(files.metadata_ready);
    assert_eq!(files.files.len(), 4);
    let a = files
        .files
        .iter()
        .find(|f| f.path == "a.bin")
        .unwrap()
        .index;
    let b = files
        .files
        .iter()
        .find(|f| f.path.replace('\\', "/") == "nested/b.bin")
        .unwrap()
        .index;
    let c = files
        .files
        .iter()
        .find(|f| f.path == "c.bin")
        .unwrap()
        .index;
    let before: Vec<_> = files.files.iter().map(|f| f.selected).collect();
    assert!(matches!(
        downloads.set_bt_task_files(&id, &[a, -1]).await,
        Err(BtFilesError::Invalid(_))
    ));
    assert_eq!(
        downloads
            .bt_task_files(&id)
            .await
            .unwrap()
            .files
            .iter()
            .map(|f| f.selected)
            .collect::<Vec<_>>(),
        before
    );
    downloads.set_bt_task_files(&id, &[a]).await.unwrap();
    assert_eq!(db.load_task_by_id(&id).await.unwrap().unwrap().status, 2);
    downloads.resume_task(&id).await;
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            let files = downloads.bt_task_files(&id).await.unwrap();
            if files
                .files
                .iter()
                .any(|f| f.index == a && f.downloaded_bytes.is_some_and(|n| n >= 65_536))
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(matches!(
        downloads.clean_bt_unselected(&id, &[a]).await,
        Err(BtFilesError::Busy(_))
    ));
    downloads.pause_task(&id).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            settle(&mut downloads, &mut done).await;
            match downloads.clean_bt_unselected(&id, &[a]).await {
                Ok(()) => break,
                Err(BtFilesError::Busy(_)) => tokio::time::sleep(Duration::from_millis(30)).await,
                Err(error) => panic!("clean stopped torrent: {error}"),
            }
        }
    })
    .await
    .unwrap();
    assert_eq!(db.load_task_by_id(&id).await.unwrap().unwrap().status, 2);
    downloads.resume_task(&id).await;
    // Retry only Busy: the 500 ms progress/completion critical section may be held.
    loop {
        match downloads.set_bt_task_files(&id, &[b]).await {
            Ok(()) => break,
            Err(BtFilesError::Busy(_)) => {
                let unchanged = downloads.bt_task_files(&id).await.unwrap();
                assert_eq!(
                    unchanged
                        .files
                        .iter()
                        .filter(|file| file.selected)
                        .map(|file| file.index)
                        .collect::<Vec<_>>(),
                    vec![a]
                );
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
            Err(e) => panic!("reselect: {e}"),
        }
    }
    wait_status(&mut downloads, &db, &mut done, &id, 3).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    settle(&mut downloads, &mut done).await;
    let stopped_stage =
        fluxdown_engine::bt_downloader::bt_stage_dir(&root.join("save").to_string_lossy(), &id)
            .join("a.bin");
    let retained = std::fs::read(&stopped_stage)
        .expect("cancelled file data survives another file completing");
    let original = std::fs::read(content.join("a.bin")).unwrap();
    assert!(
        retained
            .as_chunks::<16_384>()
            .0
            .iter()
            .zip(original.as_chunks::<16_384>().0.iter())
            .any(|(a, b)| a == b),
        "must retain verified internal piece bytes"
    );
    downloads.shutdown().await;
    drop(downloads);
    let mut downloads = manager(db.clone(), &root, &tracker_url);
    let mut done = downloads.take_done_rx().unwrap();
    let mut append_progress = downloads.take_progress_rx().unwrap();
    let append_progress_task =
        tokio::spawn(async move { while append_progress.recv().await.is_some() {} });
    downloads.load_and_send_all_tasks().await;
    assert_eq!(
        std::fs::read(&stopped_stage).unwrap(),
        retained,
        "startup must keep deliberately stopped files"
    );
    downloads.set_bt_task_files(&id, &[a, b]).await.unwrap();
    wait_status(&mut downloads, &db, &mut done, &id, 3).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    settle(&mut downloads, &mut done).await;
    let completed = db.load_task_by_id(&id).await.unwrap().unwrap();
    assert!(
        root.join("save").join(&completed.file_name).exists(),
        "task must name an existing completed artifact"
    );
    let completed = db.load_task_by_id(&id).await.unwrap().unwrap();
    assert!(
        root.join("save").join(&completed.file_name).exists(),
        "completed task must name an existing artifact"
    );
    let sidecar =
        fluxdown_engine::bt_partfile::sidecar_path(&root.join("data").to_string_lossy(), &id);
    let layout = fluxdown_engine::bt_partfile::load_sidecar_layout(&sidecar, &root.join("save"))
        .unwrap()
        .unwrap();
    let a_final = layout
        .selected_path(usize::try_from(a).unwrap())
        .unwrap()
        .to_path_buf();
    assert_eq!(
        std::fs::read(&a_final).unwrap(),
        std::fs::read(content.join("a.bin")).unwrap()
    );
    // Simulate an ordinary completed task, then crash after restore but before commit.
    db.delete_config(&format!("bt_preserve_stage_{id}"))
        .await
        .unwrap();
    let stage =
        fluxdown_engine::bt_downloader::bt_stage_dir(&root.join("save").to_string_lossy(), &id);
    if stage.exists() {
        std::fs::remove_dir_all(&stage).unwrap();
    }
    let preparation = downloads
        .prepare_bt_task_file_change(&id, &[a, b, c])
        .await
        .unwrap()
        .unwrap();
    tokio::task::spawn_blocking(move || preparation.restore())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(db.load_task_by_id(&id).await.unwrap().unwrap().status, 3);
    let name_before_restart = db.load_task_by_id(&id).await.unwrap().unwrap().file_name;
    downloads.shutdown().await;
    drop(downloads);
    let mut downloads = manager(db.clone(), &root, &tracker_url);
    let mut done = downloads.take_done_rx().unwrap();
    let mut restored_progress = downloads.take_progress_rx().unwrap();
    let restored_progress_task =
        tokio::spawn(async move { while restored_progress.recv().await.is_some() {} });
    downloads.load_and_send_all_tasks().await;
    assert_eq!(
        db.load_task_by_id(&id).await.unwrap().unwrap().file_name,
        name_before_restart
    );
    assert!(
        stage.join("a.bin").exists(),
        "restore without commit is protected from startup rescue"
    );
    downloads.set_bt_task_files(&id, &[a, b, c]).await.unwrap();
    wait_status(&mut downloads, &db, &mut done, &id, 3).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    settle(&mut downloads, &mut done).await;
    let layout = fluxdown_engine::bt_partfile::load_sidecar_layout(&sidecar, &root.join("save"))
        .unwrap()
        .unwrap();
    assert_eq!(
        layout.selected_path(usize::try_from(a).unwrap()).unwrap(),
        a_final
    );
    assert_eq!(
        db.load_task_by_id(&id).await.unwrap().unwrap().file_name,
        name_before_restart,
        "adding files must reuse the task-owned completed container"
    );
    let mut final_paths = Vec::new();
    for (index, name) in [(a, "a.bin"), (b, "nested/b.bin"), (c, "c.bin")] {
        let path = layout
            .selected_path(usize::try_from(index).unwrap())
            .unwrap()
            .to_path_buf();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            std::fs::read(content.join(name)).unwrap()
        );
        final_paths.push(path);
    }
    downloads.set_bt_task_files(&id, &[a]).await.unwrap();
    wait_status(&mut downloads, &db, &mut done, &id, 3).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    settle(&mut downloads, &mut done).await;
    assert_eq!(
        db.load_task_by_id(&id).await.unwrap().unwrap().file_name,
        name_before_restart,
        "narrowing to one top-level file keeps the completed container"
    );
    assert!(root.join("save").join(&name_before_restart).exists());
    downloads.pause_task(&id).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    settle(&mut downloads, &mut done).await;
    let stopped = db.load_task_by_id(&id).await.unwrap().unwrap();
    assert!(
        !matches!(stopped.seeding_status, 1 | 8),
        "seeding must be stopped before cleanup"
    );
    let b_output = final_paths.get(1).unwrap().clone();
    let stranger = a_final.parent().unwrap().join("stranger.keep");
    std::fs::write(&stranger, b"unrelated user file").unwrap();
    let other = downloads
        .create_task(NewTaskSpec {
            url: "http://127.0.0.1:9/owned.bin".into(),
            start_paused: true,
            save_dir: b_output.parent().unwrap().to_string_lossy().into_owned(),
            file_name: b_output.file_name().unwrap().to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(downloads.clean_bt_unselected(&id, &[a]).await.is_err());
    assert!(
        final_paths.iter().all(|path| path.exists()),
        "other claims must be rejected before deleting any complete output"
    );
    downloads.delete_task(&other, false).await;
    assert!(
        downloads.clean_bt_unselected(&id, &[a, b]).await.is_err(),
        "stale confirmed range must be rejected"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let parent = b_output.parent().unwrap();
        let permissions = std::fs::metadata(parent).unwrap().permissions();
        std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o500)).unwrap();
        let failed = downloads.clean_bt_unselected(&id, &[a]).await;
        std::fs::set_permissions(parent, permissions).unwrap();
        assert!(
            failed.is_err(),
            "permission-denied deletion must report failure and retain a retryable record"
        );
        let original = std::fs::metadata(&b_output).unwrap();
        let backup = b_output.with_extension("original.keep");
        std::fs::rename(&b_output, &backup).unwrap();
        std::fs::write(
            &b_output,
            vec![9u8; usize::try_from(original.len()).unwrap()],
        )
        .unwrap();
        std::fs::File::open(&b_output)
            .unwrap()
            .set_times(std::fs::FileTimes::new().set_modified(original.modified().unwrap()))
            .unwrap();
        assert!(
            downloads.clean_bt_unselected(&id, &[a]).await.is_err(),
            "a replaced pending output must not be deleted, even with matching size and mtime"
        );
        assert_eq!(std::fs::read(&b_output).unwrap().first(), Some(&9));
        std::fs::remove_file(&b_output).unwrap();
        std::fs::rename(&backup, &b_output).unwrap();
    }
    downloads.clean_bt_unselected(&id, &[a]).await.unwrap();
    assert_eq!(
        std::fs::read(&a_final).unwrap(),
        std::fs::read(content.join("a.bin")).unwrap()
    );
    assert!(
        final_paths.iter().skip(1).all(|path| !path.exists()),
        "explicit cleanup must delete unselected complete outputs"
    );
    downloads.clean_bt_unselected(&id, &[a]).await.unwrap();
    assert_eq!(std::fs::read(&stranger).unwrap(), b"unrelated user file");
    let cleaned_layout =
        fluxdown_engine::bt_partfile::load_sidecar_layout(&sidecar, &root.join("save"))
            .unwrap()
            .unwrap();
    let boundary = cleaned_layout.into_boundary_reader().unwrap();
    let parsed = librqbit::torrent_from_bytes(&torrent).unwrap();
    let info = parsed.info.data.validate().unwrap();
    let metadata: Vec<_> = info.iter_file_details_ext().collect();
    let selected_meta = metadata.get(usize::try_from(a).unwrap()).unwrap();
    let piece_len = u64::from(info.lengths().default_piece_length());
    let mut retained_boundaries = 0;
    for (index, file) in metadata.iter().enumerate() {
        if i32::try_from(index).unwrap() == a || file.details.len == 0 {
            continue;
        }
        for edge in [
            selected_meta.offset,
            selected_meta.offset + selected_meta.details.len - 1,
        ] {
            let piece_start = edge / piece_len * piece_len;
            let start = piece_start.max(file.offset);
            let end = (piece_start + piece_len).min(file.offset + file.details.len);
            if start >= end {
                continue;
            }
            let offset = usize::try_from(start - file.offset).unwrap();
            let len = usize::try_from(end - start).unwrap();
            let mut actual = vec![0; len];
            boundary
                .read(index, u64::try_from(offset).unwrap(), &mut actual)
                .unwrap();
            let expected = std::fs::read(content.join(file.details.filename.to_string())).unwrap();
            assert_eq!(
                actual,
                expected[offset..offset + len],
                "selected shared-piece bytes must survive completed-output deletion"
            );
            retained_boundaries += 1;
        }
    }
    assert!(retained_boundaries > 0);

    // Empty RPC lists retain the original all-files sentinel; UI disallows zero choices.
    downloads.set_bt_task_files(&id, &[]).await.unwrap();
    wait_status(&mut downloads, &db, &mut done, &id, 3).await;
    assert!(
        downloads
            .bt_task_files(&id)
            .await
            .unwrap()
            .files
            .iter()
            .all(|f| f.selected)
    );
    let restored_layout =
        fluxdown_engine::bt_partfile::load_sidecar_layout(&sidecar, &root.join("save"))
            .unwrap()
            .unwrap();
    final_paths.clear();
    for (index, name) in [(a, "a.bin"), (b, "nested/b.bin"), (c, "c.bin")] {
        let path = restored_layout
            .selected_path(usize::try_from(index).unwrap())
            .unwrap();
        assert_eq!(
            std::fs::read(path).unwrap(),
            std::fs::read(content.join(name)).unwrap(),
            "reselected cleaned files must redownload byte-exact content"
        );
        final_paths.push(path.to_path_buf());
    }
    downloads.shutdown().await;
    drop(downloads);
    let mut reopened = manager(db.clone(), &root, &tracker_url);
    let mut done = reopened.take_done_rx().unwrap();
    let mut resumed_progress = reopened.take_progress_rx().unwrap();
    let resumed_progress_task =
        tokio::spawn(async move { while resumed_progress.recv().await.is_some() {} });
    reopened.set_bt_task_files(&id, &[a, b, c]).await.unwrap();
    reopened.resume_task(&id).await;
    wait_status(&mut reopened, &db, &mut done, &id, 3).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    settle(&mut reopened, &mut done).await;
    let zero = reopened
        .bt_task_files(&id)
        .await
        .unwrap()
        .files
        .iter()
        .find(|f| f.size == 0)
        .unwrap()
        .index;
    let preparation = reopened
        .prepare_bt_task_file_change(&id, &[a, b, c, zero])
        .await
        .unwrap()
        .unwrap();
    let (release, wait) = std::sync::mpsc::channel();
    let copying = tokio::task::spawn_blocking(move || {
        wait.recv().unwrap();
        preparation.restore()
    });
    {
        let deleting = reopened.delete_task(&id, true);
        tokio::pin!(deleting);
        tokio::select! {
            _ = &mut deleting => panic!("delete must wait for the registered preparation"),
            _ = tokio::time::sleep(Duration::from_millis(100)) => {},
        }
        release.send(()).unwrap();
        deleting.await;
    }
    assert!(
        copying.await.unwrap().is_err(),
        "delete cancels copying before its next write"
    );
    assert!(db.load_task_by_id(&id).await.unwrap().is_none());
    assert!(!stage.exists(), "copying must not recreate a deleted stage");
    assert!(
        final_paths.iter().all(|path| !path.exists()),
        "explicit deletion must remove completed files from every selection"
    );
    resumed_progress_task.abort();
    restored_progress_task.abort();
    reopened.shutdown().await;
    seeder.stop().await;
    tracker_task.abort();
    progress_task.abort();
    append_progress_task.abort();
    std::fs::remove_dir_all(root).unwrap();
}
