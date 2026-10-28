// FluxMesh（临时文件分享）DTO（native/protocol/src/share.rs）。
//
// 时间一律 RFC 3339 字符串；字节数与速率用 number；`maxClaims === 0` 表示不限。
// 进度密集的数据走 `shareProgress` 事件（同一 sessionId 的后帧完整取代前帧），不进整段替换。

import type { ErrorReason } from './error';

/** 快照里最多保留的最近接收记录数（Rust `SHARE_STATE_RECEIVE_RECORDS`）。 */
export const SHARE_STATE_RECEIVE_RECORDS = 20;

/** `cdn` = 云端暂存副本（任务行徽标「云端」）；`bridge` = share-gateway 实时桥接（「桥接」）。 */
export type SharePathKind = 'directLan' | 'directWan' | 'relay' | 'cdn' | 'bridge' | 'swarm' | 'unknown';
export type ShareProvideState = 'off' | 'providing' | 'ended' | 'stopped';
export type ShareNatType = 'cone' | 'symmetric' | 'unknown';
export type ShareConnectionMode = 'auto' | 'relayOnly' | 'directOnly';
export type ShareExitBehavior = 'prompt' | 'convertToCloud' | 'quit';
export type ShareLogLevel = 'error' | 'warn' | 'info' | 'debug' | 'trace';
/** `cloud` = 云端副本已就绪（`cloudState === 'ready'`）。 */
export type ShareMode = 'live' | 'cloud';
/** 线上字面量 `none` 对应 Rust 的 `Idle`；未知值解码为 `unknown`。 */
export type ShareCloudState = 'none' | 'uploading' | 'ready' | 'deleted' | 'unknown';
export type ShareUploadStatus = 'uploading' | 'paused' | 'failed';
export type ShareStatus = 'active' | 'expired' | 'stopped' | 'unknown';
export type ShareSessionRole = 'sender' | 'receiver' | 'swarm';
export type ShareReceiveStatus = 'active' | 'completed' | 'failed';

export interface ShareFileDto {
  name: string;
  size: number;
}

/** 分享方视角下一个正在取件的接收方会话。 */
export interface ShareReceiverDto {
  sessionId: string;
  /** 游客或未知为空串。 */
  deviceName: string;
  pathKind: SharePathKind;
  /** 字节 / 秒。 */
  rateBps: number;
  bytes: number;
  /** 0..1。 */
  progress: number;
  /** RFC 3339。 */
  startedAt: string;
}

export interface ShareDto {
  id: string;
  slug: string;
  /** 规范分享链接；二维码由 UI 据此渲染。 */
  url: string;
  pickupCode: string;
  title: string;
  files: ShareFileDto[];
  totalBytes: number;
  mode: ShareMode;
  tierId: string;
  /** RFC 3339。 */
  expiresAt: string;
  /** 0 = 不限。 */
  maxClaims: number;
  claimCount: number;
  notifyOnClaim: boolean;
  status: ShareStatus;
  lastAccessedAt: string | null;
  taskIds: string[];
  activeReceivers: ShareReceiverDto[];
  /** 云端副本状态。 */
  cloudState: ShareCloudState;
  /** 云端已存储 / 计费的字节数。 */
  cloudBytes: number;
  /** 只在上传中 / 暂停 / 失败时出现，经 `shareChanged` 整体下发（约 2 次 / 秒）。 */
  upload?: ShareUploadDto;
  /** 本机源文件已缺失或已被修改，分享无法继续 / 重新开始，只能删除；为 false 时 agent 不下发。 */
  sourceMissing?: boolean;
  /** 同一内容的其他设备可从本分享共享取块。 */
  swarm?: boolean;
  /** 管理员已关闭并锁定该分享的共享：此时 `swarm` 恒为 false，且不能再打开。 */
  swarmLocked?: boolean;
}

/** 云端暂存上传进度。 */
export interface ShareUploadDto {
  status: ShareUploadStatus;
  doneBytes: number;
  totalBytes: number;
  /** 字节 / 秒；暂停 / 失败为 0。 */
  rateBps: number;
  /** `failed` / `paused` 时的原因。 */
  reason?: ErrorReason;
}

export interface ShareTierDto {
  id: string;
  hours: number;
}

export interface ShareRelayDto {
  url: string;
  region: string;
}

export interface SharePolicySummaryDto {
  enabled: boolean;
  guestReceive: boolean;
  guestBridge: boolean;
  guestShare: boolean;
  pickupCodeLength: number;
  liveTiers: ShareTierDto[];
  /** 云端暂存保留期档位（`cloud-…`，≤ 336 小时）。 */
  retentionTiers: ShareTierDto[];
  /** 云端存储已配置；`false` 时隐藏「转为云端暂存」。 */
  cloudEnabled: boolean;
  relays: ShareRelayDto[];
  holePunchHintSecs: number;
}

