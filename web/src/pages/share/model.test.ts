import { describe, expect, test } from 'bun:test'
import type { ShareDto, ShareNetStatusDto, ShareReceiveRecordDto, ShareTaskFactsDto } from '../../lib/rpc'
import {
  CLOUD_STATE_KEYS,
  canStartShare,
  canStopShare,
  isSourceMissing,
  planShareBatch,
  pruneSelection,
  selectAllState,
  bytesSplit,
  canDeleteCloudCopy,
  canSwitchBridge,
  cloudConvertAvailability,
  cloudRetryTierId,
  claimOptions,
  currentPathId,
  extractShareLinks,
  findExistingShare,
  formatPickupCode,
  isPickupCode,
  mergeReceiveRecords,
  netStatusLine,
  netSummary,
  normalizePickupDigits,
  parseShareInput,
  parseShareLink,
  reasonTextKey,
  retentionTiers,
  progressPercent,
  rateLabel,
  removeShareLink,
  shareDisabledKey,
  shareDisplayStatus,
  shareNoticeKey,
  shareSourceBadge,
  sortShares,
  sourceBytesRows,
  SESSION_ROLE_KEYS,
  servedFileLabel,
  shortId,
  swarmToggle,
  tierLabel,
  uploadPercent,
  uploadProgressText,
} from './model'

const dict: Record<string, string> = {
  shareDurationHours: '{count} h',
  shareDurationDays: '{count} d',
  shareCloudUploadProgress: '{done} / {total} ({percent}%) · {rate}',
  shareCloudUploadPaused: '{done} / {total} ({percent}%) paused',
  shareCloudUploadFailed: '{done} / {total} ({percent}%) failed',
  shareNetHealthIdle: 'idle',
  shareNetHealthOffline: 'offline',
  shareNatCone: 'cone',
  shareNatSymmetric: 'symmetric',
  shareNatUnknown: 'unknown',
  shareIpv6On: 'IPv6',
  shareIpv6Off: 'no IPv6',
  shareRelayLatency: 'relay {ms} ms',
}
const t = (key: string, params?: Record<string, string | number>): string => {
  let text = dict[key] ?? key
  for (const [name, value] of Object.entries(params ?? {})) text = text.replace(`{${name}}`, String(value))
  return text
}

const SLUG = 'AbC123_-xYz9'

function share(patch: Partial<ShareDto> = {}): ShareDto {
  return {
    id: 's1',
    slug: SLUG,
    url: `https://fluxdown.com/s/${SLUG}`,
    pickupCode: '123456',
    title: 'a.zip',
    files: [{ name: 'a.zip', size: 10 }],
    totalBytes: 10,
    mode: 'live',
    tierId: '24h',
    expiresAt: '2026-10-27T12:00:00Z',
    maxClaims: 0,
    claimCount: 0,
    notifyOnClaim: true,
    status: 'active',
    lastAccessedAt: null,
    taskIds: ['t1'],
    activeReceivers: [],
    cloudState: 'none',
    cloudBytes: 0,
    ...patch,
  }
}

function net(patch: Partial<ShareNetStatusDto> = {}): ShareNetStatusDto {
  return {
    bound: true,
    online: true,
    endpointId: 'ep',
    homeRelay: { url: 'https://r1', connected: true },
    natType: 'cone',
    isp: '联通',
    province: '北京',
    ipv6: true,
    relayLatencyMs: { 'https://r1': 18.4, 'https://r2': 40 },
    ...patch,
  }
}

