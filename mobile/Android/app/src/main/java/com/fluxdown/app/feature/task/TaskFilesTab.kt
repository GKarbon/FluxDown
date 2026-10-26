package com.fluxdown.app.feature.task

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.selection.toggleable
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.repeatOnLifecycle
import com.fluxdown.app.R
import com.fluxdown.app.actions.LocalTaskActions
import com.fluxdown.app.i18n.str
import com.fluxdown.app.shell.LocalAppContainer
import com.fluxdown.app.shell.hostState
import com.fluxdown.core.format.Format
import com.fluxdown.core.host.HostException
import com.fluxdown.core.model.TaskStatus
import com.fluxdown.core.protocol.BtTaskFile
import com.fluxdown.core.protocol.BtTaskFiles
import com.fluxdown.core.protocol.HostMethod
import com.fluxdown.core.protocol.SetBtTaskFiles
import com.fluxdown.core.protocol.callJson
import com.fluxdown.core.protocol.jsonObject
import com.fluxdown.fluxui.controls.FluxButton
import com.fluxdown.fluxui.controls.FluxCheckbox
import com.fluxdown.fluxui.controls.ButtonSize
import com.fluxdown.fluxui.controls.ButtonVariant
import com.fluxdown.fluxui.data.FlowSegmentUi
import com.fluxdown.fluxui.data.FlowStrip
import com.fluxdown.fluxui.data.FlowStripState
import com.fluxdown.fluxui.feedback.FluxSkeletonRows
import com.fluxdown.fluxui.overlay.FluxToastKind
import com.fluxdown.fluxui.overlay.FluxDialogButton
import com.fluxdown.fluxui.overlay.FluxDialogButtonStyle
import com.fluxdown.fluxui.overlay.FluxDialogSpec
import com.fluxdown.fluxui.overlay.LocalFluxOverlays
import com.fluxdown.fluxui.theme.FluxText
import com.fluxdown.fluxui.theme.FluxTheme
import kotlinx.coroutines.delay