/** 本机FluxMesh设置；`agent.share.setLocalConfig` 整体替换。 */
export interface ShareLocalConfigDto {
  udpPortV4: number | null;
  udpPortV6: number | null;
  portmapper: boolean | null;
  extraAddrs: string[];
  mode: ShareConnectionMode;
  pinnedRelay: string | null;
  sendRateLimitBytesPerSec: number | null;
  exitBehavior: ShareExitBehavior;
  irohLogLevel: ShareLogLevel;
  defaultTierId: string | null;
  /** 0 = 不限。 */
  defaultMaxClaims: number;
  defaultNotifyOnClaim: boolean;
  /** null = 开（默认）；false = 新建分享默认不加入共享。 */
  defaultSwarm?: boolean | null;
  /** null = 开（默认）；false = 接收的文件不参与共享共享。 */
  autoProvide?: boolean | null;
}

export interface ShareHomeRelayDto {
  url: string;
  connected: boolean;
}

export interface ShareNetStatusDto {
  bound: boolean;
  online: boolean;
  endpointId: string | null;
  homeRelay: ShareHomeRelayDto | null;
  natType: ShareNatType;
  isp: string | null;
  province: string | null;
  ipv6: boolean;
  relayLatencyMs: Record<string, number>;
}

/** 下载任务上的FluxMesh事实（只含FluxMesh接收任务，键为 daemon 任务 ID）。 */
export interface ShareTaskFactsDto {
  isShareReceive: boolean;
  pathKind: SharePathKind;
  peerNatType: ShareNatType;
  peerIsp: string | null;
  shareId: string | null;
  sessionId: string | null;
  /** 打洞超过 `policy.holePunchHintSecs` 仍走中继；UI 据此显示「切换桥接」提示。 */
  relayStuck: boolean;
  /** 当前可用的共享对端数。 */
  swarmPeers?: number;
}

/** 创建分享时正在做块哈希的任务（`agent.share.create` 返回前出现，完成后移除）。 */
export interface ShareHashingDto {
  id: string;
  title: string;
  doneBytes: number;
  totalBytes: number;
}

export interface ShareReceiveRecordDto {
  id: string;
  shareId: string | null;
  title: string;
  senderDeviceName: string;
  fileCount: number;
  totalBytes: number;
  taskIds: string[];
  /** RFC 3339。 */
  startedAt: string;
  status: ShareReceiveStatus;
  provideState?: ShareProvideState;
}

/** FluxMesh分区完整状态（`AgentSnapshot.share` / `shareChanged`）。 */
export interface ShareStateDto {
  /** 云端策略允许FluxMesh。 */
  enabled: boolean;
  /** 本构建编入FluxMesh、服务在运行且云端允许；`false` 时隐藏全部FluxMesh入口。 */
  available: boolean;
  /** 本机当前可发起分享。 */
  canSend: boolean;
  localConfig: ShareLocalConfigDto;
  policySummary: SharePolicySummaryDto;
  shares: ShareDto[];
  netStatus: ShareNetStatusDto;
  taskFacts: Record<string, ShareTaskFactsDto>;
  receiveRecords: ShareReceiveRecordDto[];
  /** 链接 / 深链打开后 agent 已解析、等待 UI 确认的接收预览；旧快照缺失视为 `null`。 */
  pendingReceive?: ShareResolveDto | null;
  /** 创建分享时正在做块哈希的任务；旧快照缺失视为空。 */
  hashing?: ShareHashingDto[];
}

/** 单个会话的进度 / 路径刷新（`shareProgress`，同 sessionId 后帧完整取代前帧）。 */
export interface ShareProgressDto {
  sessionId: string;
  pathKind: SharePathKind;
  rateBps: number;
  bytes: number;
  progress: number;
}

// ───────────────────────── RPC 参数 / 结果 ─────────────────────────

export interface ShareConfigDto {
  policy: SharePolicySummaryDto;
  localConfig: ShareLocalConfigDto;
}

export interface ShareCreateParams {
  taskIds: string[];
  tierId?: string;
  maxClaims?: number;
  notifyOnClaim?: boolean;
  swarm?: boolean;
}

export interface ShareUpdateParams {
  id: string;
  tierId?: string;
  maxClaims?: number;
  notifyOnClaim?: boolean;
  swarm?: boolean;
}

export interface ShareIdParams {
  id: string;
}

export interface ShareStopProvidingParams {
  recordId: string;
}

/** `agent.share.convertToCloud` 参数；`tierId` 取自 `SharePolicySummaryDto.retentionTiers`，缺省取云端默认档。 */
export interface ShareConvertToCloudParams {
  id: string;
  tierId?: string;
}

export interface ShareResolveParams {
  /** 6 位取件码、`https://…/s/<slug>` 或 `fluxdown://share/<slug>`。 */
  codeOrLink: string;
  turnstileToken?: string;
}

export interface ShareSourcesDto {
  senderOnline: boolean | null;
  cloudCopy: boolean;
  bridge: boolean;
  swarmPeers?: number;
}