describe('pickup code', () => {
  test('normalize drops separators and non-digits, caps at 6', () => {
    expect(normalizePickupDigits('12 34-56')).toBe('123456')
    expect(normalizePickupDigits('1234567890')).toBe('123456')
    expect(normalizePickupDigits('ab1c2')).toBe('12')
    expect(normalizePickupDigits('')).toBe('')
  })

  test('format groups by three', () => {
    expect(formatPickupCode('123456')).toBe('123 456')
    expect(formatPickupCode('1234')).toBe('123 4')
    expect(formatPickupCode('')).toBe('')
  })

  test('validation requires exactly six digits', () => {
    expect(isPickupCode('123456')).toBe(true)
    expect(isPickupCode(' 123 456 ')).toBe(true)
    expect(isPickupCode('123-456')).toBe(true)
    expect(isPickupCode('12345')).toBe(false)
    expect(isPickupCode('1234567')).toBe(false)
    expect(isPickupCode('12a456')).toBe(false)
    expect(isPickupCode('')).toBe(false)
  })
})

describe('share link detection', () => {
  test('deep link and https link', () => {
    expect(parseShareLink(`fluxdown://share/${SLUG}`)).toEqual({ link: `fluxdown://share/${SLUG}`, slug: SLUG, scheme: 'deeplink' })
    expect(parseShareLink(`FluxDown://share/${SLUG}/`)?.scheme).toBe('deeplink')
    expect(parseShareLink(`  https://fluxdown.com/s/${SLUG}\n`)).toEqual({ link: `https://fluxdown.com/s/${SLUG}`, slug: SLUG, scheme: 'https' })
    expect(parseShareLink(`https://fluxdown.zerx.dev/s/${SLUG}?from=qr`)?.slug).toBe(SLUG)
    expect(parseShareLink(`https://share.example.net/s/${SLUG}`)).toBeNull()
  })

  test('rejects lookalikes', () => {
    expect(parseShareLink('')).toBeNull()
    expect(parseShareLink('https://fluxdown.com/s/short')).toBeNull()
    expect(parseShareLink(`https://fluxdown.com/s/${SLUG}extra`)).toBeNull()
    expect(parseShareLink(`https://fluxdown.com/x/${SLUG}`)).toBeNull()
    expect(parseShareLink(`http://fluxdown.com/s/${SLUG}`)).toBeNull()
    expect(parseShareLink(`fluxdown://download?url=${SLUG}`)).toBeNull()
    expect(parseShareLink(`https://fluxdown.com/s/${SLUG} https://a.b/c`)).toBeNull()
  })

  test('input is a code, a link, or nothing', () => {
    expect(parseShareInput('123 456')).toEqual({ kind: 'code', value: '123456' })
    expect(parseShareInput(`fluxdown://share/${SLUG}`)).toEqual({ kind: 'link', value: `fluxdown://share/${SLUG}`, scheme: 'deeplink' })
    expect(parseShareInput('hello')).toBeNull()
    expect(parseShareInput('12345')).toBeNull()
  })
})

describe('status derivation', () => {
  const now = Date.parse('2026-10-27T00:00:00Z')

  test('active share past its expiry shows as expired', () => {
    expect(shareDisplayStatus(share(), now)).toBe('active')
    expect(shareDisplayStatus(share({ expiresAt: '2026-10-26T23:59:59Z' }), now)).toBe('expired')
    expect(shareDisplayStatus(share({ status: 'expired' }), now)).toBe('expired')
    expect(shareDisplayStatus(share({ status: 'stopped' }), now)).toBe('stopped')
    expect(shareDisplayStatus(share({ status: 'unknown' }), now)).toBe('unknown')
  })

  test('tier labels', () => {
    expect(tierLabel(t, { hours: 24 })).toBe('24 h')
    expect(tierLabel(t, { hours: 168 })).toBe('7 d')
    expect(tierLabel(t, { hours: 50 })).toBe('50 h')
  })

  test('existing share is reused only for the same task set while active', () => {
    const shares = [share({ id: 'a', taskIds: ['t1', 't2'] }), share({ id: 'b', taskIds: ['t3'], status: 'stopped' })]
    expect(findExistingShare(shares, ['t2', 't1'], now)?.id).toBe('a')
    expect(findExistingShare(shares, ['t1'], now)).toBeUndefined()
    expect(findExistingShare(shares, ['t3'], now)).toBeUndefined()
    expect(findExistingShare(shares, [], now)).toBeUndefined()
  })
})

