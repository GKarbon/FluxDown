//! 「接收记录」页签：快照记录 + 分页历史。

use std::ops::Range;

use fluxdown_protocol::{
    RpcErrorData, ShareProvideState, ShareReceiveRecordDto, ShareReceiveRecordsPage,
    ShareReceiveStatus,
};
use fluxdown_ui_components::{
    ButtonVariant, ControlExt as _, FluxIcon, icon_button, loading_button, tabular_numbers,
};
use fluxdown_ui_i18n::Translator;
use fluxdown_ui_theme::active_theme;
use gpui::{
    App, Context, Div, ElementId, Hsla, InteractiveElement as _, IntoElement, ParentElement as _,
    Pixels, SharedString, StatefulInteractiveElement as _, Styled as _, Window, div,
    prelude::FluentBuilder as _, uniform_list,
};
use gpui_component::{Icon, button::Button, h_flex, v_flex};

use super::{
    ShareView,
    table::{columns, or_placeholder, row_height},
};
use crate::{
    actions::notify_error,
    errors::error_text,
    manage_ui::{body_text, chip, clipped, empty_state, text, text_with, tooltip_builder},
    model::{
        format_bytes, format_datetime, provide_state_key, receive_status_key,
        records::ReceivedFeed, refresh::GateAction,
    },
    report::open_report_dialog,
};

pub(super) fn receive_status_tone(status: ShareReceiveStatus, cx: &App) -> Hsla {
    let colors = active_theme(cx).extended().colors;
    match status {
        ShareReceiveStatus::Active => colors.status_downloading,
        ShareReceiveStatus::Completed => colors.status_completed,
        ShareReceiveStatus::Failed => colors.status_failed,
    }
}

