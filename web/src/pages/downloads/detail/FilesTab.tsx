import { useEffect, useRef, useState } from 'react'
import { useT } from '../../../i18n'
import { rpc, useConnection } from '../../../lib/rpc'
import type { BtTaskFilesDto } from '../../../lib/rpc'
import { toastRpcError } from '../../../lib/rpcToast'
import { Button, Checkbox, Dialog, ConfirmFooter } from '../../../ui'
import { formatBytes } from '../model/task'
import type { DownloadTaskView } from '../model/task'
import { canCleanBtFiles, sameFileSelection } from './fileActions'

export function FilesTab({ view }: { view: DownloadTaskView }) {
  const t = useT()
  const online = useConnection().phase === 'ready'
  const [data, setData] = useState<BtTaskFilesDto | null>(null)
  const [draft, setDraft] = useState<number[] | null>(null)
  const [saving, setSaving] = useState(false)
  const [watchingChange, setWatchingChange] = useState(false)
  const [failed, setFailed] = useState(false)
  const [revision, setRevision] = useState(0)
  const [cleanRange, setCleanRange] = useState<number[] | null>(null)
  const savingRef = useRef(false)
  useEffect(() => {
    let alive = true
    let inflight = false
    const load = () => {
      if (document.hidden || inflight || savingRef.current) return
      inflight = true
      rpc.daemon.task.btFiles({ taskId: view.taskId }).then(
        (next) => { if (alive && next.taskId === view.taskId) {
          setData(next); setFailed(false)
          if (view.state !== 'completed' || next.files.every((f) => !f.selected || (f.downloadedBytes !== null && f.downloadedBytes >= f.size))) setWatchingChange(false)
        } },
        () => { if (alive) setFailed(true) },
      ).finally(() => { inflight = false })
    }
    load()
    const timer = view.state === 'downloading' || !data?.metadataReady || watchingChange ? setInterval(load, 2000) : null
    document.addEventListener('visibilitychange', load)
    return () => { alive = false; if (timer !== null) clearInterval(timer); document.removeEventListener('visibilitychange', load) }
  }, [view.taskId, view.state, data?.metadataReady, revision, watchingChange])
  const selected = draft ?? data?.files.filter((f) => f.selected).map((f) => f.index) ?? []
  const changed = data?.files.some((f) => f.selected !== selected.includes(f.index)) ?? false
  const canClean = canCleanBtFiles(view, data, selected, online, saving, failed)
  const cleanRangeCurrent = cleanRange !== null && sameFileSelection(cleanRange, selected)
  const clean = async () => {
    if (!canClean || !cleanRangeCurrent || cleanRange === null || savingRef.current) return
    savingRef.current = true
    setSaving(true)
    try {
      await rpc.daemon.task.cleanBtUnselected({ taskId: view.taskId, indices: cleanRange })
      setData(await rpc.daemon.task.btFiles({ taskId: view.taskId }))
      setFailed(false)
      setCleanRange(null)
    } catch (error) { toastRpcError(error); setRevision((n) => n + 1) }
    finally { savingRef.current = false; setSaving(false) }
  }
  const save = async () => {
    if (selected.length === 0 || savingRef.current || !online) return
    savingRef.current = true
    setSaving(true)
    try {
      await rpc.daemon.task.setBtFiles({ taskId: view.taskId, indices: selected })
      setWatchingChange(true)
      const next = await rpc.daemon.task.btFiles({ taskId: view.taskId })
      setData(next)
      setDraft(null)
      setFailed(false)
    } catch (error) { toastRpcError(error) }
    finally { savingRef.current = false; setSaving(false) }
  }
  return <div className="flex min-w-0 flex-col gap-3">
    {failed ? <div role="alert" className="text-sm text-text-tertiary">{t('btTaskFilesReadError')} <Button variant="ghost" onClick={() => setRevision((n) => n + 1)}>{t('detailActivityRetry')}</Button></div> : null}
    {!data || !data.metadataReady ? <p role="status" className="text-sm text-text-tertiary">{t('btTaskFilesMetadataPending')}</p> : <>
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="ghost" disabled={saving || !online || failed || data.files.length === 0} onClick={() => setDraft(data.files.map((file) => file.index))}>{t('btFileSelectAll')}</Button>
        <Button variant="ghost" disabled={saving || !online || failed || data.files.length === 0} onClick={() => setDraft([])}>{t('deselectAll')}</Button>
        <Button disabled={!changed || saving || selected.length === 0 || !online} onClick={() => { void save() }}>{t('btTaskFilesApply')}</Button>
        <Button variant="ghost" disabled={!canClean} onClick={() => setCleanRange([...selected])}>{t('btTaskFilesClean')}</Button>
        {changed ? <Button variant="ghost" disabled={saving} onClick={() => setDraft(null)}>{t('cancel')}</Button> : null}
      </div>
      {selected.length === 0 ? <p role="status" className="text-caption text-text-tertiary">{t('btTaskFilesRequireSelection')}</p> : null}
      <ul className="divide-y divide-hairline">
        {data.files.map((file) => <li key={file.index} className="flex min-w-0 items-start gap-3 py-3">
          <Checkbox aria-label={file.path} checked={selected.includes(file.index)} disabled={saving || !online} onCheckedChange={(checked) => setDraft(checked ? [...selected, file.index] : selected.filter((i) => i !== file.index))} />
          <div className="min-w-0 flex-1">
            <div className="break-all text-sm" title={file.path}>{file.path}</div>
            <div className="mt-1 text-caption tabular-nums text-text-tertiary">{file.downloadedBytes === null ? t('btTaskFilesProgressUnknown') : formatBytes(file.downloadedBytes)} / {formatBytes(file.size)} · {file.selected ? t('btTaskFilesSelected') : t('btTaskFilesStopped')}</div>
            {file.downloadedBytes !== null ? <progress className="mt-1 h-1 w-full" aria-label={file.path} value={file.size === 0 ? 1 : file.downloadedBytes} max={Math.max(1, file.size)} /> : null}
          </div>
        </li>)}
      </ul>
    </>}
    <Dialog open={cleanRange !== null} onOpenChange={(open) => { if (!open) setCleanRange(null) }} title={t('btTaskFilesClean')}
      footer={<ConfirmFooter intent="destructive" okLabel={t('btTaskFilesClean')} okDisabled={!canClean || !cleanRangeCurrent} loading={saving} onCancel={() => setCleanRange(null)} onOk={() => { void clean() }} />}>
      <p>{t('btTaskFilesCleanConfirm')}</p>
      <ul>{data?.files.filter((file) => !file.selected).map((file) => <li key={file.index}>{file.path}</li>)}</ul>
    </Dialog>
  </div>
}
