#if FULL_FLAVOR
import FluxDomain
import FluxUI
import SwiftUI

/// Existing-task file commands use the same host session for local and remote BT tasks.
@MainActor
@Observable
final class TaskBTFilesModel {
    private(set) var selection: BtTaskFilesSelection
    private(set) var loading = false
    private(set) var saving = false
    private(set) var error: String?

    init(taskId: String) { selection = BtTaskFilesSelection(taskId: taskId) }

    func select(_ index: Int32, selected: Bool) {
        guard !saving else { return }
        selection.setSelected(index, selected: selected)
    }

    func selectAll(_ selected: Bool) {
        guard !saving, error == nil else { return }
        selection.setAllSelected(selected)
    }

    func cancel() {
        guard !saving else { return }
        selection.cancel()
    }

    func refresh(session: any HostSession) async {
        guard !loading, !saving else { return }
        loading = true
        defer { loading = false }
        do throws(HostError) {
            let result: BtTaskFiles = try await session.call(
                HostMethod.daemonTaskBtFiles, params: TaskIdParams(taskId: selection.taskId)
            )
            guard !Task.isCancelled else { return }
            selection.receive(result)
            error = nil
        } catch {
            guard !Task.isCancelled else { return }
            self.error = ErrorText.describe(error)
        }
    }

    func apply(session: any HostSession) async {
        guard selection.canApply, !saving, !loading else { return }
        saving = true
        defer { saving = false }
        let wanted = selection.selected
        do throws(HostError) {
            try await session.callVoid(
                HostMethod.daemonTaskSetBtFiles,
                params: SetBtTaskFilesParams(taskId: selection.taskId, indices: wanted.sorted())
            )
            guard !Task.isCancelled else { return }
            selection.acknowledge(wanted)
            error = nil
        } catch {
            guard !Task.isCancelled else { return }
            self.error = ErrorText.describe(error)
        }
    }

    func cleanupConflict() {
        error = ErrorText.describe(HostError(.conflict))
    }

    func cleanUnselected(session: any HostSession, expected: Set<Int32>, status: TaskStatus, isSeeding: Bool) async {
        guard selection.matchesAppliedSelection(expected) else {
            cleanupConflict()
            return
        }
        guard selection.canCleanUnselected(status: status, isSeeding: isSeeding), !saving, !loading else { return }
        saving = true
        defer { saving = false }
        do throws(HostError) {
            try await session.callVoid(
                HostMethod.daemonTaskCleanBtUnselected,
                params: SetBtTaskFilesParams(taskId: selection.taskId, indices: expected.sorted())
            )
            guard !Task.isCancelled else { return }
            error = nil
            saving = false
            await refresh(session: session)
        } catch {
            guard !Task.isCancelled else { return }
            self.error = ErrorText.describe(error)
        }
    }

    /// Poll only while this page is visible/active, including completion/append verification.
    func run(session: any HostSession) async {
        while !Task.isCancelled {
            await refresh(session: session)
            do {
                try await Task.sleep(for: .seconds(2))
            } catch {
                return
            }
        }
    }
}

struct TaskDetailFilesPage: View {
    let task: DownloadTask

    @Environment(AppContainer.self) private var container
    @Environment(\.scenePhase) private var scenePhase
    @State private var files: TaskBTFilesModel
    @State private var confirmingCleanup = false
    @State private var cleanupConfirmation: CleanupConfirmation?

    private struct CleanupConfirmation {
        let session: any HostSession
        let indices: Set<Int32>
    }

    init(task: DownloadTask) {
        self.task = task
        _files = State(initialValue: TaskBTFilesModel(taskId: task.taskId))
    }

    private var canClean: Bool {
        !container.store.state.isReadOnly && !files.saving && !files.loading
            && files.selection.canCleanUnselected(status: task.status, isSeeding: task.isSeeding)
    }

