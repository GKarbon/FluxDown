package com.fluxdown.core.protocol

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class BtTaskFilesTest {
    @Test fun preservesPaddingGapsUnknownProgressAndEmptyFiles() {
        val files = BtTaskFiles.fromJson(Json.parse("""{"taskId":"bt","metadataReady":true,"files":[{"index":0,"path":"a.bin","size":100,"downloadedBytes":40,"selected":true},{"index":2,"path":"b.bin","size":200,"downloadedBytes":null,"selected":false},{"index":4,"path":"empty","size":0,"downloadedBytes":0,"selected":true}]}"""))!!
        assertEquals(listOf(0, 2, 4), files.files.map { it.index })
        assertEquals(setOf(0, 4), files.selectedIndices)
        assertNull(files.files[1].downloadedBytes)
        assertEquals(0L, files.files[2].downloadedBytes)
        assertTrue(files.metadataReady)
    }

    @Test fun pendingMetadataIsReadable() {
        val pending = BtTaskFiles.fromJson(Json.parse("""{"taskId":"bt","metadataReady":false,"files":[]}"""))!!
        assertFalse(pending.metadataReady)
        assertTrue(pending.selectedIndices.isEmpty())
    }

    @Test fun encodesExactOriginalIndices() {
        assertEquals("""{"taskId":"bt","indices":[0,4]}""", SetBtTaskFiles("bt", setOf(4, 0)).toJson().toJson())
    }

    @Test fun emptyIndicesKeepOriginalAllFilesEncoding() {
        assertEquals("""{"taskId":"bt","indices":[]}""", SetBtTaskFiles("bt", emptySet()).toJson().toJson())
    }

    @Test fun malformedFileCannotBecomeAnEditablePartialList() {
        assertNull(BtTaskFiles.fromJson(Json.parse("""{"taskId":"bt","metadataReady":true,"files":[{"index":-1,"path":"a","size":5,"downloadedBytes":0,"selected":true}]}""")))
        assertNull(BtTaskFiles.fromJson(Json.parse("""{"taskId":"bt","metadataReady":true}""")))
    }
}
