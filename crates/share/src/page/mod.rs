//! FluxMesh管理页（活动栏路由内容）：顶部取件码 / 链接输入，下方「我的分享 / 接收记录 / 网络诊断」三个页签。
//!
//! 状态只读 [`ShareHub`]；接收记录的翻页与网络诊断的取数只在对应页签实际渲染（可见）时发起。
//!
//! 按职责拆分：`toolbar`（取件输入与页签条）、`mine`（我的分享）、`received`（接收记录）、`table`（两张表共用的列宽与表头）。

mod mine;
mod received;
mod table;
mod toolbar;
mod usage;

use std::{collections::HashSet, time::Instant};

use fluxdown_protocol::ShareReceiveRecordDto;
use fluxdown_ui_components::FluxIcon;
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::active_theme;
use gpui::{
    AppContext as _, Context, Entity, IntoElement, ParentElement as _, Render, SharedString,
    Styled as _, Window, div,
};
use gpui_component::{
    input::{InputEvent, InputState},
    v_flex,
};

use crate::{
    hub::{ManageRequest, ShareHub},
    manage_ui::{empty_state, text},
    model::{ManageTab, records::ReceivedFeed, refresh::VisibleRefresh},
    network::NetworkTab,
};
use usage::UsageTab;

/// 接收记录首页两次取数之间的最短间隔（毫秒）。
const RECORDS_MIN_REFRESH_MS: u64 = 1_000;

/// FluxMesh管理页（活动栏路由内容）：取件码 / 链接输入 + 我的分享 / 接收记录 / 网络诊断三个页签。
pub struct ShareView {
    hub: Entity<ShareHub>,
    translator: Entity<Translator>,
    input: Entity<InputState>,
    placeholder: SharedString,
    input_invalid: bool,
    tab: ManageTab,
    network: Entity<NetworkTab>,
    usage: Entity<UsageTab>,
    /// 正在提交操作（停止 / 开始 / 删除）的分享 ID，防重入。
    busy: HashSet<String>,
    /// 「我的分享」多选集合（分享 ID）；分享从列表消失时在 `absorb_hub` 里剔除。
    selected: HashSet<String>,
    // 接收记录
    feed: ReceivedFeed,
    last_records: Vec<ShareReceiveRecordDto>,
    epoch: Instant,
    records_refresh: VisibleRefresh,
    records_request: u64,
    records_failed: bool,
    loading_more: bool,
}

impl ShareView {
    /// 创建管理页：`hub` 是跨窗口共享的FluxMesh状态；输入框与列表只在本视图内持有 UI 状态。
    pub fn new(hub: Entity<ShareHub>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let translator = hub.read(cx).translator().clone();
        let placeholder = text(translator.read(cx), "sharePickupPlaceholder");
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder.clone()));
        cx.subscribe_in(
            &input,
            window,
            |this, _, event: &InputEvent, window, cx| match event {
                InputEvent::PressEnter { .. } => this.submit_receive(window, cx),
                InputEvent::Change if this.input_invalid => {
                    this.input_invalid = false;
                    cx.notify();
                }
                _ => {}
            },
        )
        .detach();
        cx.observe(&hub, |this, _, cx| this.absorb_hub(cx)).detach();
        cx.observe(&translator, |_, _, cx| cx.notify()).detach();
        let network = cx.new(|cx| NetworkTab::new(hub.clone(), window, cx));
        let usage = UsageTab::create(hub.clone(), cx);

        let mut view = Self {
            hub,
            translator,
            input,
            placeholder,
            input_invalid: false,
            tab: ManageTab::Mine,
            network,
            usage,
            busy: HashSet::new(),
            selected: HashSet::new(),
            feed: ReceivedFeed::default(),
            last_records: Vec::new(),
            epoch: Instant::now(),
            records_refresh: VisibleRefresh::new(RECORDS_MIN_REFRESH_MS),
            records_request: 0,
            records_failed: false,
            loading_more: false,
        };
        // 视图创建前就已挂起的定位请求（app 先请求再创建页面）。
        view.absorb_hub(cx);
        view
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// hub 变化：消费定位请求；快照里的接收记录变了则刷新首页。
    fn absorb_hub(&mut self, cx: &mut Context<Self>) {
        let request = self.hub.update(cx, |hub, _| hub.take_manage_request());
        if let Some(request) = request {
            self.apply_manage_request(request, cx);
        }
        if !self.selected.is_empty() {
            let state = self.hub.read(cx).state();
            self.selected
                .retain(|id| state.shares.iter().any(|share| &share.id == id));
        }
        let changed = self.hub.read(cx).state().receive_records != self.last_records;
        if changed {
            self.last_records = self.hub.read(cx).state().receive_records.clone();
            self.records_refresh.mark_dirty();
        }
        cx.notify();
    }

    fn apply_manage_request(&mut self, request: ManageRequest, cx: &mut Context<Self>) {
        self.select_tab(request.tab, cx);
        if let Some(session_id) = request.session_id {
            self.network
                .update(cx, |network, cx| network.focus_session(&session_id, cx));
        }
    }

    fn select_tab(&mut self, tab: ManageTab, cx: &mut Context<Self>) {
        if self.tab == tab {
            return;
        }
        self.tab = tab;
        if tab == ManageTab::Received {
            self.records_refresh.mark_dirty();
        }
        cx.notify();
    }
}

// ───────────────────────── 渲染 ─────────────────────────

impl Render for ShareView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = active_theme(cx);
        let tokens = theme.tokens().clone();
        let translator = self.translator.read(cx).clone();

        let root = v_flex().size_full().min_h_0();
        if !self.hub.read(cx).available() {
            return root.child(empty_state(
                FluxIcon::Share2,
                None,
                text(&translator, "shareNavTitle"),
                Some(text(&translator, "errReasonShareUnavailable")),
                cx,
            ));
        }

        // 语言切换后同步输入框 placeholder。
        let placeholder = text(&translator, "sharePickupPlaceholder");
        if placeholder != self.placeholder {
            self.placeholder = placeholder.clone();
            self.input.update(cx, |input, cx| {
                input.set_placeholder(placeholder, window, cx)
            });
        }

        let content = match self.tab {
            ManageTab::Mine => self.render_mine(&translator, cx).into_any_element(),
            ManageTab::Received => {
                self.refresh_records_if_needed(cx);
                self.render_received(&translator, cx).into_any_element()
            }
            ManageTab::Usage => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(self.usage.clone())
                .into_any_element(),
            ManageTab::Network => div()
                .flex()
                .flex_col()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(self.network.clone())
                .into_any_element(),
        };

        root.px(tokens.spacing.lg)
            .pt(tokens.spacing.md)
            .gap(tokens.spacing.md)
            .child(self.render_top_bar(&translator, cx))
            .child(self.render_tabs(&translator, cx))
            .child(content)
    }
}
