// 「额度与贡献」页签（GPUI `crates/share/src/page/usage.rs` 的 Web 对应）：
// 三条额度条、额度明细、贡献与等级、购买入口。可见才取数（agent 侧 30 s 缓存）。

import { useNavigate } from '@tanstack/react-router'
import { RefreshCw, UserRound } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useT } from '../../i18n'
import { cn } from '../../lib/cn'
import { rpc } from '../../lib/rpc'
import type { ShareEarnDto, ShareUsageDto } from '../../lib/rpc'
import { Badge, Button, Card, EmptyState, Icon, ProgressBar } from '../../ui'
import { requestPlanDialog } from '../settings/sections/account/planRequest'
import type { PlanDialogTab } from '../settings/sections/account/planRequest'
import { accountErrorKey } from '../settings/sections/account/errorText'
import { formatShortTime } from '../webhooks/cloudLogic'
import { boostLabel, formatQuotaBytes, grantSourceKey, levelLabel, trafficKindKey, usageMeters } from './model'
import type { MeterView } from './model'
import { useShareSignedIn, useShareState } from './state'

function Meter({ meter, resetAt }: { meter: MeterView; resetAt: string }) {
  const t = useT()
  const reset = formatShortTime(resetAt)
  const text = meter.unlimited
    ? t('shareUsageUnlimited')
    : meter.exhausted
      ? t('shareUsageExhausted', { time: reset })
      : t('shareUsageRemaining', { remaining: formatQuotaBytes(meter.remaining), allowance: formatQuotaBytes(meter.allowance) })
  return (
    <div id={`share-usage-meter-${meter.kind}`} className="flex min-w-0 flex-col gap-1">
      <div className="flex flex-wrap items-baseline justify-between gap-x-2 text-xs">
        <span className={cn('font-medium', meter.exhausted ? 'text-destructive' : 'text-foreground')}>{t(meter.labelKey)}</span>
        <span className={cn('tabular break-words', meter.exhausted ? 'text-destructive' : 'text-muted-foreground')}>{text}</span>
      </div>
      {meter.unlimited ? null : <ProgressBar value={meter.ratio} className={cn(meter.exhausted && '[&>div]:!bg-destructive')} />}
    </div>
  )
}

function earnLine(t: ReturnType<typeof useT>, earn: ShareEarnDto): string | null {
  const key = trafficKindKey(earn.kind)
  if (!key) return null
  const kind = t(key)
  const earned = formatQuotaBytes(earn.earnedThisMonth)
  return earn.capBytes > 0
    ? t('shareUsageEarned', { kind, earned, cap: formatQuotaBytes(earn.capBytes), months: earn.validMonths })
    : t('shareUsageEarnedNoCap', { kind, earned })
}

