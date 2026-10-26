package com.fluxdown.core.protocol

/** Original metadata indices are preserved, including gaps left by padding files. */
data class BtTaskFile(
    val index: Int,
    val path: String,
    val size: Long,
    val downloadedBytes: Long?,
    val selected: Boolean,
)

/** Existing-task details; unknown verified progress remains null. */
data class BtTaskFiles(val taskId: String, val metadataReady: Boolean, val files: List<BtTaskFile>) {
    val selectedIndices: Set<Int> get() = files.filter { it.selected }.mapTo(linkedSetOf()) { it.index }

    companion object {
        fun fromJson(value: JsonValue?): BtTaskFiles? {
            val taskId = value.strOrNull("taskId") ?: return null
            val ready = value["metadataReady"].boolOrNull ?: return null
            val entries = value["files"].arrayOrNull ?: return null
            val files = entries.map { file ->
                val index = file["index"].intOrNull?.takeIf { it >= 0 } ?: return null
                val size = file["size"].longOrNull?.takeIf { it >= 0 } ?: return null
                val progress = file["downloadedBytes"]
                val bytes = progress.longOrNull
                if (!progress.isNull && (bytes == null || bytes < 0)) return null
                BtTaskFile(
                    index = index,
                    path = file.strOrNull("path") ?: return null,
                    size = size,
                    downloadedBytes = bytes,
                    selected = file["selected"].boolOrNull ?: return null,
                )
            }
            if (files.map { it.index }.distinct().size != files.size) return null
            return BtTaskFiles(taskId, ready, files)
        }
    }
}

/** Empty indices preserve the original wire convention: all files. UI disallows zero checked files. */
data class SetBtTaskFiles(val taskId: String, val indices: Set<Int>) {
    fun toJson(): JsonValue {
        require(indices.all { it >= 0 })
        return jsonObject("taskId" to taskId, "indices" to indices.sorted())
    }
}