describe('claims options', () => {
  test('presets with unlimited last; current value merged in order', () => {
    expect(claimOptions(0)).toEqual([1, 3, 5, 10, 20, 50, 0])
    expect(claimOptions(7)).toEqual([1, 3, 5, 7, 10, 20, 50, 0])
    expect(claimOptions(10)).toEqual([1, 3, 5, 10, 20, 50, 0])
  })
})

describe('source badge', () => {
  const facts = (patch: Partial<ShareTaskFactsDto>): ShareTaskFactsDto => ({
    isShareReceive: true,
    pathKind: 'directLan',
    peerNatType: 'cone',
    peerIsp: null,
    shareId: 's1',
    sessionId: 'x',
    relayStuck: false,
    ...patch,
  })

  test('only share-receive tasks get a badge', () => {
    expect(shareSourceBadge(undefined)).toBeNull()
    expect(shareSourceBadge(facts({ isShareReceive: false }))).toBeNull()
  })

  test('badge label and tone follow the path', () => {
    expect(shareSourceBadge(facts({}))).toEqual({ pathKind: 'directLan', labelKey: 'sharePathDirectLan', tone: 'success', peers: 0 })
    expect(shareSourceBadge(facts({ pathKind: 'directWan' }))?.tone).toBe('success')
    expect(shareSourceBadge(facts({ pathKind: 'relay' }))).toEqual({ pathKind: 'relay', labelKey: 'sharePathRelay', tone: 'warning', peers: 0 })
    expect(shareSourceBadge(facts({ pathKind: 'cdn' }))).toEqual({ pathKind: 'cdn', labelKey: 'shareBadgeCloud', tone: 'success', peers: 0 })
    expect(shareSourceBadge(facts({ pathKind: 'bridge' }))).toEqual({ pathKind: 'bridge', labelKey: 'shareBadgeBridge', tone: 'neutral', peers: 0 })
    expect(shareSourceBadge(facts({ pathKind: 'unknown' }))?.tone).toBe('neutral')
    expect(shareSourceBadge(facts({ pathKind: 'swarm', swarmPeers: 3 }))).toEqual({ pathKind: 'swarm', labelKey: 'sharePathSwarm', tone: 'success', peers: 3 })
    expect(sourceBytesRows({ direct: 0, relay: 0, cdn: 0, bridge: 0, peer: 9 }).map((row) => [row.kind, row.bytes]).at(-1)).toEqual(['peer', 9])
    expect(bytesSplit({ direct: 1, relay: 0, cdn: 0, bridge: 0, peer: 3 }).percent).toBe(25)
  })

  test('bridge switch is offered on active share-receive tasks not already on the bridge', () => {
    expect(canSwitchBridge(facts({ pathKind: 'relay' }), 'downloading')).toBe(true)
    expect(canSwitchBridge(facts({ pathKind: 'directWan' }), 'downloading')).toBe(true)
    expect(canSwitchBridge(facts({ pathKind: 'cdn' }), 'pending')).toBe(true)
    expect(canSwitchBridge(facts({ pathKind: 'bridge' }), 'downloading')).toBe(false)
    expect(canSwitchBridge(facts({ pathKind: 'relay' }), 'paused')).toBe(false)
    expect(canSwitchBridge(facts({ pathKind: 'relay' }), 'completed')).toBe(false)
    expect(canSwitchBridge(facts({ pathKind: 'relay', isShareReceive: false }), 'downloading')).toBe(false)
    expect(canSwitchBridge(undefined, 'downloading')).toBe(false)
  })
})