function Body({ usage, failed, onRefresh, refreshing }: { usage: ShareUsageDto; failed: boolean; onRefresh: () => void; refreshing: boolean }) {
  const t = useT()
  const navigate = useNavigate()
  const meters = usageMeters(usage)
  const c = usage.contribution
  const boost = boostLabel(c.boostPermille)
  const go = (tab: PlanDialogTab) => {
    requestPlanDialog(tab)
    void navigate({ to: '/settings/$category', params: { category: 'account' } })
  }
  const priority = !c.priorityActive
    ? null
    : c.priority
      ? t('shareUsagePriorityOn')
      : c.priorityThresholdBytes > 0
        ? t('shareUsagePriorityOffContrib', { bytes: formatQuotaBytes(c.priorityThresholdBytes), days: c.priorityWindowDays })
        : t('shareUsagePriorityOffPlan')
  const earnLines = c.earn.map((item) => earnLine(t, item)).filter((line): line is string => line !== null)

  return (
    <div className="flex min-w-0 flex-col gap-4">
      <Card className="flex min-w-0 flex-col gap-3 p-4">
        <div className="flex items-center justify-between gap-2">
          <span className="min-w-0 text-sm font-semibold text-foreground">{t('shareUsageMonth', { month: usage.month, time: formatShortTime(usage.resetAt) })}</span>
          <Button id="share-usage-refresh" variant="ghost" iconOnly aria-label={t('accountCloudRefresh')} loading={refreshing} onClick={onRefresh}>
            <Icon icon={RefreshCw} />
          </Button>
        </div>
        {usage.enforced.swarm ? null : <span className="text-xs text-text-tertiary">{t('shareUsageNotEnforced')}</span>}
        {failed ? <span className="text-xs text-destructive">{t('shareUsageLoadFailed')}</span> : null}
        {meters.map((meter) => (
          <Meter key={meter.kind} meter={meter} resetAt={usage.resetAt} />
        ))}
      </Card>

      {usage.grants.length > 0 ? (
        <Card className="flex min-w-0 flex-col gap-2 p-4">
          <span className="text-sm font-semibold text-foreground">{t('shareUsageGrantsTitle')}</span>
          <div className="overflow-x-auto">
            <table className="w-full min-w-[420px] text-left text-xs">
              <thead className="text-text-tertiary">
                <tr>
                  <th className="py-1 pr-3 font-normal">{t('shareUsageColSource')}</th>
                  <th className="py-1 pr-3 font-normal">{t('shareUsageColKind')}</th>
                  <th className="py-1 pr-3 font-normal">{t('shareUsageColRemaining')}</th>
                  <th className="py-1 font-normal">{t('shareUsageColExpires')}</th>
                </tr>
              </thead>
              <tbody className="text-foreground">
                {usage.grants.map((grant, index) => {
                  const kindKey = trafficKindKey(grant.kind)
                  return (
                    <tr key={index} className="border-t border-hairline">
                      <td className="py-1.5 pr-3">{t(grantSourceKey(grant.source))}</td>
                      <td className="py-1.5 pr-3">{kindKey ? t(kindKey) : '—'}</td>
                      <td className="py-1.5 pr-3 tabular">{formatQuotaBytes(Math.max(0, grant.bytes - grant.usedBytes))}</td>
                      <td className="py-1.5 tabular">{grant.expiresAt ? formatShortTime(grant.expiresAt) : t('shareUsageGrantNoExpiry')}</td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        </Card>
      ) : null}

      <Card className="flex min-w-0 flex-col gap-2 p-4">
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-sm font-semibold text-foreground">{t('shareUsageContributionTitle')}</span>
          <Badge tone="accent">{levelLabel(c.level, c.levelMax)}</Badge>
          {c.nextLevelBytes !== null && c.level < c.levelMax ? (
            <span className="text-xs text-muted-foreground tabular">{t('shareUsageNextLevel', { bytes: formatQuotaBytes(Math.max(0, c.nextLevelBytes - c.lifetimeBytes)) })}</span>
          ) : null}
        </div>
        <span className="text-xs text-muted-foreground tabular">{t('shareUsageContribMonth', { bytes: formatQuotaBytes(c.monthBytes) })}</span>
        {boost ? <span className="text-xs text-muted-foreground">{t('shareUsageBoost', { boost })}</span> : null}
        {earnLines.map((line) => (
          <span key={line} className="text-xs text-muted-foreground tabular break-words">
            {line}
          </span>
        ))}
        {priority ? <span className="text-xs text-muted-foreground">{priority}</span> : null}
      </Card>

      <div className="flex flex-wrap gap-2">
        <Button id="share-usage-buy-pack" onClick={() => go('packs')}>
          {t('shareUsageBuyPack')}
        </Button>
        <Button id="share-usage-upgrade" variant="primary" onClick={() => go('membership')}>
          {t('shareUsageUpgrade')}
        </Button>
      </div>
    </div>
  )
}

export function UsageTab() {
  const t = useT()
  const navigate = useNavigate()
  const signedIn = useShareSignedIn()
  const { usage, usageError } = useShareState()
  const [refreshing, setRefreshing] = useState(false)
  const [nonce, setNonce] = useState(0)

  // 可见才取数；结果经 `shareChanged` 推到快照，这里只触发。
  useEffect(() => {
    if (!signedIn) return
    let cancelled = false
    setRefreshing(true)
    rpc.agent.share
      .usageRefresh()
      .catch((error: unknown) => console.warn('[share] usageRefresh failed', accountErrorKey(error), error))
      .finally(() => {
        if (!cancelled) setRefreshing(false)
      })
    return () => {
      cancelled = true
    }
  }, [signedIn, nonce])

  if (!signedIn) {
    return (
      <EmptyState
        icon={UserRound}
        title={t('shareUsageLogin')}
        action={
          <Button id="share-usage-login" variant="primary" onClick={() => void navigate({ to: '/settings/$category', params: { category: 'account' } })}>
            {t('accountLogin')}
          </Button>
        }
      />
    )
  }
  if (!usage) {
    return <EmptyState icon={UserRound} title={usageError ? t('shareUsageLoadFailed') : ''} />
  }
  return <Body usage={usage} failed={usageError != null} refreshing={refreshing} onRefresh={() => setNonce((n) => n + 1)} />
}
