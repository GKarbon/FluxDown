import Foundation

/// Existing-task BT files. Indices retain gaps left by padding files; unknown verified progress is nil.
public struct BtTaskFile: Codable, Sendable, Hashable {
    public var index: Int32
    public var path: String
    public var size: Int64
    public var downloadedBytes: UInt64?
    public var selected: Bool

    public init(index: Int32, path: String, size: Int64, downloadedBytes: UInt64?, selected: Bool) {
        self.index = index
        self.path = path
        self.size = size
        self.downloadedBytes = downloadedBytes
        self.selected = selected
    }

    /// Zero-byte files are complete only after local verification has supplied a known value.
    public var progress: Double? {
        guard let downloadedBytes else { return nil }
        return size == 0 ? 1 : min(max(Double(downloadedBytes) / Double(size), 0), 1)
    }
}

public struct BtTaskFiles: Codable, Sendable, Hashable {
    public var taskId: String
    public var metadataReady: Bool
    public var files: [BtTaskFile]

    public init(taskId: String, metadataReady: Bool, files: [BtTaskFile]) {
        self.taskId = taskId
        self.metadataReady = metadataReady
        self.files = files
    }
}

public struct SetBtTaskFilesParams: Codable, Sendable, Hashable {
    public var taskId: String
    public var indices: [Int32]

    public init(taskId: String, indices: [Int32]) {
        self.taskId = taskId
        self.indices = indices
    }
}

/// A task's editable selection, kept separate from the host's last acknowledged selection/progress.
public struct BtTaskFilesSelection: Sendable, Equatable {
    public let taskId: String
    public private(set) var snapshot: BtTaskFiles?
    private var draft: Set<Int32>?

    public init(taskId: String) { self.taskId = taskId }

    public var selected: Set<Int32> { draft ?? baseline }
    public var changed: Bool { selected != baseline }
    public var canApply: Bool {
        snapshot?.metadataReady == true && changed && !selected.isEmpty
            && selected.isSubset(of: Set(snapshot?.files.map(\.index) ?? []))
    }

    /// Cleanup acts on the acknowledged selection, never on an unsaved draft or a live torrent.
    public func canCleanUnselected(status: TaskStatus, isSeeding: Bool) -> Bool {
        let stopped = status == .paused || status == .failed || status == .completed
        return stopped && !isSeeding && !changed && snapshot?.metadataReady == true
            && snapshot?.files.contains(where: { !$0.selected }) == true
    }

    /// A cleanup confirmation is valid only for the exact applied scope that the user reviewed.
    public func matchesAppliedSelection(_ expected: Set<Int32>) -> Bool {
        snapshot?.metadataReady == true && !changed && baseline == expected
    }

    private var baseline: Set<Int32> {
        Set(snapshot?.files.filter(\.selected).map(\.index) ?? [])
    }

    public mutating func receive(_ value: BtTaskFiles) {
        guard value.taskId == taskId else { return }
        snapshot = value
    }

    public mutating func setSelected(_ index: Int32, selected: Bool) {
        guard snapshot?.files.contains(where: { $0.index == index }) == true else { return }
        var next = self.selected
        if selected { next.insert(index) } else { next.remove(index) }
        draft = next
    }

    public mutating func setAllSelected(_ selected: Bool) {
        guard let snapshot, snapshot.metadataReady else { return }
        draft = selected ? Set(snapshot.files.map(\.index)) : []
    }

    public mutating func cancel() { draft = nil }

    /// Called only after the mutation was acknowledged; a subsequent read may still fail independently.
    public mutating func acknowledge(_ indices: Set<Int32>) {
        guard var value = snapshot else { return }
        for index in value.files.indices {
            value.files[index].selected = indices.contains(value.files[index].index)
        }
        snapshot = value
        draft = nil
    }
}
