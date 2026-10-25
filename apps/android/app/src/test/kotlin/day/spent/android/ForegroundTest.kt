package day.spent.android

import day.spent.android.UsageEventKind.Paused
import day.spent.android.UsageEventKind.Resumed
import day.spent.android.UsageEventKind.Other
import day.spent.android.UsageEventKind.ScreenOff
import day.spent.android.UsageEventKind.Startup
import day.spent.android.UsageEventKind.Stopped
import org.junit.Assert.assertEquals
import org.junit.Test

class ForegroundTest {
    private fun ev(kind: UsageEventKind, pkg: String, at: Long, cls: String? = null, id: Int? = null) = UsageEvent(kind, pkg, at, cls, id)
    private fun iv(pkg: String, start: Long, end: Long) = ForegroundInterval(pkg, start, end)

    @Test
    fun pairsResumeAndPauseAcrossApps() {
        val pass = foregroundIntervals(
            listOf(
                ev(Resumed, "a", 1_000), ev(Paused, "a", 10_000),
                ev(Resumed, "b", 10_300), ev(Paused, "b", 50_000),
            ),
            untilMs = 100_000,
        )
        assertEquals(listOf(iv("a", 1_000, 10_000), iv("b", 10_300, 50_000)), pass.intervals)
        assertEquals(100_000, pass.nextCursorMs)
    }

    @Test
    fun activityHandoffInsideOneAppIsOneInterval() {
        val pass = foregroundIntervals(
            listOf(
                ev(Resumed, "a", 1_000), ev(Paused, "a", 5_000),
                ev(Resumed, "a", 5_200), ev(Paused, "a", 9_000),
            ),
            untilMs = 60_000,
        )
        assertEquals(listOf(iv("a", 1_000, 9_000)), pass.intervals)
    }

    @Test
    fun screenOffClosesTheOpenApp() {
        val pass = foregroundIntervals(
            listOf(ev(Resumed, "a", 1_000), ev(ScreenOff, "android", 4_000), ev(Paused, "a", 4_100)),
            untilMs = 60_000,
        )
        assertEquals(listOf(iv("a", 1_000, 4_000)), pass.intervals)
        assertEquals(60_000, pass.nextCursorMs)
    }

    @Test
    fun stillOpenAppIsClippedAndCursorStaysAtItsStart() {
        val pass = foregroundIntervals(
            listOf(ev(Resumed, "a", 1_000), ev(Paused, "a", 2_000), ev(Resumed, "b", 2_100)),
            untilMs = 30_000,
        )
        assertEquals(listOf(iv("a", 1_000, 2_000), iv("b", 2_100, 30_000)), pass.intervals)
        assertEquals(2_100, pass.nextCursorMs)
    }

    @Test
    fun reReadingFromTheCursorExtendsTheSameInterval() {
        val first = foregroundIntervals(listOf(ev(Resumed, "b", 2_100)), untilMs = 30_000)
        val second = foregroundIntervals(
            listOf(ev(Resumed, "b", 2_100), ev(Paused, "b", 45_000)),
            untilMs = 90_000,
        )
        assertEquals(listOf(iv("b", 2_100, 30_000)), first.intervals)
        assertEquals(listOf(iv("b", 2_100, 45_000)), second.intervals)
        assertEquals(90_000, second.nextCursorMs)
    }

    @Test
    fun recentPauseKeepsCursorInCaseTheAppResumes() {
        val pass = foregroundIntervals(listOf(ev(Resumed, "a", 1_000), ev(Paused, "a", 29_500)), untilMs = 30_000)
        assertEquals(listOf(iv("a", 1_000, 29_500)), pass.intervals)
        assertEquals(1_000, pass.nextCursorMs)
    }

    @Test
    fun pauseWithoutResumeIsIgnored() {
        val pass = foregroundIntervals(listOf(ev(Paused, "a", 1_000)), untilMs = 5_000)
        assertEquals(emptyList<ForegroundInterval>(), pass.intervals)
        assertEquals(5_000, pass.nextCursorMs)
    }

    @Test
    fun uncleanRebootEndsTheOpenAppAtTheLastEventBeforeIt() {
        val pass = foregroundIntervals(
            listOf(
                ev(Resumed, "a", 1_000), ev(Other, "x", 5_000),
                ev(Startup, "android", 3_600_000), ev(Resumed, "b", 3_610_000),
            ),
            untilMs = 3_700_000,
        )
        assertEquals(listOf(iv("a", 1_000, 5_000), iv("b", 3_610_000, 3_700_000)), pass.intervals)
    }

    @Test
    fun rebootRightAfterAResumeRetractsThatApp() {
        val pass = foregroundIntervals(listOf(ev(Resumed, "a", 1_000), ev(Startup, "android", 900_000)), untilMs = 1_000_000)
        assertEquals(listOf(iv("a", 1_000, 1_000)), pass.intervals)
        assertEquals(1_000_000, pass.nextCursorMs)
    }

    @Test
    fun bootTimeStandsInForStartupWhenInsideTheWindow() {
        assertEquals(UsageEvent(Startup, "android", 40_000), estimatedStartup(nowMs = 100_000, elapsedRealtimeMs = 60_000, fromMs = 10_000))
        assertEquals(null, estimatedStartup(nowMs = 100_000, elapsedRealtimeMs = 95_000, fromMs = 10_000))
    }

    @Test
    fun splitScreenFallsBackToTheAppStillResumed() {
        val pass = foregroundIntervals(
            listOf(
                ev(Resumed, "a", 1_000, "A"),
                ev(Resumed, "b", 2_000, "B"),
                ev(Paused, "b", 9_000, "B"),
                ev(Paused, "a", 20_000, "A"),
            ),
            untilMs = 60_000,
        )
        assertEquals(listOf(iv("a", 1_000, 2_000), iv("b", 2_000, 9_000), iv("a", 9_000, 20_000)), pass.intervals)
        assertEquals(60_000, pass.nextCursorMs)
    }

