use super::fixture::{HeldInitialChoice, manager, manager_with_choice};
use fluxdown_engine::db::Db;
use fluxdown_engine::download_manager::{BtFilesError, NewTaskSpec};
use librqbit::{CreateTorrentOptions, create_torrent};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn bt_files_pending_padding_all_files_and_save_failure_have_explicit_semantics() {
    use sha1::{Digest, Sha1};
    use sqlx::Connection;
    let root =
        std::env::temp_dir().join(format!("fluxdown-bt-file-inputs-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let database_url = format!("sqlite://{}?mode=rwc", root.join("tasks.sqlite").display());
    let db = Db::connect(&database_url).await.unwrap();
    db.seed_builtin_queues().await.unwrap();
    let mut downloads = manager(db.clone(), &root, "invalid-tracker-url");
    let pending = downloads
        .create_task(NewTaskSpec {
            url: "magnet:?xt=urn:btih:0123456789012345678901234567890123456789".into(),
            start_paused: true,
            save_dir: root.join("save").to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await
        .unwrap();
    assert!(
        !downloads
            .bt_task_files(&pending)
            .await
            .unwrap()
            .metadata_ready
    );
    assert!(matches!(
        downloads.set_bt_task_files(&pending, &[]).await,
        Err(BtFilesError::Busy(_))
    ));
    assert!(matches!(
        downloads.bt_task_files("missing").await,
        Err(BtFilesError::NotFound)
    ));

    // One shared piece: file 0, padding slot 1, file 2, zero-byte file 3.
    let mut bytes = b"d4:infod5:filesld6:lengthi3e4:pathl5:a.bineed4:attr1:p6:lengthi2e4:pathl3:padeed6:lengthi4e4:pathl5:b.bineed6:lengthi0e4:pathl5:emptyeee4:name4:test12:piece lengthi16384e6:pieces20:".to_vec();
    bytes.extend_from_slice(&Sha1::digest(b"abc\0\0defg"));
    bytes.extend_from_slice(b"ee");
    let id = downloads
        .create_task(NewTaskSpec {
            torrent_file_bytes: bytes,
            start_paused: true,
            save_dir: root.join("save").to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await
        .unwrap();
    let details = downloads.bt_task_files(&id).await.unwrap();
    assert_eq!(
        details
            .files
            .iter()
            .map(|f| (f.index, f.size))
            .collect::<Vec<_>>(),
        vec![(0, 3), (2, 4), (3, 0)]
    );
    assert!(matches!(
        downloads.set_bt_task_files(&id, &[1]).await,
        Err(BtFilesError::Invalid(_))
    ));
    downloads.set_bt_task_files(&id, &[2, 2]).await.unwrap();
    let chosen = || vec![(0, false), (2, true), (3, false)];
    assert_eq!(
        downloads
            .bt_task_files(&id)
            .await
            .unwrap()
            .files
            .iter()
            .map(|f| (f.index, f.selected))
            .collect::<Vec<_>>(),
        chosen()
    );
    let mut connection = sqlx::SqliteConnection::connect(&database_url)
        .await
        .unwrap();
    sqlx::query("CREATE TRIGGER reject_file_edit BEFORE UPDATE OF bt_selected_files ON tasks BEGIN SELECT RAISE(FAIL, 'fixture rejects selection'); END;").execute(&mut connection).await.unwrap();
    assert!(matches!(
        downloads.set_bt_task_files(&id, &[0]).await,
        Err(BtFilesError::Operation(_))
    ));
    assert_eq!(
        downloads
            .bt_task_files(&id)
            .await
            .unwrap()
            .files
            .iter()
            .map(|f| (f.index, f.selected))
            .collect::<Vec<_>>(),
        chosen()
    );
    sqlx::query("DROP TRIGGER reject_file_edit")
        .execute(&mut connection)
        .await
        .unwrap();
    downloads.set_bt_task_files(&id, &[]).await.unwrap();
    assert!(
        downloads
            .bt_task_files(&id)
            .await
            .unwrap()
            .files
            .iter()
            .all(|file| file.selected)
    );
    downloads.set_bt_task_files(&id, &[2]).await.unwrap();
    let stage =
        fluxdown_engine::bt_downloader::bt_stage_dir(&root.join("save").to_string_lossy(), &id);
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::write(stage.join("a.bin"), b"abc").unwrap();
    std::fs::write(stage.join("b.bin"), b"defg").unwrap();
    std::fs::write(stage.join("stranger"), b"keep").unwrap();
    downloads.clean_bt_unselected(&id, &[2]).await.unwrap();
    assert_eq!(std::fs::read(stage.join("b.bin")).unwrap(), b"defg");
    assert_eq!(std::fs::read(stage.join("stranger")).unwrap(), b"keep");
    // All of this tiny unselected file belongs to the selected file's shared piece.
    assert_eq!(std::fs::read(stage.join("a.bin")).unwrap(), b"abc");
    downloads.clean_bt_unselected(&id, &[2]).await.unwrap();
    downloads.shutdown().await;
    drop(connection);
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bt_files_cannot_compete_with_an_outstanding_initial_dialog() {
    let root = std::env::temp_dir().join(format!(
        "fluxdown-bt-initial-choice-{}",
        uuid::Uuid::new_v4()
    ));
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("a.bin"), vec![17u8; 65_536]).unwrap();
    std::fs::write(source.join("b.bin"), vec![39u8; 65_536]).unwrap();
    let torrent = create_torrent(
        &source,
        CreateTorrentOptions {
            name: Some("initial-choice"),
            trackers: vec![],
            piece_length: Some(16_384),
        },
        &librqbit::spawn_utils::BlockingSpawner::new(1),
    )
    .await
    .unwrap()
    .as_bytes()
    .unwrap();
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.seed_builtin_queues().await.unwrap();
    let choice = Arc::new(HeldInitialChoice {
        arrived: tokio::sync::Notify::new(),
        answer: tokio::sync::Notify::new(),
        indices: vec![0],
    });
    let mut downloads = manager_with_choice(db, &root, "invalid-tracker-url", choice.clone());
    let id = downloads
        .create_task(NewTaskSpec {
            torrent_file_bytes: torrent.to_vec(),
            save_dir: root.join("save").to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), choice.arrived.notified())
        .await
        .unwrap();
    assert!(downloads.bt_task_files(&id).await.unwrap().metadata_ready);
    assert!(matches!(
        downloads.set_bt_task_files(&id, &[1]).await,
        Err(BtFilesError::Busy(_))
    ));
    choice.answer.notify_one();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let details = downloads.bt_task_files(&id).await.unwrap();
            let selected: Vec<_> = details
                .files
                .iter()
                .filter(|f| f.selected)
                .map(|f| f.index)
                .collect();
            if selected == [0] {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    downloads.shutdown().await;
    std::fs::remove_dir_all(root).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn bt_files_initial_empty_fallback_means_all() {
    let root = std::env::temp_dir().join(format!(
        "fluxdown-bt-initial-empty-{}",
        uuid::Uuid::new_v4()
    ));
    let source = root.join("source");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("a.bin"), vec![17u8; 65_536]).unwrap();
    std::fs::write(source.join("b.bin"), vec![39u8; 65_536]).unwrap();
    let torrent = create_torrent(
        &source,
        CreateTorrentOptions {
            name: Some("initial-empty"),
            trackers: vec![],
            piece_length: Some(16_384),
        },
        &librqbit::spawn_utils::BlockingSpawner::new(1),
    )
    .await
    .unwrap()
    .as_bytes()
    .unwrap();
    let db = Db::connect("sqlite::memory:").await.unwrap();
    db.seed_builtin_queues().await.unwrap();
    let choice = Arc::new(HeldInitialChoice {
        arrived: tokio::sync::Notify::new(),
        answer: tokio::sync::Notify::new(),
        indices: vec![],
    });
    choice.answer.notify_one();
    let mut downloads = manager_with_choice(db.clone(), &root, "invalid-tracker-url", choice);
    let id = downloads
        .create_task(NewTaskSpec {
            torrent_file_bytes: torrent.to_vec(),
            save_dir: root.join("save").to_string_lossy().into_owned(),
            ..Default::default()
        })
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if db.load_bt_selected_files(&id).await.unwrap() == Some(vec![]) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert!(
        downloads
            .bt_task_files(&id)
            .await
            .unwrap()
            .files
            .iter()
            .all(|f| f.selected)
    );
    downloads.shutdown().await;
    std::fs::remove_dir_all(root).unwrap();
}
