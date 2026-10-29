//! FluxMesh宿主装配：状态中枢与 agent 会话的接入、窗口入口（分享面板 / 接收确认 / 分享页 / 网络诊断）、
//! 链接触发的待确认接收自动弹窗，以及FluxMesh可用性对活动栏入口的联动。

use std::{rc::Rc, sync::Arc};

use fluxdown_ui_downloads::{DownloadHostActions, IdOpener};
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_settings::SettingsTarget;
use fluxdown_ui_share::{
    PanelTarget, ReceiveSource, ShareApi, ShareDiagnosticsView, ShareHost, ShareHub, ShareHubEvent,
};
use gpui::{App, AppContext as _, Entity};

use crate::{
    activity::{self, SHARE_ROUTE},
    agent_client::AgentClient,
    app::Desktop,
    capability_ports::AgentSharePort,
    session::{AgentSession, attach},
};

/// 创建FluxMesh状态中枢并接入会话：快照 / 事件进 hub，窗口入口注入，待确认接收自动弹接收窗口，
/// FluxMesh可用性变化时刷新活动栏入口。
pub(crate) fn install(
    translator: &Entity<Translator>,
    session: &Entity<AgentSession>,
    client: &Arc<AgentClient>,
    cx: &mut App,
) -> Entity<ShareHub> {
    let api = ShareApi::new(Arc::new(AgentSharePort::new(client.clone())));
    let hub = cx.new(|_| ShareHub::new(translator.clone(), api));
    hub.update(cx, |hub, _| hub.set_host(host()));
    attach(session, &hub, cx);

    cx.subscribe(&hub, |_, event: &ShareHubEvent, cx| match event {
        ShareHubEvent::PendingReceive { resolve_id } => {
            crate::windows::share_receive::open(cx, ReceiveSource::Pending(resolve_id.clone()));
        }
    })
    .detach();

    let mut available = hub.read(cx).available();
    cx.observe(&hub, move |hub, cx| {
        let now = hub.read(cx).available();
        if now != available {
            available = now;
            refresh_entry_visibility(cx);
        }
    })
    .detach();
    hub
}

fn host() -> ShareHost {
    ShareHost {
        open_panel: Some(Rc::new(|target, _, cx| open_panel(cx, target))),
        open_receive: Some(Rc::new(|input, _, cx| {
            crate::windows::share_receive::open(cx, ReceiveSource::Input(input));
        })),
        open_manage: Some(Rc::new(|_, cx| open_manage(cx))),
        open_diagnostics: Some(Rc::new(|session_id, _, cx| {
            open_diagnostics(cx, session_id);
        })),
        open_account: Some(Rc::new(|_, cx| crate::activity::open_account(cx))),
    }
}

/// 打开分享面板（下载页分享入口、命令面板与快捷键共用）。
pub(crate) fn open_panel(cx: &mut App, target: PanelTarget) {
    crate::windows::share::open(cx, target);
}

/// 主窗口到前台并切到FluxMesh分享页。
pub(crate) fn open_manage(cx: &mut App) {
    cx.defer(|cx| {
        crate::command_palette::navigate_now(cx, SHARE_ROUTE);
    });
}

/// 打开设置 › FluxMesh › 网络诊断（可聚焦某个会话）。
pub(crate) fn open_diagnostics(cx: &mut App, session_id: Option<String>) {
    cx.defer(move |cx| {
        let hub = Desktop::global(cx).share_hub.clone();
        hub.update(cx, |hub, cx| hub.request_diagnostics(session_id, cx));
        crate::windows::settings::reveal(
            cx,
            SettingsTarget {
                page: "share",
                tab: "network",
                row: None,
            },
        );
    });
}

/// 下载页「接收分享」入口：新建下载框里粘贴的分享链接交给接收确认窗口。
pub(crate) fn open_share_receive_opener() -> IdOpener {
    Rc::new(|link, _, cx| {
        crate::windows::share_receive::open(cx, ReceiveSource::Input(link));
    })
}

/// 把FluxMesh入口注入下载页的宿主动作（主窗口与独立任务窗口共用）：分享面板、任务行「切换桥接」、
/// 任务详情「传输诊断」页。
pub(crate) fn extend_download_actions(host: &mut DownloadHostActions, hub: &Entity<ShareHub>) {
    host.share_tasks = Some(Rc::new(|task_ids, _, cx| {
        open_panel(cx, PanelTarget::Tasks(task_ids));
    }));
    let bridge_hub = hub.clone();
    host.share_switch_to_bridge = Some(Rc::new(move |task_id, window, cx| {
        fluxdown_ui_share::switch_to_bridge(&bridge_hub, &task_id, window, cx);
    }));
    host.share_open_usage = Some(Rc::new(|_, _, cx| {
        crate::windows::settings::reveal(
            cx,
            SettingsTarget {
                page: "account",
                tab: "",
                row: None,
            },
        );
    }));
    let stop_hub = hub.clone();
    host.share_stop_providing = Some(Rc::new(move |record_id, window, cx| {
        fluxdown_ui_share::stop_providing(&stop_hub, &record_id, window, cx);
    }));
    let diagnostics_hub = hub.clone();
    host.share_diagnostics_view = Some(Rc::new(move |task_id, window, cx| {
        let hub = diagnostics_hub.clone();
        cx.new(|cx| ShareDiagnosticsView::new(hub, task_id, window, cx))
            .into()
    }));
    host.open_share_receive = Some(open_share_receive_opener());
}

/// FluxMesh可用性变化：按偏好 + 可用性重算活动栏入口。
fn refresh_entry_visibility(cx: &mut App) {
    let Some(shell) = Desktop::global(cx)
        .main_shell
        .as_ref()
        .and_then(gpui::WeakEntity::upgrade)
    else {
        return;
    };
    let preferences = Desktop::preferences(cx).clone();
    shell.update(cx, |shell, cx| {
        activity::apply_visibility(shell, &preferences, cx);
    });
}
