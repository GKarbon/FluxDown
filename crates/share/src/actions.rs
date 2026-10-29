//! 跨视图复用的FluxMesh动作：不属于某一个窗口，调用方只给 hub 与触发窗口。

use fluxdown_ui_i18n::Translator;
use gpui::{AnyWindowHandle, App, AsyncApp, Entity, Window};
use gpui_component::{WindowExt as _, notification::Notification};

use crate::{errors::error_text, hub::ShareHub};

/// 在触发窗口弹出一条错误通知；窗口已关闭则没有可展示的位置，直接放弃。
pub(crate) fn notify_error(window: AnyWindowHandle, cx: &mut AsyncApp, text: String) {
    let Ok(()) = window.update(cx, move |_, window: &mut Window, cx| {
        window.push_notification(Notification::error(text), cx);
    }) else {
        return;
    };
}

/// 在触发窗口弹出一条成功通知；窗口已关闭则放弃。
pub(crate) fn notify_success(window: AnyWindowHandle, cx: &mut AsyncApp, text: String) {
    let Ok(()) = window.update(cx, move |_, window: &mut Window, cx| {
        window.push_notification(Notification::success(text), cx);
    }) else {
        return;
    };
}

/// 任务行「切换桥接」：调用 `agent.share.switchToBridge`（任务继续运行，来源改走桥接 / 云端副本，
/// 之后任务的 `pathKind` 随 `ShareChanged` 变为 `bridge` / `cdn`）。失败（如当前没有可用桥接）时在
/// 触发窗口弹出带类型的错误文案。
pub fn switch_to_bridge(hub: &Entity<ShareHub>, task_id: &str, window: &Window, cx: &mut App) {
    let handle = window.window_handle();
    let (api, translator): (_, Translator) = {
        let hub = hub.read(cx);
        (hub.api(), hub.translator().read(cx).clone())
    };
    let future = api.switch_to_bridge(task_id);
    cx.spawn(async move |cx| {
        if let Err(error) = future.await {
            notify_error(handle, cx, error_text(&translator, &error));
        }
    })
    .detach();
}

/// 任务「停止共享」（任务详情「传输诊断」页 / 任务右键菜单）：调用 `agent.share.stopProviding`。行状态由随后的 `ShareChanged` 刷新；
/// 失败时在触发窗口弹出带类型的错误文案。
pub fn stop_providing(hub: &Entity<ShareHub>, record_id: &str, window: &Window, cx: &mut App) {
    let handle = window.window_handle();
    let (api, translator): (_, Translator) = {
        let hub = hub.read(cx);
        (hub.api(), hub.translator().read(cx).clone())
    };
    let future = api.stop_providing(record_id);
    cx.spawn(async move |cx| {
        if let Err(error) = future.await {
            notify_error(handle, cx, error_text(&translator, &error));
        }
    })
    .detach();
}

/// 把实时分享转为云端暂存：调用 `agent.share.convertToCloud`（`tier_id = None` 取云端默认档）。
/// agent 过完预检就返回，上传进度经 `ShareChanged` 回流；失败（游客 / 额度 / 云端不可用…）时在触发窗口
/// 弹出带类型的错误文案。
pub(crate) fn convert_to_cloud(
    hub: &Entity<ShareHub>,
    share_id: &str,
    tier_id: Option<&str>,
    window: &Window,
    cx: &mut App,
) {
    let handle = window.window_handle();
    let (api, translator): (_, Translator) = {
        let hub = hub.read(cx);
        (hub.api(), hub.translator().read(cx).clone())
    };
    let future = api.convert_to_cloud(share_id, tier_id);
    cx.spawn(async move |cx| {
        if let Err(error) = future.await {
            notify_error(handle, cx, error_text(&translator, &error));
        }
    })
    .detach();
}

/// 删除云端副本（上传进行中则取消上传）：调用 `agent.share.deleteCloudCopy`，失败时弹出错误文案。
pub(crate) fn delete_cloud_copy(
    hub: &Entity<ShareHub>,
    share_id: &str,
    window: &Window,
    cx: &mut App,
) {
    let handle = window.window_handle();
    let (api, translator): (_, Translator) = {
        let hub = hub.read(cx);
        (hub.api(), hub.translator().read(cx).clone())
    };
    let future = api.delete_cloud_copy(share_id);
    cx.spawn(async move |cx| {
        if let Err(error) = future.await {
            notify_error(handle, cx, error_text(&translator, &error));
        }
    })
    .detach();
}
