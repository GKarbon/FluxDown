use fluxdown_engine::bt_config::BtConfig;
use fluxdown_engine::db::Db;
use fluxdown_engine::download_manager::{DownloadManager, DownloadManagerConfig, TaskDone};
use fluxdown_engine::proxy_config::ProxyConfig;
use fluxdown_engine::{NoopSelection, NoopSink};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

pub(super) fn manager(db: Db, root: &Path, tracker: &str) -> DownloadManager {
    manager_with_choice(db, root, tracker, Arc::new(NoopSelection))
}

pub(super) fn manager_with_choice(
    db: Db,
    root: &Path,
    tracker: &str,
    choice: Arc<dyn fluxdown_engine::selection::HostSelection>,
) -> DownloadManager {
    DownloadManager::new(
        db,
        DownloadManagerConfig {
            max_concurrent: 2,
            speed_limit_bps: 0,
            upload_limit_bps: 0,
            default_save_dir: root.join("save").to_string_lossy().into_owned(),
            app_data_dir: root.join("data").to_string_lossy().into_owned(),
            data_dir: root.join("data"),
            bt_config: BtConfig {
                enable_dht: false,
                enable_upnp: false,
                enable_utp: false,
                custom_trackers: tracker.into(),
                ..Default::default()
            },
            proxy_config: ProxyConfig::default(),
            user_agent: String::new(),
        },
        Arc::new(NoopSink),
        choice,
    )
    .unwrap()
}

pub(super) async fn settle(manager: &mut DownloadManager, done: &mut mpsc::Receiver<TaskDone>) {
    while let Ok(event) = done.try_recv() {
        manager.on_task_done(&event).await;
    }
}

pub(super) async fn wait_status(
    manager: &mut DownloadManager,
    db: &Db,
    done: &mut mpsc::Receiver<TaskDone>,
    id: &str,
    status: i32,
) {
    tokio::time::timeout(Duration::from_secs(40), async {
        loop {
            settle(manager, done).await;
            let task = db.load_task_by_id(id).await.unwrap().unwrap();
            assert_ne!(task.status, 4, "download failed: {}", task.error_message);
            if task.status == status {
                settle(manager, done).await;
                return;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("task state deadline");
}

pub(super) struct HeldInitialChoice {
    pub(super) arrived: tokio::sync::Notify,
    pub(super) answer: tokio::sync::Notify,
    pub(super) indices: Vec<i32>,
}

#[async_trait::async_trait]
impl fluxdown_engine::selection::HostSelection for HeldInitialChoice {
    async fn select_hls_quality(
        &self,
        _: &str,
        _: &[fluxdown_engine::model::HlsQualityOption],
        _: Duration,
    ) -> fluxdown_engine::selection::SelectionOutcome<i32> {
        panic!("BT fixture")
    }
    async fn select_bt_files(
        &self,
        _: &str,
        _: &[fluxdown_engine::model::BtFileEntry],
        _: Option<Duration>,
    ) -> fluxdown_engine::selection::SelectionOutcome<Vec<i32>> {
        self.arrived.notify_one();
        self.answer.notified().await;
        fluxdown_engine::selection::SelectionOutcome::UserChose(self.indices.clone())
    }
    async fn select_resolve_variant(
        &self,
        _: &str,
        _: &[fluxdown_engine::model::ResolveVariantOption],
        _: i32,
        _: Duration,
    ) -> fluxdown_engine::selection::SelectionOutcome<i32> {
        panic!("BT fixture")
    }
    fn provide_hls_selection(&self, _: &str, _: i32) {
        panic!("BT fixture")
    }
    fn provide_bt_selection(&self, _: &str, _: Vec<i32>) {
        self.answer.notify_one();
    }
    fn provide_variant_selection(&self, _: &str, _: i32) {
        panic!("BT fixture")
    }
}

pub(super) struct Swarm {
    pub(super) root: std::path::PathBuf,
    pub(super) content: std::path::PathBuf,
    pub(super) torrent: bytes::Bytes,
    pub(super) seeder: Arc<librqbit::Session>,
    pub(super) tracker_url: String,
    pub(super) tracker_task: tokio::task::JoinHandle<()>,
}

pub(super) async fn swarm() -> Swarm {
    use librqbit::{
        AddTorrent, AddTorrentResponse, CreateTorrentOptions, Session, SessionOptions,
        create_torrent,
    };
    use std::num::NonZeroU32;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    let root = std::env::temp_dir().join(format!("fluxdown-bt-files-{}", uuid::Uuid::new_v4()));
    let content = root.join("source");
    std::fs::create_dir_all(content.join("nested")).unwrap();
    for (name, len, seed) in [
        ("a.bin", 250_001, 17u8),
        ("nested/b.bin", 280_003, 39),
        ("c.bin", 310_007, 61),
    ] {
        std::fs::write(
            content.join(name),
            (0..len)
                .map(|n| (n as u8).wrapping_mul(31).wrapping_add(seed))
                .collect::<Vec<_>>(),
        )
        .unwrap();
    }
    std::fs::write(content.join("empty.bin"), []).unwrap();
    let torrent = create_torrent(
        &content,
        CreateTorrentOptions {
            name: Some("file-test"),
            trackers: vec![],
            piece_length: Some(16_384),
        },
        &librqbit::spawn_utils::BlockingSpawner::new(1),
    )
    .await
    .unwrap()
    .as_bytes()
    .unwrap();
    let seeder = Session::new_with_opts(
        content.clone(),
        SessionOptions {
            dht: None,
            disable_trackers: true,
            listen: Some(librqbit::ListenerOptions {
                mode: librqbit::ListenerMode::TcpOnly,
                listen_addr: "127.0.0.1:0".parse().unwrap(),
                enable_upnp_port_forwarding: false,
                ipv4_only: true,
                ..Default::default()
            }),
            ratelimits: librqbit::limits::LimitsConfig {
                upload_bps: NonZeroU32::new(128 * 1024),
                download_bps: None,
            },
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let added = seeder
        .add_torrent(
            AddTorrent::from_bytes(torrent.clone()),
            Some(librqbit::AddTorrentOptions {
                output_folder: Some(content.to_string_lossy().into_owned()),
                overwrite: true,
                ..Default::default()
            }),
        )
        .await
        .unwrap();
    let seed_handle = match added {
        AddTorrentResponse::Added(_, h) => h,
        _ => panic!("expected seeder"),
    };
    seed_handle.wait_until_initialized().await.unwrap();
    assert!(seed_handle.stats().finished);
    let seed_addr = seeder.listen_addr().unwrap();
    let tracker = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tracker_url = format!("http://{}/announce", tracker.local_addr().unwrap());
    let tracker_task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = tracker.accept().await.unwrap();
            let mut request = [0u8; 8192];
            match socket.read(&mut request).await {
                Ok(0) => continue,
                Ok(_) => {}
                Err(error) => {
                    eprintln!("local tracker client disconnected: {error}");
                    continue;
                }
            }
            let mut body = b"d8:intervali1e5:peers6:".to_vec();
            body.extend_from_slice(&[127, 0, 0, 1]);
            body.extend_from_slice(&seed_addr.port().to_be_bytes());
            body.push(b'e');
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            if let Err(error) = socket.write_all(header.as_bytes()).await {
                eprintln!("local tracker header connection closed: {error}");
                continue;
            }
            if let Err(error) = socket.write_all(&body).await {
                eprintln!("local tracker body connection closed: {error}");
            }
        }
    });
    Swarm {
        root,
        content,
        torrent,
        seeder,
        tracker_url,
        tracker_task,
    }
}
