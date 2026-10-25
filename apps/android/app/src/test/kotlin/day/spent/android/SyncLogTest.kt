package day.spent.android

import android.app.Application
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import uniffi.spentday_core.SpentDayException
import uniffi.spentday_core.SyncReport

@RunWith(RobolectricTestRunner::class)
@Config(application = Application::class)
class SyncLogTest {
    private val context = RuntimeEnvironment.getApplication()

    private fun ok(finishedAtMs: Long) = Result.success(SyncReport(1uL, 2uL, finishedAtMs, 0uL))
    private val unreachable = Result.failure<SyncReport>(SpentDayException.Network("down"))

    @Test
    fun aSyncThatFinishesLateDoesNotReplaceANewerResult() {
        SyncLog.record(context, ok(2_000), nowMs = 2_000)
        SyncLog.record(context, ok(1_000), nowMs = 2_100)
        SyncLog.record(context, unreachable, nowMs = 1_500)
        assertEquals(SyncHistory(lastGood = SyncResult(2_000, "Sent 1, received 2")), SyncLog.history(context))
    }

    @Test
    fun aNewerFailureShowsBesideTheLastGoodSync() {
        SyncLog.record(context, ok(2_000), nowMs = 2_000)
        SyncLog.record(context, unreachable, nowMs = 3_000)
        SyncLog.record(context, ok(2_500), nowMs = 3_100)
        assertEquals(
            SyncHistory(
                lastGood = SyncResult(2_500, "Sent 1, received 2"),
                failure = SyncResult(3_000, "Couldn't reach the server. Check the address and your connection."),
            ),
            SyncLog.history(context),
        )
        SyncLog.record(context, ok(4_000), nowMs = 4_000)
        assertEquals(SyncHistory(lastGood = SyncResult(4_000, "Sent 1, received 2")), SyncLog.history(context))
    }
}
