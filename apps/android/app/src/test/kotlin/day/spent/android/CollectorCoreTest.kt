package day.spent.android

import day.spent.android.UsageEventKind.Other
import day.spent.android.UsageEventKind.Resumed
import day.spent.android.UsageEventKind.Startup
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder
import uniffi.spentday_core.SpentDay
import uniffi.spentday_core.Platform
import uniffi.spentday_core.SummaryQuery

/** Collector passes recorded into a real core database. */
class CollectorCoreTest {
    @get:Rule val dir = TemporaryFolder()

    private fun ev(kind: UsageEventKind, pkg: String, at: Long) = UsageEvent(kind, pkg, at)

    private fun withCore(block: SpentDay.() -> Unit) =
        SpentDay.open(dir.newFolder().resolve("spentday.db").path, "test", Platform.ANDROID).use(block)

    private fun SpentDay.pass(untilMs: Long, vararg events: UsageEvent) =
        UsageCollector.record(this, foregroundIntervals(events.toList(), untilMs).intervals) { it }

    private fun SpentDay.totalMs(app: String): Long =
        summary(SummaryQuery(0, 10_000_000, 10_000_000, null)).apps.filter { app in it.appIds }.sumOf { it.totalMs }

    @Test
    fun aRebootRightAfterAResumeRetractsTheIntervalRecordedUpToNow() = withCore {
        pass(30_000, ev(Resumed, "a", 1_000))
        assertEquals(29_000, totalMs("a"))

        pass(200_000, ev(Resumed, "a", 1_000), ev(Startup, "android", 100_000))
        assertEquals(0, totalMs("a"))
    }

    @Test
    fun aRebootShortensTheIntervalRecordedUpToNowToTheLastSignOfLife() = withCore {
        pass(30_000, ev(Resumed, "a", 1_000), ev(Other, "x", 5_000))
        assertEquals(29_000, totalMs("a"))

        pass(200_000, ev(Resumed, "a", 1_000), ev(Other, "x", 5_000), ev(Startup, "android", 100_000))
        assertEquals(4_000, totalMs("a"))
    }
}
