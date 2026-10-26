package com.fluxdown.app.feature.task

import com.fluxdown.core.model.Task
import com.fluxdown.core.model.TaskProtocol
import com.fluxdown.core.model.TaskStatus
import com.fluxdown.core.protocol.BtTaskFiles

/** Rechecked after confirmation: the task or connection may change while the dialog is open. */
internal fun canCleanBtUnselected(
    task: Task?,
    files: BtTaskFiles?,
    selected: Set<Int>,
    readOnly: Boolean,
    busy: Boolean,
    failed: Boolean,
    expectedSelection: Set<Int> = selected,
): Boolean {
    if (task == null || files == null) return false
    val stopped = task.status == TaskStatus.Paused || task.status == TaskStatus.Failed || task.status == TaskStatus.Completed
    return task.taskId == files.taskId && task.protocol == TaskProtocol.Bt && stopped && !task.isSeeding &&
        files.metadataReady && files.files.any { !it.selected } && selected == files.selectedIndices &&
        expectedSelection == files.selectedIndices &&
        !readOnly && !busy && !failed
}
