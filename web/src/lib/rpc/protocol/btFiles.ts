/** Mirrors native/protocol/src/bt_files.rs. Unknown verified bytes remain null. */
export interface BtTaskFileDto {
  index: number
  path: string
  size: number
  downloadedBytes: number | null
  selected: boolean
}
export interface BtTaskFilesDto {
  taskId: string
  metadataReady: boolean
  files: BtTaskFileDto[]
}
export interface SetBtTaskFilesParams {
  taskId: string
  indices: number[]
}