/** The same RPC is used for embedded and remote hosts; this tab owns only the edit draft. */
@Composable
internal fun TaskFilesTab(taskId: String, modelOf: () -> DetailModel?) {
    val container = LocalAppContainer.current
    val state = hostState()
    val readOnly by remember(state) { derivedStateOf { state.value.isReadOnly } }
    val host by container.host.collectAsStateWithLifecycle()
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    val actions = LocalTaskActions.current
    val overlays = LocalFluxOverlays.current
    val status = modelOf()?.task?.status
    var data by remember(taskId, host) { mutableStateOf<BtTaskFiles?>(null) }
    var draft by remember(taskId, host) { mutableStateOf<Set<Int>?>(null) }
    var saveRequested by remember(taskId, host) { mutableStateOf<Set<Int>?>(null) }
    var cleanRequested by remember(taskId, host) { mutableStateOf<Set<Int>?>(null) }
    var failed by remember(taskId, host) { mutableStateOf(false) }
    var revision by remember(taskId, host) { mutableStateOf(0) }
    var watchingChange by remember(taskId, host) { mutableStateOf(false) }
    val saving = saveRequested != null || cleanRequested != null
    val cleanText = str(R.string.btTaskFilesClean)
    val cleanConfirm = str(R.string.btTaskFilesCleanConfirm)
    val cancelText = str(R.string.cancel)
    val conflictText = str(R.string.localServiceConflict)

    fun cleanAllowed(busy: Boolean, expectedSelection: Set<Int> = data?.selectedIndices.orEmpty()): Boolean = container.host.value == host && canCleanBtUnselected(
        container.store.state.value.task(taskId), data, draft ?: data?.selectedIndices.orEmpty(),
        container.store.state.value.isReadOnly, busy, failed, expectedSelection,
    )

    fun confirmClean() {
        if (!actions.guard()) return
        if (!cleanAllowed(saveRequested != null || cleanRequested != null)) {
            overlays.toast(conflictText, FluxToastKind.Error)
            return
        }
        if (overlays.hasDialog) return
        val wanted = data?.selectedIndices?.toSet() ?: return
        val expectedHost = host
        overlays.showDialog(FluxDialogSpec(
            title = cleanText,
            message = cleanConfirm,
            buttons = listOf(
                FluxDialogButton(cancelText),
                FluxDialogButton(cleanText, FluxDialogButtonStyle.Destructive) {
                    if (actions.guard()) {
                        if (container.host.value == expectedHost && cleanAllowed(saveRequested != null || cleanRequested != null, wanted)) cleanRequested = wanted
                        else overlays.toast(conflictText, FluxToastKind.Error)
                    }
                },
            ),
        ))
    }

    LaunchedEffect(taskId, host, status, revision, saving) {
        if (saving) return@LaunchedEffect
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            do {
                try {
                    val next = BtTaskFiles.fromJson(container.session.callJson(HostMethod.daemonTaskBtFiles, jsonObject("taskId" to taskId)))
                    if (next?.taskId == taskId) {
                        data = next
                        failed = false
                        if (next.files.filter { it.selected }.all { file -> file.downloadedBytes?.let { it >= file.size } == true }) watchingChange = false
                    } else failed = true
                } catch (_: HostException) {
                    failed = true
                }
                val poll = status == TaskStatus.Downloading || data?.metadataReady == false ||
                    (status == TaskStatus.Completed && watchingChange)
                if (!poll) break
                delay(2_000)
            } while (true)
        }
    }

    LaunchedEffect(taskId, host, saveRequested) {
        val indices = saveRequested ?: return@LaunchedEffect
        if (!actions.guard() || cleanRequested != null || indices.isEmpty()) {
            saveRequested = null
            return@LaunchedEffect
        }
        try {
            container.session.callJson(HostMethod.daemonTaskSetBtFiles, SetBtTaskFiles(taskId, indices).toJson())
            val next = BtTaskFiles.fromJson(container.session.callJson(HostMethod.daemonTaskBtFiles, jsonObject("taskId" to taskId)))
            if (next?.taskId == taskId) {
                data = next
                draft = null
                failed = false
            } else failed = true
            watchingChange = true
            revision += 1
        } catch (error: HostException) {
            overlays.toast(actions.errorText(error), FluxToastKind.Error)
        } finally {
            saveRequested = null
        }
    }

    LaunchedEffect(taskId, host, cleanRequested) {
        val wanted = cleanRequested ?: return@LaunchedEffect
        if (!actions.guard()) {
            cleanRequested = null
            return@LaunchedEffect
        }
        if (!cleanAllowed(saveRequested != null, wanted)) {
            overlays.toast(conflictText, FluxToastKind.Error)
            cleanRequested = null
            return@LaunchedEffect
        }
        try {
            container.session.callJson(HostMethod.daemonTaskCleanBtUnselected, SetBtTaskFiles(taskId, wanted).toJson())
            val next = BtTaskFiles.fromJson(container.session.callJson(HostMethod.daemonTaskBtFiles, jsonObject("taskId" to taskId)))
            if (next?.taskId == taskId) {
                data = next
                failed = false
            } else failed = true
            revision += 1
        } catch (error: HostException) {
            overlays.toast(actions.errorText(error), FluxToastKind.Error)
        } finally {
            cleanRequested = null
        }
    }

    val current = data
    val selected = draft ?: current?.selectedIndices.orEmpty()
    if (failed) {
        FluxText(str(R.string.btTaskFilesReadError), style = FluxTheme.type.sm, color = FluxTheme.colors.inkMuted)
        FluxButton(str(R.string.detailActivityRetry), { revision += 1 }, enabled = !saving)
    }
    when {
        current == null && !failed -> FluxSkeletonRows(rows = 3, loadingLabel = str(R.string.pluginCommonLoading))
        current != null && !current.metadataReady -> FluxText(str(R.string.btTaskFilesMetadataPending), style = FluxTheme.type.body)
        current != null -> {
            val changed = selected != current.selectedIndices
            Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                val editable = !saving && !failed && !readOnly && current.files.isNotEmpty()
                FluxButton(
                    str(R.string.btFileSelectAll), { draft = current.files.mapTo(linkedSetOf()) { it.index } },
                    variant = ButtonVariant.Ghost, size = ButtonSize.Xs, enabled = editable,
                )
                FluxButton(
                    str(R.string.deselectAll), { draft = emptySet() },
                    variant = ButtonVariant.Ghost, size = ButtonSize.Xs, enabled = editable,
                )
            }
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                FluxButton(
                    str(R.string.btTaskFilesApply),
                    { if (selected.isNotEmpty() && saveRequested == null && cleanRequested == null && actions.guard()) saveRequested = selected.toSet() },
                    enabled = changed && selected.isNotEmpty() && !saving && !failed && !readOnly,
                    loading = saveRequested != null,
                )
                if (changed) FluxButton(str(R.string.cancel), { draft = null }, enabled = !saving)
            }
            FluxButton(
                cleanText, ::confirmClean, variant = ButtonVariant.Danger, fullWidth = true,
                enabled = canCleanBtUnselected(modelOf()?.task, current, selected, readOnly, saving, failed),
                loading = cleanRequested != null,
            )
            if (selected.isEmpty()) FluxText(str(R.string.btTaskFilesRequireSelection), style = FluxTheme.type.sm, color = FluxTheme.colors.inkMuted)
            LazyColumn(Modifier.fillMaxWidth().heightIn(max = 520.dp)) {
                items(current.files, key = { it.index }) { file ->
                    TaskFileRow(file, selected.contains(file.index), !saving && !readOnly, modelOf()?.flowState ?: FlowStripState.Paused) { checked ->
                        draft = if (checked) selected + file.index else selected - file.index
                    }
                }
            }
        }
    }
}

@Composable
private fun TaskFileRow(file: BtTaskFile, checked: Boolean, enabled: Boolean, flowState: FlowStripState, onCheckedChange: (Boolean) -> Unit) {
    Row(
        Modifier.fillMaxWidth()
            .toggleable(value = checked, enabled = enabled, role = Role.Checkbox, onValueChange = onCheckedChange)
            .padding(vertical = 12.dp),
        horizontalArrangement = Arrangement.spacedBy(12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        FluxCheckbox(checked, onCheckedChange = null, modifier = Modifier.clearAndSetSemantics { })
        Column(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(6.dp)) {
            FluxText(file.path, style = FluxTheme.type.body)
            val progress = file.downloadedBytes?.let { Format.bytes(it).toString() } ?: str(R.string.btTaskFilesProgressUnknown)
            val wanted = str(if (file.selected) R.string.btTaskFilesSelected else R.string.btTaskFilesStopped)
            FluxText("$progress / ${Format.bytes(file.size)} · $wanted", style = FluxTheme.type.sm, color = FluxTheme.colors.inkMuted)
            file.downloadedBytes?.let { bytes ->
                val fraction = if (file.size == 0L) 1f else (bytes.toFloat() / file.size).coerceIn(0f, 1f)
                val state = when {
                    !file.selected -> FlowStripState.Paused
                    bytes >= file.size -> FlowStripState.Completed
                    else -> flowState
                }
                FlowStrip(listOf(FlowSegmentUi(1f, fraction)), state, semanticsLabel = file.path)
            }
        }
    }
}
