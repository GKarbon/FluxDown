import { describe, expect, test } from 'bun:test'
import type { BtTaskFilesDto } from '../../../lib/rpc'
import { canCleanBtFiles, sameFileSelection } from './fileActions'

const view = { taskId: 't', state: 'paused' as const, seedingStatus: 0, runtimeConnected: true }
const files: BtTaskFilesDto = { taskId: 't', metadataReady: true, files: [
  { index: 0, path: 'a', size: 10, downloadedBytes: 5, selected: true },
  { index: 2, path: 'b', size: 20, downloadedBytes: null, selected: false },
] }
const allowed = (task = view, data = files, selected = [0], online = true, busy = false, failed = false) =>
  canCleanBtFiles(task, data, selected, online, busy, failed)

describe('stopped torrent cleanup', () => {
  test('keeps zero-choice drafts and busy or disconnected pages from deleting data', () => {
    expect(allowed()).toBe(true)
    expect(allowed(view, files, [])).toBe(false)
    expect(allowed(view, files, [0, 2])).toBe(false)
    expect(allowed({ ...view, runtimeConnected: false })).toBe(false)
    expect(allowed(view, files, [0], false)).toBe(false)
    expect(allowed(view, files, [0], true, true)).toBe(false)
    expect(allowed(view, files, [0], true, false, true)).toBe(false)
    expect(allowed(view, { ...files, taskId: 'another' })).toBe(false)
    expect(allowed(view, { ...files, metadataReady: false })).toBe(false)
  })
  test('rejects running tasks and active or queued seeders', () => {
    for (const state of ['downloading', 'pending'] as const) {
      expect(canCleanBtFiles({ ...view, state }, files, [0], true, false, false)).toBe(false)
    }
    for (const seedingStatus of [1, 8]) expect(allowed({ ...view, seedingStatus })).toBe(false)
  })
  test('confirmation remains valid through progress updates, but not a changed range', () => {
    expect(sameFileSelection([0, 2], [2, 0])).toBe(true)
    expect(sameFileSelection([0], [2])).toBe(false)
    expect(sameFileSelection([0], [0, 2])).toBe(false)
  })
})
