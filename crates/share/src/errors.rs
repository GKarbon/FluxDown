//! RPC 错误 → 文案键：优先按 `reason`（`errReasonShare*`），缺失或非FluxMesh原因时给通用失败提示。

use fluxdown_protocol::{ErrorReason, RpcErrorData};
use fluxdown_ui_i18n::Translator;

/// 通用失败文案键（agent 不可达 / 未带细分原因）。
const GENERIC_ERROR_KEY: &str = "localServiceActionFailed";

/// 错误的文案键。
#[must_use]
pub fn error_key(error: &RpcErrorData) -> &'static str {
    reason_key(error.reason)
}

/// 错误原因的文案键（云端上传进度里的失败 / 暂停原因没有 `RpcErrorData`，直接按原因取）。
#[must_use]
pub fn reason_key(reason: Option<ErrorReason>) -> &'static str {
    match reason {
        Some(ErrorReason::ShareUnavailable) => "errReasonShareUnavailable",
        Some(ErrorReason::ShareNotFound) => "errReasonShareNotFound",
        Some(ErrorReason::ShareExpired) => "errReasonShareExpired",
        Some(ErrorReason::ShareStopped) => "errReasonShareStopped",
        Some(ErrorReason::ShareCodeInvalid) => "errReasonShareCodeInvalid",
        Some(ErrorReason::ShareRateLimited) => "errReasonShareRateLimited",
        Some(ErrorReason::ShareTurnstileRequired) => "errReasonShareTurnstileRequired",
        Some(ErrorReason::ShareFileChanged) => "errReasonShareFileChanged",
        Some(ErrorReason::ShareSenderOffline) => "errReasonShareSenderOffline",
        Some(ErrorReason::ShareTaskNotShareable) => "errReasonShareTaskNotShareable",
        Some(ErrorReason::ShareBridgeUnavailable) => "errReasonShareBridgeUnavailable",
        Some(ErrorReason::ShareQuotaExceeded) => "errReasonShareQuotaExceeded",
        Some(ErrorReason::ShareCloudNotReady) => "errReasonShareCloudNotReady",
        Some(ErrorReason::ShareTrafficExhausted) => "errReasonShareTrafficExhausted",
        Some(ErrorReason::ShareCloudUnavailable) => "errReasonShareCloudUnavailable",
        Some(ErrorReason::ShareOwnShare) => "errReasonShareOwnShare",
        Some(ErrorReason::ShareSourceMissing) => "errReasonShareSourceMissing",
        Some(ErrorReason::ShareSwarmLocked) => "errReasonShareSwarmLocked",
        _ => GENERIC_ERROR_KEY,
    }
}

/// 错误的本地化文案。
#[must_use]
pub fn error_text(translator: &Translator, error: &RpcErrorData) -> String {
    translator.text(error_key(error)).to_owned()
}

#[cfg(test)]
mod tests {
    use fluxdown_protocol::{ApplicationErrorCode, ErrorReason, RpcErrorData};

    use super::error_key;

    #[test]
    fn share_reasons_map_to_their_keys_and_everything_else_is_generic() {
        let with =
            |reason| RpcErrorData::new(ApplicationErrorCode::Conflict, false).with_reason(reason);
        assert_eq!(
            error_key(&with(ErrorReason::ShareExpired)),
            "errReasonShareExpired"
        );
        assert_eq!(
            error_key(&with(ErrorReason::ShareBridgeUnavailable)),
            "errReasonShareBridgeUnavailable"
        );
        for (reason, key) in [
            (
                ErrorReason::ShareCloudNotReady,
                "errReasonShareCloudNotReady",
            ),
            (
                ErrorReason::ShareTrafficExhausted,
                "errReasonShareTrafficExhausted",
            ),
            (
                ErrorReason::ShareCloudUnavailable,
                "errReasonShareCloudUnavailable",
            ),
            (ErrorReason::ShareSwarmLocked, "errReasonShareSwarmLocked"),
        ] {
            assert_eq!(error_key(&with(reason)), key);
        }
        assert_eq!(
            error_key(&with(ErrorReason::SaveDirUnavailable)),
            "localServiceActionFailed"
        );
        assert_eq!(
            error_key(&RpcErrorData::new(ApplicationErrorCode::Unavailable, true)),
            "localServiceActionFailed"
        );
    }
}
