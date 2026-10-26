import FluxDomain
import Foundation
import Testing

struct BtTaskFilesTests {
    @Test func wirePreservesPaddingGapsAndUnknownZeroByteProgress() throws {
        let data = Data(#"""
        {"taskId":"t","metadataReady":true,"files":[
          {"index":0,"path":"a.bin","size":20,"downloadedBytes":10,"selected":true},
          {"index":2,"path":"nested/empty.bin","size":0,"downloadedBytes":null,"selected":false}
        ]}
        """#.utf8)
        let result = try JSONDecoder().decode(BtTaskFiles.self, from: data)
        #expect(result.files.map(\.index) == [0, 2])
        #expect(result.files[0].progress == 0.5)
        #expect(result.files[1].progress == nil)
        var verified = result.files[1]
        verified.downloadedBytes = 0
        #expect(verified.progress == 1)
        let command = SetBtTaskFilesParams(taskId: "t", indices: [2])
        let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(command)) as? [String: Any]
        #expect(json?["taskId"] as? String == "t")
        #expect(json?["indices"] as? [Int] == [2])
    }

    @Test func pollingPreservesDraftAndCancelReturnsToLatestHostChoice() {
        var editor = BtTaskFilesSelection(taskId: "t")
        editor.receive(fixture())
        editor.setSelected(2, selected: true)
        var refreshed = fixture()
        refreshed.files[0].downloadedBytes = 17
        editor.receive(refreshed)
        #expect(editor.selected == [0, 2])
        #expect(editor.snapshot?.files[0].downloadedBytes == 17)
        #expect(editor.canApply)
        editor.cancel()
        #expect(editor.selected == [0])
        #expect(!editor.changed)
        editor.receive(BtTaskFiles(taskId: "another", metadataReady: true, files: []))
        #expect(editor.snapshot?.taskId == "t")
    }

    @Test func emptyDraftCannotApply() {
        var editor = BtTaskFilesSelection(taskId: "t")
        #expect(!editor.canApply)
        editor.receive(fixture())
        editor.setSelected(0, selected: false)
        #expect(editor.changed && editor.selected.isEmpty && !editor.canApply)
        // Editing download scope never mutates the host snapshot before acknowledgment.
        #expect(editor.snapshot?.files[0].selected == true)
        editor.cancel()
        editor.setSelected(1, selected: true) // Padding indices never enter a request.
        #expect(editor.selected == [0])
        editor.setSelected(0, selected: false)
        editor.setSelected(2, selected: true)
        #expect(editor.canApply)
        editor.acknowledge([2])
        #expect(editor.selected == [2] && !editor.changed)
    }

    @Test func emptyWireSelectionKeepsAllFilesSentinel() throws {
        let command = SetBtTaskFilesParams(taskId: "t", indices: [])
        let json = try JSONSerialization.jsonObject(with: JSONEncoder().encode(command)) as? [String: Any]
        #expect(json?["indices"] as? [Int] == [])
    }

    @Test func cleanupRequiresStoppedTaskAndAcknowledgedSubset() {
        var editor = BtTaskFilesSelection(taskId: "t")
        #expect(!editor.canCleanUnselected(status: .paused, isSeeding: false))
        editor.receive(fixture())
        for status in [TaskStatus.paused, .failed, .completed] {
            #expect(editor.canCleanUnselected(status: status, isSeeding: false))
        }
        for status in [TaskStatus.pending, .downloading, .preparing, .unknown] {
            #expect(!editor.canCleanUnselected(status: status, isSeeding: false))
        }
        #expect(!editor.canCleanUnselected(status: .completed, isSeeding: true))
        editor.setSelected(2, selected: true)
        #expect(!editor.canCleanUnselected(status: .paused, isSeeding: false))
        editor.acknowledge([0, 2])
        #expect(!editor.canCleanUnselected(status: .paused, isSeeding: false))
    }

    @Test func cleanupConfirmationRejectsChangedAppliedScopeAndUnsavedDraft() {
        var editor = BtTaskFilesSelection(taskId: "t")
        #expect(!editor.matchesAppliedSelection([0]))
        editor.receive(fixture())
        let confirmed = editor.selected
        #expect(editor.matchesAppliedSelection(confirmed))
        var progress = fixture()
        progress.files[0].downloadedBytes = 19
        editor.receive(progress)
        #expect(editor.matchesAppliedSelection(confirmed))
        editor.setSelected(2, selected: true)
        #expect(!editor.matchesAppliedSelection(confirmed))
        editor.cancel()
        #expect(editor.matchesAppliedSelection(confirmed))
        var remote = fixture()
        remote.files[0].selected = false
        remote.files[1].selected = true
        editor.receive(remote)
        #expect(!editor.matchesAppliedSelection(confirmed))
    }

    private func fixture() -> BtTaskFiles {
        BtTaskFiles(taskId: "t", metadataReady: true, files: [
            BtTaskFile(index: 0, path: "a.bin", size: 20, downloadedBytes: 10, selected: true),
            BtTaskFile(index: 2, path: "nested/b.bin", size: 30, downloadedBytes: nil, selected: false),
        ])
    }
}