    @Test
    fun pausingTheOlderSplitScreenAppKeepsTheNewerOne() {
        val pass = foregroundIntervals(
            listOf(ev(Resumed, "a", 1_000, "A"), ev(Resumed, "b", 2_000, "B"), ev(Paused, "a", 5_000, "A")),
            untilMs = 30_000,
        )
        assertEquals(listOf(iv("a", 1_000, 2_000), iv("b", 2_000, 30_000)), pass.intervals)
        assertEquals(2_000, pass.nextCursorMs)
    }

    @Test
    fun anotherActivityOfTheSameAppStillResumedKeepsItOpen() {
        val pass = foregroundIntervals(
            listOf(ev(Resumed, "a", 1_000, "Main"), ev(Resumed, "a", 3_000, "Detail"), ev(Paused, "a", 4_000, "Main")),
            untilMs = 30_000,
        )
        assertEquals(listOf(iv("a", 1_000, 30_000)), pass.intervals)
    }

    @Test
    fun cursorStaysAtTheOldestStillResumedActivity() {
        val events = listOf(ev(Resumed, "a", 1_000, "A"), ev(Resumed, "b", 2_000, "B"))
        val first = foregroundIntervals(events, untilMs = 30_000)
        assertEquals(1_000, first.nextCursorMs)
        val second = foregroundIntervals(events + ev(Paused, "b", 40_000, "B"), untilMs = 50_000)
        assertEquals(listOf(iv("a", 1_000, 2_000), iv("b", 2_000, 40_000), iv("a", 40_000, 50_000)), second.intervals)
    }

    @Test
    fun screenOffForgetsResumedActivities() {
        val pass = foregroundIntervals(
            listOf(
                ev(Resumed, "a", 1_000, "A"), ev(Resumed, "b", 2_000, "B"), ev(ScreenOff, "android", 5_000),
                ev(Resumed, "c", 10_000, "C"), ev(Paused, "c", 12_000, "C"),
            ),
            untilMs = 60_000,
        )
        assertEquals(listOf(iv("a", 1_000, 2_000), iv("b", 2_000, 5_000), iv("c", 10_000, 12_000)), pass.intervals)
    }

    @Test
    fun twoInstancesOfOneActivityAreTrackedApart() {
        val pass = foregroundIntervals(
            listOf(
                ev(Resumed, "a", 1_000, "Main", id = 1),
                ev(Resumed, "a", 2_000, "Main", id = 2),
                ev(Paused, "a", 3_000, "Main", id = 1),
            ),
            untilMs = 30_000,
        )
        assertEquals(listOf(iv("a", 1_000, 30_000)), pass.intervals)
        assertEquals(1_000, pass.nextCursorMs)
    }

    @Test
    fun aStoppedActivityIsNoFallback() {
        val pass = foregroundIntervals(
            listOf(
                ev(Resumed, "a", 1_000, "A", id = 1),
                ev(Resumed, "b", 2_000, "B", id = 2),
                ev(Stopped, "a", 3_000, "A", id = 1),
                ev(Paused, "b", 9_000, "B", id = 2),
            ),
            untilMs = 60_000,
        )
        assertEquals(listOf(iv("a", 1_000, 2_000), iv("b", 2_000, 9_000)), pass.intervals)
        assertEquals(60_000, pass.nextCursorMs)
    }

    @Test
    fun anActivityResumedWhileTheScreenWasOffIsStillAFallback() {
        val pass = foregroundIntervals(
            listOf(
                ev(ScreenOff, "android", 1_000),
                ev(Resumed, "a", 2_000, "A", id = 1),
                ev(Other, "android", 3_000),
                ev(Resumed, "b", 4_000, "B", id = 2),
                ev(Paused, "b", 6_000, "B", id = 2),
                ev(Paused, "a", 9_000, "A", id = 1),
            ),
            untilMs = 20_000,
        )
        assertEquals(listOf(iv("a", 2_000, 4_000), iv("b", 4_000, 6_000), iv("a", 6_000, 9_000)), pass.intervals)
    }

    @Test
    fun stoppingTheActivityInFrontEndsIt() {
        val pass = foregroundIntervals(listOf(ev(Resumed, "a", 1_000), ev(Stopped, "a", 2_000)), untilMs = 30_000)
        assertEquals(listOf(iv("a", 1_000, 2_000)), pass.intervals)
        assertEquals(30_000, pass.nextCursorMs)
    }

    @Test
    fun stoppingTheActivityInFrontFallsBackToTheNewestStillResumed() {
        val pass = foregroundIntervals(
            listOf(
                ev(Resumed, "a", 1_000, "A", id = 1),
                ev(Resumed, "c", 1_500, "C", id = 3),
                ev(Resumed, "b", 2_000, "B", id = 2),
                ev(Stopped, "b", 5_000, "B", id = 2),
            ),
            untilMs = 30_000,
        )
        assertEquals(listOf(iv("a", 1_000, 1_500), iv("c", 1_500, 2_000), iv("b", 2_000, 5_000), iv("c", 5_000, 30_000)), pass.intervals)
    }

    @Test
    fun anotherInstanceOfTheAppInFrontStoppingKeepsItOpen() {
        val pass = foregroundIntervals(
            listOf(ev(Resumed, "a", 1_000, "Main", id = 1), ev(Stopped, "a", 2_000, "Main", id = 2)),
            untilMs = 30_000,
        )
        assertEquals(listOf(iv("a", 1_000, 30_000)), pass.intervals)
        assertEquals(1_000, pass.nextCursorMs)
    }
}
