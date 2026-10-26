import type { BtTaskFilesDto } from '../../../lib/rpc'
import type { DownloadTaskView } from '../model/task'

export function sameFileSelection(left: readonly number[], right: readonly number[]): boolean {
  return left.length === right.length && left.every((index) => right.includes(index))
}

/** Cleanup uses the acknowledged range, never an uncommitted checkbox draft. */
export function canCleanBtFiles(
  view: Pick<DownloadTaskView, 'taskId' | 'state' | 'seedingStatus' | 'runtimeConnected'>,
  data: BtTaskFilesDto | null,
  selected: readonly number[],
  online: boolean,
  busy: boolean,
  failed: boolean,
): boolean {
  return online && view.runtimeConnected && !busy && !failed
    && ['paused', 'failed', 'completed'].includes(view.state)
    && ![1, 8].includes(view.seedingStatus)
    && data?.taskId === view.taskId && data.metadataReady
    && data.files.some((file) => !file.selected)
    && sameFileSelection(selected, data.files.filter((file) => file.selected).map((file) => file.index))
}
