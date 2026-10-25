package day.spent.android

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class ReadScheduleTest {
    @Test
    fun aReadWaitsForChangesToSettle() {
        val schedule = ReadSchedule()
        schedule.changed(nowMs = 0)
        assertEquals(300L, schedule.dueMs)
        schedule.changed(nowMs = 200)
        assertEquals(500L, schedule.dueMs)
        schedule.read(nowMs = 500, resolved = true)
        assertNull(schedule.dueMs)
    }

    @Test
    fun steadyChangesStillReadWithinASecond() {
        val schedule = ReadSchedule()
        for (at in 0L..900L step 150) schedule.changed(nowMs = at)
        assertEquals(1_000L, schedule.dueMs)
        schedule.read(nowMs = 1_000, resolved = true)
        schedule.changed(nowMs = 1_100)
        assertEquals(1_400L, schedule.dueMs)
    }

    @Test
    fun aHiddenOrFocusedBarIsReadAgainThreeTimes() {
        val schedule = ReadSchedule()
        schedule.changed(nowMs = 0)
        val reads = generateSequence(schedule.dueMs) { at -> schedule.read(at, resolved = false); schedule.dueMs }
        assertEquals(listOf(300L, 800L, 1_300L, 1_800L), reads.toList())
    }

    @Test
    fun onlyChangesBeforeTheReadAreGathered() {
        val schedule = ReadSchedule()
        assertFalse(schedule.gathering)
        schedule.changed(nowMs = 0)
        assertTrue(schedule.gathering)
        schedule.read(nowMs = 300, resolved = false)
        assertFalse(schedule.gathering)
    }

    @Test
    fun aRetryThatFindsThePageStops() {
        val schedule = ReadSchedule()
        schedule.changed(nowMs = 0)
        schedule.read(nowMs = 300, resolved = false)
        assertEquals(800L, schedule.dueMs)
        schedule.read(nowMs = 800, resolved = true)
        assertNull(schedule.dueMs)
    }

    @Test
    fun aChangeDuringRetriesStartsOver() {
        val schedule = ReadSchedule()
        schedule.changed(nowMs = 0)
        schedule.read(nowMs = 300, resolved = false)
        schedule.read(nowMs = 800, resolved = false)
        schedule.changed(nowMs = 900)
        assertEquals(1_200L, schedule.dueMs)
        val reads = generateSequence(schedule.dueMs) { at -> schedule.read(at, resolved = false); schedule.dueMs }
        assertEquals(listOf(1_200L, 1_700L, 2_200L, 2_700L), reads.toList())
    }
}