describe('network status', () => {
  test('summary picks the home relay latency, else the lowest', () => {
    expect(netSummary(net()).relayMs).toBe(18.4)
    expect(netSummary(net({ homeRelay: null })).relayMs).toBe(18.4)
    expect(netSummary(net({ homeRelay: { url: 'https://missing', connected: false } })).relayMs).toBe(18.4)
    expect(netSummary(net({ relayLatencyMs: {} })).relayMs).toBeNull()
  })

  test('state derives from bound / online', () => {
    expect(netSummary(net({ bound: false })).state).toBe('unbound')
    expect(netSummary(net({ online: false })).state).toBe('offline')
    expect(netSummary(net()).state).toBe('online')
  })

  test('status line', () => {
    expect(netStatusLine(t, net())).toBe('cone · 北京 联通 · IPv6 · relay 18 ms')
    expect(netStatusLine(t, net({ isp: null, province: null, ipv6: false, relayLatencyMs: {} }))).toBe('cone · no IPv6')
    expect(netStatusLine(t, net({ bound: false }))).toBe('idle')
    expect(netStatusLine(t, net({ online: false }))).toBe('offline')
  })
})

describe('sendability', () => {
  const view = { source: 'local' as const, state: 'completed', fileMissing: false }

  test('single task gets a specific reason; a mixed selection a general one', () => {
    expect(shareDisabledKey([])).toBe('shareDisabledSelection')
    expect(shareDisabledKey([view])).toBeNull()
    expect(shareDisabledKey([view, view])).toBeNull()
    expect(shareDisabledKey([{ ...view, source: 'remote' }])).toBe('shareDisabledRemote')
    expect(shareDisabledKey([{ ...view, state: 'downloading' }])).toBe('shareDisabledIncomplete')
    expect(shareDisabledKey([{ ...view, fileMissing: true }])).toBe('shareDisabledMissing')
    expect(shareDisabledKey([view, { ...view, source: 'remote' }])).toBe('shareDisabledSelection')
    expect(shareDisabledKey([view, { ...view, fileMissing: true }])).toBe('shareDisabledSelection')
  })
})

describe('numbers', () => {
  test('rate label and percent', () => {
    expect(rateLabel(1536)).toBe('1.5 KB/s')
    expect(rateLabel(-1)).toBe('0 B/s')
    expect(progressPercent(0.456)).toBe(46)
    expect(progressPercent(2)).toBe(100)
    expect(progressPercent(Number.NaN)).toBe(0)
  })
})

describe('lists', () => {
  const now = Date.parse('2026-10-27T00:00:00Z')

  test('shares: active first, then latest expiry first', () => {
    const list = [
      share({ id: 'stopped', status: 'stopped', expiresAt: '2026-12-01T00:00:00Z' }),
      share({ id: 'soon', expiresAt: '2026-10-28T00:00:00Z' }),
      share({ id: 'later', expiresAt: '2026-11-05T00:00:00Z' }),
      share({ id: 'lapsed', expiresAt: '2026-10-01T00:00:00Z' }),
    ]
    expect(sortShares(list, now).map((item) => item.id)).toEqual(['later', 'soon', 'stopped', 'lapsed'])
    expect(list[0]?.id).toBe('stopped')
  })

  const record = (id: string, startedAt: string, status: ShareReceiveRecordDto['status']): ShareReceiveRecordDto => ({
    id,
    shareId: null,
    title: id,
    senderDeviceName: '',
    fileCount: 1,
    totalBytes: 1,
    taskIds: [],
    startedAt,
    status,
  })

  test('receive records: live snapshot wins over paged, newest first', () => {
    const merged = mergeReceiveRecords(
      [record('b', '2026-10-26T10:00:00Z', 'completed')],
      [record('b', '2026-10-26T10:00:00Z', 'active'), record('a', '2026-10-25T10:00:00Z', 'failed'), record('c', 'bad', 'failed')],
    )
    expect(merged.map((item) => item.id)).toEqual(['b', 'a', 'c'])
    expect(merged[0]?.status).toBe('completed')
  })
})

