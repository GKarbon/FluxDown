//! FluxMesh（临时文件分享）capability：分享面板、接收确认、管理页、网络诊断与「FluxMesh」设置页。
//!
//! 状态只有一份：[`ShareHub`] 投影 `AgentSnapshot.share`（跨窗口共享实体，由 app 把会话快照 / 事件
//! 喂进来）；视图观察它、经 [`ShareApi`] 调 `agent.share.*`。窗口由 app 创建，本 crate 通过
//! [`ShareHost`] 回调请求打开，不依赖 downloads / settings 等其它 capability。

mod actions;
mod appeal;
mod cloud_ui;
mod errors;
mod exit_ui;
mod host;
mod hub;
mod manage_ui;
pub mod model;
mod nested_scroll;
mod network;
mod page;
mod panel;
mod port;
mod qr;
mod receive;
mod report;
mod session_view;
mod settings_view;
mod task_diagnostics;
mod window_ui;

pub use actions::switch_to_bridge;
pub use errors::{error_key, error_text};
pub use exit_ui::{ExitContinuation, needs_exit_gate, run_exit_gate};
pub use host::{AccountOpener, ManageOpener, PanelOpener, ReceiveOpener, ShareHost};
pub use hub::{ManageRequest, ShareHub, ShareHubEvent};
pub use model::{ManageTab, PanelTarget, panel_key};
pub use page::ShareView;
pub use panel::SharePanelView;
pub use port::{PortFuture, ShareApi, SharePort};
pub use receive::{ReceiveSource, ShareReceiveView};
pub use settings_view::ShareSettingsView;
pub use task_diagnostics::ShareDiagnosticsView;