impl ShareView {
    /// 渲染「接收记录」页签时调用：首页按需（打开 / 记录变化 / 隐藏后再显示）经节流刷新。
    pub(super) fn refresh_records_if_needed(&mut self, cx: &mut Context<Self>) {
        match self.records_refresh.on_render(self.now_ms()) {
            Some(GateAction::Fetch) => self.fetch_first_page(cx),
            Some(GateAction::Wait(delay)) => {
                cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(delay).await;
                    let Ok(()) = this.update(cx, |this, cx| {
                        this.records_refresh.timer_fired();
                        cx.notify();
                    }) else {
                        return;
                    };
                })
                .detach();
            }
            Some(GateAction::Skip) | None => {}
        }
    }

    pub(super) fn fetch_first_page(&mut self, cx: &mut Context<Self>) {
        self.records_request += 1;
        let request = self.records_request;
        let future = self
            .hub
            .read(cx)
            .api()
            .receive_records(&ReceivedFeed::first_params());
        cx.spawn(async move |this, cx| {
            let result = future.await;
            let Ok(()) = this.update(cx, |this, cx| this.finish_first_page(request, result, cx))
            else {
                return;
            };
        })
        .detach();
    }

    pub(super) fn finish_first_page(
        &mut self,
        request: u64,
        result: Result<ShareReceiveRecordsPage, RpcErrorData>,
        cx: &mut Context<Self>,
    ) {
        if request != self.records_request {
            return;
        }
        match result {
            Ok(page) => {
                self.feed.apply_first_page(page);
                self.records_failed = false;
            }
            Err(_) => self.records_failed = true,
        }
        cx.notify();
    }

    pub(super) fn load_more(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.loading_more {
            return;
        }
        let Some(params) = self.feed.more_params() else {
            return;
        };
        self.loading_more = true;
        let handle = window.window_handle();
        let translator = self.translator.read(cx).clone();
        let future = self.hub.read(cx).api().receive_records(&params);
        cx.spawn(async move |this, cx| {
            let result = future.await;
            let Ok(()) = this.update(cx, |this, cx| {
                this.loading_more = false;
                if let Ok(page) = &result {
                    this.feed.apply_more_page(page.clone());
                }
                cx.notify();
            }) else {
                return;
            };
            if let Err(error) = result {
                notify_error(handle, cx, error_text(&translator, &error));
            }
        })
        .detach();
        cx.notify();
    }

    pub(super) fn render_received(&self, translator: &Translator, cx: &mut Context<Self>) -> Div {
        let tokens = active_theme(cx).tokens().clone();
        let count = self
            .feed
            .visible(&self.hub.read(cx).state().receive_records)
            .len();
        let container = v_flex().flex_1().min_h_0().w_full();
        if count == 0 {
            let empty = if self.records_failed {
                empty_state(
                    FluxIcon::CircleAlert,
                    Some(tokens.colors.destructive),
                    text(translator, "shareRecordsLoadFailed"),
                    None,
                    cx,
                )
                .child(
                    Button::new("share-records-retry")
                        .outline()
                        .label(text(translator, "shareRetry"))
                        .control(cx)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.records_refresh.mark_dirty();
                            cx.notify();
                        })),
                )
            } else {
                empty_state(
                    FluxIcon::Download,
                    None,
                    text(translator, "shareReceivedEmpty"),
                    None,
                    cx,
                )
            };
            return container.child(empty);
        }
        let cols = columns(cx);
        let stale = self.hub.read(cx).is_stale();
        container
            .child(self.header_row(
                vec![
                    (text(translator, "shareColFile"), None),
                    (text(translator, "shareColSender"), Some(cols.sender)),
                    (text(translator, "shareColFiles"), Some(cols.files)),
                    (text(translator, "shareColSize"), Some(cols.size)),
                    (text(translator, "shareColStarted"), Some(cols.started)),
                    (text(translator, "shareColStatus"), Some(cols.status)),
                    (text(translator, "shareColProvide"), Some(cols.provide)),
                ],
                tokens.spacing.xxs,
                None,
                cx,
            ))
            .child(
                uniform_list(
                    "share-received",
                    count,
                    cx.processor(move |this, range: Range<usize>, _, cx| {
                        let rows: Vec<ShareReceiveRecordDto> = {
                            let snapshot = &this.hub.read(cx).state().receive_records;
                            this.feed
                                .visible(snapshot)
                                .into_iter()
                                .skip(range.start)
                                .take(range.len())
                                .cloned()
                                .collect()
                        };
                        let translator = this.translator.read(cx).clone();
                        rows.into_iter()
                            .enumerate()
                            .map(|(offset, record)| {
                                this.render_record_row(
                                    range.start + offset,
                                    record,
                                    &translator,
                                    cx,
                                )
                            })
                            .collect::<Vec<_>>()
                    }),
                )
                .flex_1()
                .min_h_0()
                .w_full(),
            )
            .when(self.feed.has_more(), |this| {
                this.child(
                    h_flex()
                        .w_full()
                        .flex_none()
                        .justify_center()
                        .py(tokens.spacing.sm)
                        .child(
                            loading_button(
                                "share-load-more",
                                text(translator, "shareLoadMore"),
                                ButtonVariant::Secondary,
                                self.loading_more,
                                cx,
                            )
                            .disabled(stale || self.loading_more)
                            .on_click(cx.listener(
                                |this, _, window, cx| {
                                    this.load_more(window, cx);
                                },
                            )),
                        ),
                )
            })
    }

    pub(super) fn render_record_row(
        &self,
        index: usize,
        record: ShareReceiveRecordDto,
        translator: &Translator,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = active_theme(cx);
        let tokens = theme.tokens().clone();
        let cols = columns(cx);
        let row_id = |part: &str| {
            ElementId::NamedInteger(
                SharedString::from(format!("share-received-{part}")),
                index as u64,
            )
        };
        let sender = if record.sender_device_name.trim().is_empty() {
            text(translator, "shareSenderUnknown")
        } else {
            SharedString::from(record.sender_device_name.clone())
        };
        let files = text_with(
            translator,
            "shareFilesCount",
            &[("count", &record.file_count.to_string())],
        );
        let cell = |width: Pixels, value: SharedString, id: ElementId| {
            body_text(cx)
                .w(width)
                .flex_none()
                .font_features(tabular_numbers())
                .child(clipped(id, value))
        };
        let status = chip(
            row_id("status"),
            text(translator, receive_status_key(record.status)),
            Some(receive_status_tone(record.status, cx)),
            cx,
        );
        let provide_chip = provide_state_key(record.provide_state).map(|key| {
            let tone = (record.provide_state == ShareProvideState::Providing)
                .then(|| active_theme(cx).extended().colors.status_completed);
            chip(row_id("provide"), text(translator, key), tone, cx)
        });
        let stop_button = (record.provide_state == ShareProvideState::Providing).then(|| {
            let label = text(translator, "shareStopProviding");
            let icon_size = active_theme(cx).extended().icon.md;
            let hub = self.hub.clone();
            let record_id = record.id.clone();
            icon_button(
                row_id("stop-provide"),
                label.clone(),
                Icon::new(FluxIcon::X).size(icon_size),
                ButtonVariant::Ghost,
                cx,
            )
            .tooltip(tooltip_builder(label))
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                crate::actions::stop_providing(&hub, &record_id, window, cx);
            })
        });
        let report_button = record.share_id.clone().map(|share_id| {
            let label = text(translator, "shareReport");
            let icon_size = active_theme(cx).extended().icon.md;
            let hub = self.hub.clone();
            icon_button(
                row_id("report"),
                label.clone(),
                Icon::new(FluxIcon::Flag)
                    .size(icon_size)
                    .text_color(active_theme(cx).extended().colors.text_tertiary),
                ButtonVariant::Ghost,
                cx,
            )
            .tooltip(tooltip_builder(label))
            .on_click(move |_, window, cx| {
                cx.stop_propagation();
                open_report_dialog(&hub, share_id.clone(), window, cx);
            })
        });
        h_flex()
            .w_full()
            .h(row_height(cx))
            .flex_none()
            .items_center()
            .gap(tokens.spacing.md)
            .px(tokens.spacing.sm)
            .rounded(tokens.radius.md)
            .hover(move |style| style.bg(theme.extended().colors.row_hover))
            .id(row_id("row"))
            .child(v_flex().flex_1().min_w_0().child(
                body_text(cx).child(clipped(row_id("title"), or_placeholder(&record.title))),
            ))
            .child(cell(cols.sender, sender, row_id("sender")))
            .child(cell(cols.files, files, row_id("files")))
            .child(cell(
                cols.size,
                SharedString::from(format_bytes(record.total_bytes)),
                row_id("size"),
            ))
            .child(cell(
                cols.started,
                SharedString::from(or_placeholder(&format_datetime(&record.started_at))),
                row_id("started"),
            ))
            .child(h_flex().w(cols.status).flex_none().min_w_0().child(status))
            .child(
                h_flex()
                    .w(cols.provide)
                    .flex_none()
                    .min_w_0()
                    .gap(tokens.spacing.xxs)
                    .children(provide_chip)
                    .children(stop_button)
                    .children(report_button),
            )
            .child(div().w(tokens.spacing.xxs).flex_none())
    }
}