describe('new download paste', () => {
  const deep = `fluxdown://share/${SLUG}`
  const https = `https://fluxdown.com/s/${SLUG}`

  test('share links are found line by line; one is removed without touching the rest', () => {
    const text = `https://example.com/a.zip\n${deep}\n  out=a.zip\n${https}\r\n# note`
    expect(extractShareLinks(text).map((ref) => ref.link)).toEqual([deep, https])
    expect(removeShareLink(text, deep)).toBe(`https://example.com/a.zip\n  out=a.zip\n${https}\n# note`)
    expect(removeShareLink(`${deep}\n${deep}`, deep)).toBe(deep)
    expect(removeShareLink('https://example.com/a.zip', deep)).toBe('https://example.com/a.zip')
    expect(extractShareLinks('https://example.com/a.zip')).toEqual([])
  })

  test('notice depends on availability, count and mixing', () => {
    expect(shareNoticeKey(0, 3, true)).toBeNull()
    expect(shareNoticeKey(1, 0, false)).toBe('shareNewDownloadUnavailable')
    expect(shareNoticeKey(2, 0, true)).toBe('shareNewDownloadMultiple')
    expect(shareNoticeKey(1, 2, true)).toBe('shareNewDownloadMixed')
    expect(shareNoticeKey(1, 0, true)).toBe('shareNewDownloadLink')
  })
})

describe('path diagnostics', () => {
  test('current path follows the latest selected event until it closes', () => {
    expect(currentPathId([])).toBeNull()
    expect(currentPathId([{ kind: 'opened', path: 'a' }, { kind: 'selected', path: 'a' }])).toBe('a')
    expect(currentPathId([{ kind: 'selected', path: 'a' }, { kind: 'selected', path: 'b' }, { kind: 'closed', path: 'a' }])).toBe('b')
    expect(currentPathId([{ kind: 'selected', path: 'a' }, { kind: 'closed', path: 'a' }])).toBeNull()
  })

  test('direct share of bytes', () => {
    expect(bytesSplit({ direct: 0, relay: 0, cdn: 0, bridge: 0 }).percent).toBeNull()
    expect(bytesSplit({ direct: 3, relay: 1, cdn: 0, bridge: 0 }).percent).toBe(75)
    expect(bytesSplit({ direct: 0, relay: 5, cdn: 0, bridge: 0 }).percent).toBe(0)
    expect(bytesSplit({ direct: 1, relay: 0, cdn: 2, bridge: 1 }).percent).toBe(25)
  })

  test('swarm provider session: role label, served file name or short root hash', () => {
    expect(SESSION_ROLE_KEYS.swarm).toBe('shareNetRoleSwarm')
    expect(SESSION_ROLE_KEYS.sender).toBe('shareNetRoleSender')
    const root = 'ab'.repeat(32)
    expect(servedFileLabel({ rootHash: root, fileName: '电影 2024 第一季.mkv' })).toBe('电影 2024 第一季.mkv')
    for (const fileName of [undefined, null, '', '  \t']) {
      expect(servedFileLabel({ rootHash: root, fileName })).toBe(`${root.slice(0, 6)}…${root.slice(-4)}`)
    }
    expect(shortId('peer')).toBe('peer')
    expect(shortId('日本語のエンドポイント識別子です')).toBe('日本語のエン…別子です')
  })

  test('swarm toggle is off and disabled once an administrator locked it', () => {
    expect(swarmToggle({ swarm: true }, true, false)).toEqual({ checked: true, disabled: false, labelKey: 'shareSwarmOn' })
    expect(swarmToggle({ swarm: false }, true, false)).toEqual({ checked: false, disabled: false, labelKey: 'shareSwarmOff' })
    expect(swarmToggle({}, true, false).checked).toBe(false)
    expect(swarmToggle({ swarm: true }, false, false).disabled).toBe(true)
    expect(swarmToggle({ swarm: true }, true, true).disabled).toBe(true)
    // 锁定时即使本地缓存里还带着 swarm=true，也显示为关，并给出提示文案。
    expect(swarmToggle({ swarm: true, swarmLocked: true }, true, false)).toEqual({ checked: false, disabled: true, labelKey: 'shareSwarmLockedHint' })
  })
})

