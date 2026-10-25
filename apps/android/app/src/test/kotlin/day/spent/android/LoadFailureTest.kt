package day.spent.android

import android.os.Build
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import uniffi.spentday_core.SpentDayException

/** Only a database from an early build may lead the user to Clear storage. */
class LoadFailureTest {
    private val old = CantOpenData(SpentDayException.OldDatabase("/data/spentday.db"))
    private val diskFull = CantOpenData(SpentDayException.Storage("database or disk is full"))

    @Test
    fun anOldDatabaseGetsTheStartOverSteps() {
        assertTrue(old.oldDatabase)
        val text = describeLoadFailure(old, sdk = Build.VERSION_CODES.S)
        assertTrue(text, "Storage & cache, then Clear storage" in text)
        assertTrue(text, "Storage, then Clear storage" in describeLoadFailure(old, sdk = Build.VERSION_CODES.R))
    }

    @Test
    fun anyOtherOpenFailureSaysWhyAndToTryAgain() {
        assertFalse(diskFull.oldDatabase)
        val text = describeLoadFailure(diskFull, sdk = Build.VERSION_CODES.S)
        assertEquals(
            "The local data didn't open (database or disk is full). Pull down to try again. " +
                "If it keeps failing, the data on this phone may be damaged or from a newer version of Spent Day.",
            text,
        )
        assertFalse(text, "delete" in text.lowercase() || "clear storage" in text.lowercase())
    }

    @Test
    fun theSyncScreenGetsOneShortLineForEitherKind() {
        for (e in listOf(old, diskFull)) assertEquals("Can't open the local data. See the main screen.", describe(e))
    }

    @Test
    fun otherLoadFailuresAreDescribedAsAnywhere() {
        val e = SpentDayException.Storage("locked")
        assertEquals(describe(e), describeLoadFailure(e))
    }
}