    var body: some View {
        Section {
            HStack(spacing: 12) {
                Button(L("btTaskFilesApply")) {
                    guard !container.store.state.isReadOnly else { return }
                    let session = container.session
                    Task { await files.apply(session: session) }
                }
                .buttonStyle(.borderedProminent)
                .disabled(!files.selection.canApply || files.saving || files.loading || container.store.state.isReadOnly)
                if files.selection.changed {
                    Button(L("cancel")) { files.cancel() }
                        .buttonStyle(.bordered)
                        .disabled(files.saving)
                }
                if files.saving { ProgressView() }
            }
        }
        .task(id: scenePhase) {
            guard scenePhase == .active else { return }
            await files.run(session: container.session)
        }
        .confirmationDialog(L("btTaskFilesCleanConfirm"), isPresented: $confirmingCleanup, titleVisibility: .visible) {
            Button(L("btTaskFilesClean"), role: .destructive) {
                guard let confirmation = cleanupConfirmation else { return }
                cleanupConfirmation = nil
                guard canClean,
                      ObjectIdentifier(container.session) == ObjectIdentifier(confirmation.session),
                      files.selection.matchesAppliedSelection(confirmation.indices) else {
                    files.cleanupConflict()
                    return
                }
                let editor = files
                let status = task.status
                let isSeeding = task.isSeeding
                Task {
                    await editor.cleanUnselected(
                        session: confirmation.session, expected: confirmation.indices, status: status, isSeeding: isSeeding
                    )
                }
            }
            Button(L("cancel"), role: .cancel) { cleanupConfirmation = nil }
        }

        if let error = files.error {
            Section {
                VStack(alignment: .leading, spacing: 8) {
                    Text(error).font(.footnote).foregroundStyle(.secondary)
                    Button(L("detailActivityRetry")) {
                        let session = container.session
                        Task { await files.refresh(session: session) }
                    }
                    .disabled(files.loading || files.saving)
                }
            }
        }

        if let data = files.selection.snapshot, data.metadataReady {
            Section {
                HStack(spacing: 12) {
                    Button(L("btFileSelectAll")) {
                        files.selectAll(true)
                        FluxHaptic.selection.play()
                    }
                    Button(L("deselectAll")) {
                        files.selectAll(false)
                        FluxHaptic.selection.play()
                    }
                }
                .disabled(files.saving || files.error != nil || container.store.state.isReadOnly || data.files.isEmpty)
            }
            if files.selection.selected.isEmpty {
                Section {
                    Text(L("btTaskFilesRequireSelection")).foregroundStyle(.secondary)
                }
            }
            Section {
                ForEach(data.files, id: \.index) { file in
                    fileRow(file)
                }
            }
            Section {
                Button(L("btTaskFilesClean"), role: .destructive) {
                    guard canClean else { return }
                    cleanupConfirmation = CleanupConfirmation(session: container.session, indices: files.selection.selected)
                    confirmingCleanup = true
                }
                    .disabled(!canClean)
            }
        } else if files.error == nil {
            Section {
                HStack(spacing: 12) {
                    ProgressView()
                    Text(L(files.selection.snapshot == nil ? "pluginCommonLoading" : "btTaskFilesMetadataPending"))
                        .foregroundStyle(.secondary)
                }
                .accessibilityElement(children: .combine)
            }
        }
    }

    private func fileRow(_ file: BtTaskFile) -> some View {
        let selected = files.selection.selected.contains(file.index)
        return Button {
            files.select(file.index, selected: !selected)
            FluxHaptic.selection.play()
        } label: {
            HStack(alignment: .top, spacing: 10) {
                BtSelectionCheckmark(state: selected ? .on : .off)
                VStack(alignment: .leading, spacing: 6) {
                    Text(file.path).font(.subheadline)
                    let bytes = file.downloadedBytes.map { Format.bytes(Int64(clamping: $0)).description }
                        ?? L("btTaskFilesProgressUnknown")
                    Text("\(bytes) / \(Format.bytes(file.size).description) · \(L(file.selected ? "btTaskFilesSelected" : "btTaskFilesStopped"))")
                        .font(.footnote)
                        .monospacedDigit()
                        .foregroundStyle(.secondary)
                    if let progress = file.progress {
                        ProgressView(value: progress)
                            .accessibilityLabel(file.path)
                    }
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
            .contentShape(Rectangle())
            .padding(.vertical, 4)
        }
        .buttonStyle(.plain)
        .disabled(files.saving)
        .accessibilityElement(children: .combine)
        .accessibilityAddTraits(selected ? [.isSelected] : [])
    }
}

#endif