describe('source bytes', () => {
  test('direct and relay always listed; cloud and bridge only when used', () => {
    expect(sourceBytesRows({ direct: 1, relay: 2, cdn: 0, bridge: 0 }).map((row) => row.kind)).toEqual(['direct', 'relay'])
    expect(sourceBytesRows({ direct: 0, relay: 0, cdn: 5, bridge: 7 }).map((row) => [row.kind, row.bytes])).toEqual([
      ['direct', 0],
      ['relay', 0],
      ['cdn', 5],
      ['bridge', 7],
    ])
  })
})

describe('cloud storage', () => {
  const policy = { cloudEnabled: true, retentionTiers: [{ id: 'cloud-7d', hours: 168 }, { id: 'cloud-24h', hours: 24 }] }
  const convert = (patch: Partial<Parameters<typeof cloudConvertAvailability>[0]> = {}) =>
    cloudConvertAvailability({ share: { cloudState: 'none' }, displayStatus: 'active', policy, signedIn: true, ...patch })

  test('retention tiers sort by duration', () => {
    expect(retentionTiers(policy).map((tier) => tier.id)).toEqual(['cloud-24h', 'cloud-7d'])
    expect(retentionTiers({ retentionTiers: [] })).toEqual([])
  })

  test('convert is hidden without storage, disabled with a reason otherwise', () => {
    expect(convert({ policy: { ...policy, cloudEnabled: false } })).toEqual({ kind: 'hidden' })
    expect(convert({ signedIn: false })).toEqual({ kind: 'disabled', reasonKey: 'shareCloudNeedLogin' })
    expect(convert({ displayStatus: 'expired' })).toEqual({ kind: 'disabled', reasonKey: 'shareCloudNotActive' })
    expect(convert({ policy: { ...policy, retentionTiers: [] } })).toEqual({ kind: 'disabled', reasonKey: 'shareCloudNoTiers' })
    expect(convert()).toEqual({ kind: 'enabled' })
    expect(convert({ share: { cloudState: 'deleted' } })).toEqual({ kind: 'enabled' })
  })

  test('convert is not offered once uploading or ready; deleting is', () => {
    expect(convert({ share: { cloudState: 'uploading' } })).toEqual({ kind: 'hidden' })
    expect(convert({ share: { cloudState: 'ready' } })).toEqual({ kind: 'hidden' })
    expect(canDeleteCloudCopy({ cloudState: 'uploading' })).toBe(true)
    expect(canDeleteCloudCopy({ cloudState: 'ready' })).toBe(true)
    expect(canDeleteCloudCopy({ cloudState: 'none' })).toBe(false)
    expect(canDeleteCloudCopy({ cloudState: 'deleted' })).toBe(false)
  })

  test('state keys', () => {
    expect(CLOUD_STATE_KEYS.none).toBeNull()
    expect(CLOUD_STATE_KEYS.ready).toBe('shareCloudStateReady')
  })

  test('upload percent is clamped and safe for unknown totals', () => {
    expect(uploadPercent({ doneBytes: 50, totalBytes: 200 })).toBe(25)
    expect(uploadPercent({ doneBytes: 5, totalBytes: 0 })).toBe(0)
    expect(uploadPercent({ doneBytes: 300, totalBytes: 200 })).toBe(100)
    expect(uploadPercent({ doneBytes: 199, totalBytes: 200 })).toBe(99)
  })

  test('upload text follows the status', () => {
    const base = { doneBytes: 512, totalBytes: 1024, rateBps: 2048 }
    expect(uploadProgressText(t, { ...base, status: 'uploading' })).toBe('512 B / 1.0 KB (50%) · 2.0 KB/s')
    expect(uploadProgressText(t, { ...base, status: 'paused' })).toBe('512 B / 1.0 KB (50%) paused')
    expect(uploadProgressText(t, { ...base, status: 'failed' })).toBe('512 B / 1.0 KB (50%) failed')
  })

  test('reason text key resolves typed reasons only', () => {
    expect(reasonTextKey('shareTrafficExhausted')).toBe('errReasonShareTrafficExhausted')
    expect(reasonTextKey('shareQuotaExceeded')).toBe('errReasonShareQuotaExceeded')
    expect(reasonTextKey(undefined)).toBeNull()
    expect(reasonTextKey(null)).toBeNull()
  })

  test('retry reuses the share tier only when it is a cloud tier', () => {
    expect(cloudRetryTierId({ tierId: 'cloud-7d' }, policy.retentionTiers)).toBe('cloud-7d')
    expect(cloudRetryTierId({ tierId: '24h' }, policy.retentionTiers)).toBeUndefined()
  })
})