export interface ShareResolveDto {
  resolveId: string;
  shareId: string;
  title: string;
  senderDeviceName: string;
  files: ShareFileDto[];
  totalBytes: number;
  expiresAt: string;
  sources: ShareSourcesDto;
}

/** `resolveId` 与 `codeOrLink` 二选一。 */
export interface ShareReceiveParams {
  resolveId?: string;
  codeOrLink?: string;
  saveDir?: string;
  fileIndexes?: number[];
}

export interface ShareDismissPendingParams {
  resolveId: string;
}

export interface ShareReceiveResultDto {
  recordId: string;
  taskIds: string[];
}

export interface ShareReceiveRecordsParams {
  limit?: number;
  cursor?: string;
}

export interface ShareReceiveRecordsPage {
  records: ShareReceiveRecordDto[];
  nextCursor: string | null;
}

export interface ShareSwitchToBridgeParams {
  taskId: string;
}

export interface ShareDiagnosticsParams {
  sessionId?: string;
}

export interface ShareLogsParams {
  level?: ShareLogLevel;
  regex?: string;
  limit?: number;
}

export interface ShareLogLineDto {
  /** RFC 3339，毫秒精度。 */
  at: string;
  level: ShareLogLevel;
  target: string;
  message: string;
}

export interface ShareLogsDto {
  lines: ShareLogLineDto[];
}

export interface ShareSessionKillParams {
  sessionId: string;
}

// ───────────────────────── 诊断 ─────────────────────────

export interface ShareNetReportDto {
  udpV4: boolean;
  udpV6: boolean;
  mappingVariesByDest: boolean | null;
  natType: ShareNatType;
  preferredRelay?: string;
  relayLatencyMs: Record<string, number>;
  globalV4?: string;
  globalV6?: string;
  captivePortal?: boolean;
}

/** 展示与上报 FluxCloud 的同一份端点网络快照。 */
export interface ShareNetSnapshotDto {
  capturedAt: string;
  endpointId: string;
  clientVersion: string;
  platform: string;
  netReport?: ShareNetReportDto;
  homeRelay?: ShareHomeRelayDto;
  addrs: string[];
  /** 全部 iroh 指标：键 `<group>.<name>`，直方图展开为 `.count` / `.sum`。 */
  metrics: Record<string, unknown>;
}

export interface ShareSessionBytesDto {
  direct: number;
  relay: number;
  /** 经云端暂存副本（CDN）取得并通过校验的字节。 */
  cdn: number;
  /** 经 share-gateway 桥接取得并通过校验的字节。 */
  bridge: number;
  /** 从共享对端取回并通过校验的字节。 */
  peer?: number;
}

export interface SharePathSummaryDto {
  id: string;
  kind: SharePathKind;
  remote: string;
  rttMs: number;
  cwnd: number;
  mtu: number;
  sent: number;
  recv: number;
  lost: number;
  congestionEvents: number;
  /** bit/s，无估计为 0。 */
  bwEstimate: number;
}

export interface SharePathEventDto {
  at: string;
  /** `opened` / `selected` / `closed` / `lagged`。 */
  kind: string;
  path: string;
}

export interface ShareSessionSummaryDto {
  startedAt: string;
  connectMs: number;
  firstDirectMs?: number;
  pathKind: SharePathKind;
  bytes: ShareSessionBytesDto;
  paths: SharePathSummaryDto[];
  events: SharePathEventDto[];
  verifyFailures: number;
  error?: string;
}

export interface ShareSessionSampleDto {
  at: string;
  bytes: number;
  rateBps: number;
  rttMs: number;
  pathKind: SharePathKind;
}

/** 共享提供方会话里单个内容的累计服务量。 */
export interface ShareSwarmServedFileDto {
  /** 文件根哈希（64 位小写 hex）。 */
  rootHash: string;
  /** 本机已知该内容的文件名；未知为 `null`（UI 回退显示短哈希）。 */
  fileName?: string | null;
  bytes: number;
  chunks: number;
}

export interface ShareSessionDetailDto {
  sessionId: string;
  shareId: string;
  role: ShareSessionRole;
  peerEndpointId: string;
  peerDeviceName: string | null;
  peerNatType: ShareNatType;
  peerIsp: string | null;
  ended: boolean;
  summary: ShareSessionSummaryDto;
  samples: ShareSessionSampleDto[];
  /** 共享提供方（`role === 'swarm'`）：本连接按内容累计已发送的字节与块数；其他角色为空。 */
  swarmFiles?: ShareSwarmServedFileDto[];
}

export interface ShareDiagnosticsDto {
  /** 端点未绑定为 `null`。 */
  snapshot: ShareNetSnapshotDto | null;
  /** 与上报 FluxCloud 完全相同的快照 JSON 文本；端点未绑定为空串。 */
  snapshotJson: string;
  sessions: ShareSessionDetailDto[];
}
