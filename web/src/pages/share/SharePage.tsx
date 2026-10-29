// FluxMesh页（活动栏路由 `/share`，可隐藏）：顶部 6 位取件码 / 分享链接输入框，下方三个页签
// [我的分享 | 接收记录 | 额度与贡献 | 网络诊断]。对应 GPUI `crates/share` 的管理页。

import { useSearch } from '@tanstack/react-router'
import { Share2 } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useT } from '../../i18n'
import { Button, EmptyState, FieldError, Input, SegmentedTabs } from '../../ui'
import { MyShares } from './MyShares'
import { NetworkDiagnostics } from './NetworkDiagnostics'
import { ReceiveRecords } from './ReceiveRecords'
import { UsageTab } from './UsageTab'
import { formatPickupCode, parseShareInput } from './model'
import { useShareAvailable } from './state'
import { openShareReceive } from './store'

type Tab = 'mine' | 'received' | 'usage' | 'network'

/** 取件码框：纯数字时按 `123 456` 分组显示；链接原样保留。 */
function displayInput(value: string): string {
  return /^[\d\s-]*$/.test(value) ? formatPickupCode(value.replace(/\D/g, '').slice(0, 6)) : value
}

function PickupBox() {
  const t = useT()
  const [value, setValue] = useState('')
  const [invalid, setInvalid] = useState(false)

  const submit = () => {
    const input = parseShareInput(value)
    if (!input) {
      setInvalid(true)
      return
    }
    setInvalid(false)
    openShareReceive(input.value)
    setValue('')
  }

  return (
    <form
      className="flex min-w-0 flex-col gap-1.5"
      onSubmit={(event) => {
        event.preventDefault()
        submit()
      }}
    >
      <div className="flex min-w-0 gap-2">
        <Input
          value={value}
          invalid={invalid}
          inputMode="text"
          autoComplete="off"
          spellCheck={false}
          placeholder={t('sharePickupPlaceholder')}
          aria-label={t('sharePickupPlaceholder')}
          className="min-w-0 flex-1 tabular"
          onChange={(event) => {
            setInvalid(false)
            setValue(displayInput(event.target.value))
          }}
        />
        <Button type="submit" variant="primary" disabled={value.trim() === ''}>
          {t('shareReceive')}
        </Button>
      </div>
      {invalid ? <FieldError>{t('shareInputInvalid')}</FieldError> : null}
    </form>
  )
}

export function SharePage() {
  const t = useT()
  const available = useShareAvailable()
  const search = useSearch({ strict: false }) as { tab?: Tab }
  const [tab, setTab] = useState<Tab>(search.tab ?? 'mine')
  useEffect(() => {
    if (search.tab) setTab(search.tab)
  }, [search.tab])

  return (
    <div className="flex h-full min-h-0 min-w-0 flex-col bg-surface text-foreground">
      <header className="flex flex-none flex-col gap-3 border-b border-hairline px-6 pt-4 pb-3 mobile:px-4">
        <h1 className="text-title font-semibold text-foreground">{t('shareNavTitle')}</h1>
        {available ? (
          <>
            <PickupBox />
            <SegmentedTabs
              aria-label={t('shareNavTitle')}
              value={tab}
              onValueChange={setTab}
              className="self-start"
              items={[
                { value: 'mine', label: t('shareTabMine') },
                { value: 'received', label: t('shareTabReceived') },
                { value: 'usage', label: t('shareTabUsage') },
                { value: 'network', label: t('shareTabNetwork') },
              ]}
            />
          </>
        ) : null}
      </header>
      <div className="min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-6 pt-5 pb-6 mobile:px-4 mobile:pt-4">
        <div className="mx-auto flex w-full min-w-0 max-w-5xl flex-col gap-4">
          {!available ? (
            <EmptyState icon={Share2} title={t('errReasonShareUnavailable')} />
          ) : tab === 'mine' ? (
            <MyShares />
          ) : tab === 'received' ? (
            <ReceiveRecords />
          ) : tab === 'usage' ? (
            <UsageTab />
          ) : (
            <NetworkDiagnostics />
          )}
        </div>
      </div>
    </div>
  )
}