describe('batch share actions', () => {
  const stopped = share({ id: 'stopped', status: 'stopped' })
  const expired = share({ id: 'expired', status: 'expired' })
  const active = share({ id: 'active', status: 'active' })
  const missing = share({ id: 'missing', status: 'stopped', sourceMissing: true })
  const missingCloud = share({ id: 'missing-cloud', status: 'stopped', sourceMissing: true, cloudState: 'ready' })
  const activeMissing = share({ id: 'active-missing', status: 'active', sourceMissing: true })
  const activeMissingCloud = share({ id: 'active-missing-cloud', status: 'active', sourceMissing: true, cloudState: 'ready' })
  const unknown = share({ id: 'unknown', status: 'unknown' })

  test('start needs a stopped / expired share whose source is still there', () => {
    expect(canStartShare(stopped)).toBe(true)
    expect(canStartShare(expired)).toBe(true)
    expect(canStartShare(active)).toBe(false)
    expect(canStartShare(unknown)).toBe(false)
    expect(canStartShare(missing)).toBe(false)
  })

  test('missing badge: ended shares always, active ones only without a ready cloud copy', () => {
    expect(isSourceMissing(stopped)).toBe(false)
    expect(isSourceMissing(missing)).toBe(true)
    expect(isSourceMissing(missingCloud)).toBe(true)
    expect(isSourceMissing(activeMissing)).toBe(true)
    expect(isSourceMissing(activeMissingCloud)).toBe(false)
  })

  test('a stopped share with a missing source cannot start even with a cloud copy', () => {
    expect(canStartShare(missingCloud)).toBe(false)
  })

  test('stop only applies to active shares', () => {
    expect(canStopShare(active)).toBe(true)
    expect(canStopShare(stopped)).toBe(false)
    expect(canStopShare(expired)).toBe(false)
  })

  test('plan picks applicable subsets and delete covers everything', () => {
    const plan = planShareBatch([stopped, active, missing, expired, unknown])
    expect(plan.start.map((s) => s.id)).toEqual(['stopped', 'expired'])
    expect(plan.stop.map((s) => s.id)).toEqual(['active'])
    expect(plan.remove.map((s) => s.id)).toEqual(['stopped', 'active', 'missing', 'expired', 'unknown'])
    expect(planShareBatch([missing])).toEqual({ start: [], stop: [], remove: [missing] })
  })

  test('select-all state', () => {
    expect(selectAllState(0, 3)).toBe(false)
    expect(selectAllState(2, 3)).toBe('indeterminate')
    expect(selectAllState(3, 3)).toBe(true)
    expect(selectAllState(0, 0)).toBe(false)
  })

  test('selection drops shares that are gone', () => {
    const kept = pruneSelection(new Set(['a', 'b', 'c']), [{ id: 'a' }, { id: 'c' }])
    expect([...kept]).toEqual(['a', 'c'])
  })
})
