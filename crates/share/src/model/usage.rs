//! 「额度与贡献」页签的纯模型：封顶格式化、额度条、来源文案键、等级与加成标签。

use fluxdown_protocol::{DISPLAY_BYTES_CAP, ShareGrantSource, ShareTrafficKind, ShareUsageDto};

use super::format::format_bytes;

/// 额度字节的展示：达到云端封顶值显示 `≥ 1 PB`，其余同 [`format_bytes`]。
#[must_use]
pub fn format_quota_bytes(bytes: u64) -> String {
    if bytes >= DISPLAY_BYTES_CAP {
        "≥ 1 PB".to_owned()
    } else {
        format_bytes(bytes)
    }
}

/// 一条额度条的视图数据。
#[derive(Clone, Debug, PartialEq)]
pub struct MeterView {
    /// 流量种类名文案键。
    pub label_key: &'static str,
    pub remaining: u64,
    pub allowance: u64,
    pub used: u64,
    pub unlimited: bool,
    pub exhausted: bool,
    /// 已用占比 `0.0..=1.0`（不限时恒为 0）。
    pub ratio: f32,
}

/// 流量种类名文案键；未知种类没有名字（调用方跳过）。
#[must_use]
pub const fn kind_key(kind: ShareTrafficKind) -> Option<&'static str> {
    match kind {
        ShareTrafficKind::Swarm => Some("shareUsageKindSwarm"),
        ShareTrafficKind::Download => Some("shareUsageKindDownload"),
        ShareTrafficKind::Upload => Some("shareUsageKindUpload"),
        ShareTrafficKind::Unknown => None,
    }
}

/// 三种流量的额度条（保持云端给出的顺序，忽略未知种类）。
#[must_use]
pub fn meters(usage: &ShareUsageDto) -> Vec<MeterView> {
    usage
        .kinds
        .iter()
        .filter_map(|item| {
            let label_key = kind_key(item.kind)?;
            let exhausted = !item.unlimited && item.remaining == 0;
            let ratio = if item.unlimited {
                0.
            } else if exhausted {
                1.
            } else if item.monthly_allowance == 0 {
                0.
            } else {
                (item.used_this_month as f64 / item.monthly_allowance as f64).clamp(0., 1.) as f32
            };
            Some(MeterView {
                label_key,
                remaining: item.remaining,
                allowance: item.monthly_allowance,
                used: item.used_this_month,
                unlimited: item.unlimited,
                exhausted,
                ratio,
            })
        })
        .collect()
}

/// 额度来源文案键；系统赠送（后台 / free）与未知来源共用 `shareUsageGrantGranted`。
#[must_use]
pub const fn grant_source_key(source: ShareGrantSource) -> &'static str {
    match source {
        ShareGrantSource::Pack => "shareUsageGrantPack",
        ShareGrantSource::Rollover => "shareUsageGrantRollover",
        ShareGrantSource::Contribution => "shareUsageGrantContribution",
        ShareGrantSource::Admin | ShareGrantSource::Free | ShareGrantSource::Unknown => {
            "shareUsageGrantGranted"
        }
    }
}

/// `Lv.N`；`max > 0` 时等级不超过上限。
#[must_use]
pub fn level_label(level: u32, max: u32) -> String {
    let shown = if max > 0 { level.min(max) } else { level };
    format!("Lv.{shown}")
}

/// 贡献加成标签：×1（1000‰）不显示；否则 `×1.5`、`×2`（最多两位小数，去尾零）。
#[must_use]
pub fn boost_label(permille: u32) -> Option<String> {
    if permille == 1000 {
        return None;
    }
    let hundredths = (u64::from(permille) + 5) / 10;
    let whole = hundredths / 100;
    let fraction = hundredths % 100;
    let text = if fraction == 0 {
        format!("×{whole}")
    } else if fraction % 10 == 0 {
        format!("×{whole}.{}", fraction / 10)
    } else {
        format!("×{whole}.{fraction:02}")
    };
    Some(text)
}

#[cfg(test)]
mod tests {
    use fluxdown_protocol::{
        DISPLAY_BYTES_CAP, ShareGrantSource, ShareKindUsageDto, ShareTrafficKind, ShareUsageDto,
    };

    use super::{boost_label, format_quota_bytes, grant_source_key, level_label, meters};

    fn kind(
        kind: ShareTrafficKind,
        unlimited: bool,
        allowance: u64,
        used: u64,
        remaining: u64,
    ) -> ShareKindUsageDto {
        ShareKindUsageDto {
            kind,
            unlimited,
            monthly_allowance: allowance,
            used_this_month: used,
            remaining,
            reserved: 0,
        }
    }

    #[test]
    fn quota_bytes_cap_at_one_petabyte() {
        assert_eq!(format_quota_bytes(0), "0 B");
        assert_eq!(format_quota_bytes(3 << 30), "3.0 GB");
        assert_eq!(format_quota_bytes(DISPLAY_BYTES_CAP - 1), "1024.0 TB");
        assert_eq!(format_quota_bytes(DISPLAY_BYTES_CAP), "≥ 1 PB");
        assert_eq!(format_quota_bytes(u64::MAX), "≥ 1 PB");
    }

    #[test]
    fn meters_cover_unlimited_exhausted_and_partial() {
        let usage = ShareUsageDto {
            kinds: vec![
                kind(ShareTrafficKind::Swarm, false, 100, 25, 75),
                kind(ShareTrafficKind::Download, true, 0, 999, 0),
                kind(ShareTrafficKind::Upload, false, 100, 100, 0),
                kind(ShareTrafficKind::Unknown, false, 1, 0, 1),
            ],
            ..ShareUsageDto::default()
        };
        let views = meters(&usage);
        assert_eq!(views.len(), 3);
        assert_eq!(views[0].label_key, "shareUsageKindSwarm");
        assert!((views[0].ratio - 0.25).abs() < f32::EPSILON);
        assert!(!views[0].exhausted);
        assert!(views[1].unlimited);
        assert!(!views[1].exhausted);
        assert!(views[1].ratio.abs() < f32::EPSILON);
        assert!(views[2].exhausted);
        assert!((views[2].ratio - 1.).abs() < f32::EPSILON);
    }

    #[test]
    fn grant_sources_map_to_keys() {
        assert_eq!(
            grant_source_key(ShareGrantSource::Pack),
            "shareUsageGrantPack"
        );
        assert_eq!(
            grant_source_key(ShareGrantSource::Rollover),
            "shareUsageGrantRollover"
        );
        assert_eq!(
            grant_source_key(ShareGrantSource::Contribution),
            "shareUsageGrantContribution"
        );
        for source in [
            ShareGrantSource::Admin,
            ShareGrantSource::Free,
            ShareGrantSource::Unknown,
        ] {
            assert_eq!(grant_source_key(source), "shareUsageGrantGranted");
        }
    }

    #[test]
    fn level_and_boost_labels() {
        assert_eq!(level_label(3, 10), "Lv.3");
        assert_eq!(level_label(12, 10), "Lv.10");
        assert_eq!(level_label(0, 0), "Lv.0");
        assert_eq!(boost_label(1000), None);
        assert_eq!(boost_label(1500).as_deref(), Some("×1.5"));
        assert_eq!(boost_label(2000).as_deref(), Some("×2"));
        assert_eq!(boost_label(1250).as_deref(), Some("×1.25"));
        assert_eq!(boost_label(500).as_deref(), Some("×0.5"));
    }
}
