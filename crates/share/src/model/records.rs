//! 管理页的纯状态：页签索引映射与「接收记录」的分页合并。

use std::collections::HashSet;

use fluxdown_protocol::{
    ShareReceiveRecordDto, ShareReceiveRecordsPage, ShareReceiveRecordsParams,
};

use super::target::ManageTab;

/// 管理页页签的显示顺序。
pub const MANAGE_TABS: [ManageTab; 4] = [
    ManageTab::Mine,
    ManageTab::Received,
    ManageTab::Usage,
    ManageTab::Network,
];

/// 页签在分段标签里的下标。
#[must_use]
pub fn tab_index(tab: ManageTab) -> usize {
    MANAGE_TABS
        .iter()
        .position(|candidate| *candidate == tab)
        .unwrap_or(0)
}

/// 分段标签下标 → 页签；越界回到第一个页签。
#[must_use]
pub fn tab_from_index(index: usize) -> ManageTab {
    MANAGE_TABS.get(index).copied().unwrap_or_default()
}

/// 页签文案键。
#[must_use]
pub const fn tab_key(tab: ManageTab) -> &'static str {
    match tab {
        ManageTab::Mine => "shareTabMine",
        ManageTab::Received => "shareTabReceived",
        ManageTab::Usage => "shareTabUsage",
        ManageTab::Network => "shareTabNetwork",
    }
}

/// 每页向 agent 取多少条接收记录（与快照里保留的条数一致）。
pub const RECORDS_PAGE: u32 = 20;

/// 接收记录的列表：快照里的最近记录在前（实时更新），其后是经 `agent.share.receiveRecords` 翻页取得的
/// 记录。快照给不出 `nextCursor`，所以页面打开时先取一次首页拿到游标，之后「加载更多」沿游标向前翻。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ReceivedFeed {
    first: Vec<ShareReceiveRecordDto>,
    older: Vec<ShareReceiveRecordDto>,
    cursor: Option<String>,
}

impl ReceivedFeed {
    /// 首页请求参数。
    #[must_use]
    pub fn first_params() -> ShareReceiveRecordsParams {
        ShareReceiveRecordsParams {
            limit: Some(RECORDS_PAGE),
            cursor: None,
        }
    }

    /// 「加载更多」的请求参数；没有更早的记录为 `None`。
    #[must_use]
    pub fn more_params(&self) -> Option<ShareReceiveRecordsParams> {
        self.cursor
            .as_ref()
            .map(|cursor| ShareReceiveRecordsParams {
                limit: Some(RECORDS_PAGE),
                cursor: Some(cursor.clone()),
            })
    }

    /// 是否还有更早的记录可加载。
    #[must_use]
    pub fn has_more(&self) -> bool {
        self.cursor.is_some()
    }

    /// 应用（重新取得的）首页：替换首页内容；还没翻过页时游标取首页给出的，翻过页则保留已翻到的游标。
    pub fn apply_first_page(&mut self, page: ShareReceiveRecordsPage) {
        self.first = page.records;
        if self.older.is_empty() {
            self.cursor = page.next_cursor;
        }
    }

    /// 追加一页更早的记录。
    pub fn apply_more_page(&mut self, page: ShareReceiveRecordsPage) {
        self.older.extend(page.records);
        self.cursor = page.next_cursor;
    }

    /// 要显示的全部记录：快照在前，其后首页与翻页取得的记录，按 `id` 去重（先出现者优先）。
    #[must_use]
    pub fn visible<'a>(
        &'a self,
        snapshot: &'a [ShareReceiveRecordDto],
    ) -> Vec<&'a ShareReceiveRecordDto> {
        let mut seen: HashSet<&str> = HashSet::new();
        snapshot
            .iter()
            .chain(&self.first)
            .chain(&self.older)
            .filter(|record| seen.insert(record.id.as_str()))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use fluxdown_protocol::{ShareReceiveRecordDto, ShareReceiveRecordsPage};

    use super::{MANAGE_TABS, RECORDS_PAGE, ReceivedFeed, tab_from_index, tab_index, tab_key};
    use crate::model::ManageTab;

    fn record(id: &str) -> ShareReceiveRecordDto {
        ShareReceiveRecordDto {
            id: id.into(),
            title: id.into(),
            ..ShareReceiveRecordDto::default()
        }
    }

    fn page(ids: &[&str], cursor: Option<&str>) -> ShareReceiveRecordsPage {
        ShareReceiveRecordsPage {
            records: ids.iter().map(|id| record(id)).collect(),
            next_cursor: cursor.map(str::to_owned),
        }
    }

    fn ids(feed: &ReceivedFeed, snapshot: &[ShareReceiveRecordDto]) -> Vec<String> {
        feed.visible(snapshot)
            .iter()
            .map(|record| record.id.clone())
            .collect()
    }

    #[test]
    fn tabs_round_trip_through_their_index() {
        for tab in MANAGE_TABS {
            assert_eq!(tab_from_index(tab_index(tab)), tab);
        }
        assert_eq!(tab_from_index(99), ManageTab::Mine);
        assert_eq!(tab_key(ManageTab::Network), "shareTabNetwork");
    }

    #[test]
    fn snapshot_records_come_first_and_pages_extend_without_duplicates() {
        let mut feed = ReceivedFeed::default();
        let snapshot = vec![record("c"), record("b"), record("a")];
        assert_eq!(ids(&feed, &snapshot), ["c", "b", "a"]);
        assert!(!feed.has_more());
        assert!(feed.more_params().is_none());

        feed.apply_first_page(page(&["c", "b", "a"], Some("cur-1")));
        assert_eq!(ids(&feed, &snapshot), ["c", "b", "a"]);
        let more = feed.more_params();
        assert_eq!(
            more.as_ref().and_then(|params| params.cursor.as_deref()),
            Some("cur-1")
        );
        assert_eq!(more.and_then(|params| params.limit), Some(RECORDS_PAGE));

        feed.apply_more_page(page(&["a", "z", "y"], None));
        assert_eq!(ids(&feed, &snapshot), ["c", "b", "a", "z", "y"]);
        assert!(!feed.has_more());
    }

    #[test]
    fn refreshing_the_first_page_keeps_the_older_cursor_once_paged() {
        let mut feed = ReceivedFeed::default();
        feed.apply_first_page(page(&["c", "b"], Some("cur-1")));
        feed.apply_more_page(page(&["a"], Some("cur-2")));
        // 新记录 d 到来：首页刷新，但已翻到的游标不被首页游标覆盖。
        feed.apply_first_page(page(&["d", "c"], Some("cur-1")));
        assert_eq!(
            feed.more_params().and_then(|params| params.cursor),
            Some("cur-2".to_owned())
        );
        assert_eq!(ids(&feed, &[]), ["d", "c", "a"]);
    }
}
