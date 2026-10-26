package com.fluxdown.app.feature.task

import com.fluxdown.core.model.SeedingStatus
import com.fluxdown.core.model.Task
import com.fluxdown.core.model.TaskStatus
import com.fluxdown.core.protocol.BtTaskFile
import com.fluxdown.core.protocol.BtTaskFiles
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TaskFilesActionsTest {
    private val files = BtTaskFiles("t", true, listOf(
        BtTaskFile(0, "selected", 10, 5, true),
        BtTaskFile(2, "unselected", 10, 3, false),
    ))

    private fun task(status: TaskStatus, seeding: SeedingStatus = SeedingStatus.None) = Task(
        taskId = "t", url = "magnet:?xt=1", originUrl = "", fileName = "bt", saveDir = "/d",
        status = status, downloadedBytes = 8, totalBytes = 10, seedingStatus = seeding,
    )

    private fun allowed(task: Task = task(TaskStatus.Paused), data: BtTaskFiles = files, selected: Set<Int> = setOf(0),
                        readOnly: Boolean = false, busy: Boolean = false, failed: Boolean = false) =
        canCleanBtUnselected(task, data, selected, readOnly, busy, failed)

    @Test fun onlyStoppedTasksCanClean() {
        assertTrue(allowed(task(TaskStatus.Paused)))
        assertTrue(allowed(task(TaskStatus.Failed)))
        assertTrue(allowed(task(TaskStatus.Completed)))
        assertFalse(allowed(task(TaskStatus.Pending)))
        assertFalse(allowed(task(TaskStatus.Downloading)))
        assertFalse(allowed(task(TaskStatus.Preparing)))
        assertFalse(allowed(task(TaskStatus.Unknown)))
    }

    @Test fun activeOrQueuedSeedersCannotClean() {
        assertFalse(allowed(task(TaskStatus.Completed, SeedingStatus.Seeding)))
        assertFalse(allowed(task(TaskStatus.Completed, SeedingStatus.QueuedForSlot)))
        assertTrue(allowed(task(TaskStatus.Completed, SeedingStatus.UserStopped)))
    }

    @Test fun readOnlyBusyUnsavedOrUnavailableDetailsCannotClean() {
        assertFalse(allowed(readOnly = true))
        assertFalse(allowed(busy = true))
        assertFalse(allowed(failed = true))
        assertFalse(allowed(selected = setOf(0, 2)))
        assertFalse(allowed(data = files.copy(metadataReady = false)))
        assertFalse(allowed(data = files.copy(files = files.files.map { it.copy(selected = true) }), selected = setOf(0, 2)))
        assertFalse(allowed(task = task(TaskStatus.Paused).copy(url = "https://example.test/file")))
        assertFalse(allowed(data = files.copy(taskId = "other")))
    }

    @Test fun confirmationCannotSilentlyCleanADifferentAppliedRange() {
        val wantedAtConfirmation = files.selectedIndices.toSet()
        val changed = files.copy(files = files.files.map { it.copy(selected = !it.selected) })
        // A valid new range with no draft still requires a fresh destructive confirmation.
        assertTrue(allowed(data = changed, selected = setOf(2)))
        assertFalse(canCleanBtUnselected(task(TaskStatus.Paused), changed, setOf(2), false, false, false, wantedAtConfirmation))
        assertTrue(canCleanBtUnselected(task(TaskStatus.Paused), files, setOf(0), false, false, false, wantedAtConfirmation))
    }
}
