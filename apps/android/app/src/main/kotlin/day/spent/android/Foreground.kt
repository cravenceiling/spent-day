package day.spent.android

/**
 * [Stopped] is an activity no longer shown, stopped or destroyed. [Other] is any event Spent Day doesn't
 * act on, the screen coming on included. It still shows the device was on at that time.
 */
enum class UsageEventKind { Resumed, Paused, Stopped, ScreenOff, Startup, Other }

/**
 * [instanceId] tells two activities of one app apart, as in split screen, even two of one class.
 * Below Android 10 there is none, and [className] stands in for it.
 */
data class UsageEvent(
    val kind: UsageEventKind,
    val packageName: String,
    val atMs: Long,
    val className: String? = null,
    val instanceId: Int? = null,
)

private data class ActivityKey(val packageName: String, val className: String?, val instanceId: Int?)

private val UsageEvent.activity: ActivityKey
    get() = ActivityKey(packageName, className.takeIf { instanceId == null }, instanceId)

/**
 * One with no length means the app had no foreground time from [startMs]: recording it retracts
 * what an earlier pass recorded from that start, as when a reboot shows that a session read as
 * still open had already ended.
 */
data class ForegroundInterval(val packageName: String, val startMs: Long, val endMs: Long)

/**
 * [intervals] may end with a still-open interval clipped at `untilMs`. [nextCursorMs] is where the
 * next pass must start reading so that interval is read again from its start and re-recorded with
 * its real end. `record_interval` is idempotent per (app, start), so the overlap is safe.
 */
data class ForegroundPass(val intervals: List<ForegroundInterval>, val nextCursorMs: Long)

/** [Open.activity] is the activity in front, which may not be the one the interval started with. */
private sealed interface Foreground {
    data object None : Foreground
    data class Open(val activity: ActivityKey, val startMs: Long) : Foreground
    data class Paused(val packageName: String, val startMs: Long, val pausedAtMs: Long) : Foreground
}

/** A pause followed this soon by a resume of the same app is one activity handing off to another. */
const val HANDOFF_MS = 2_000L

fun foregroundIntervals(events: List<UsageEvent>, untilMs: Long): ForegroundPass {
    val out = mutableListOf<ForegroundInterval>()
    fun emit(packageName: String, startMs: Long, endMs: Long) {
        out += ForegroundInterval(packageName, startMs, maxOf(startMs, endMs))
    }
    fun close(state: Foreground, atMs: Long) = when (state) {
        Foreground.None -> Unit
        is Foreground.Open -> emit(state.activity.packageName, state.startMs, atMs)
        is Foreground.Paused -> emit(state.packageName, state.startMs, state.pausedAtMs)
    }

    // Activities resumed and not yet paused, oldest first, with when each resumed. Split screen
    // keeps two resumed at once, and the app in front is the most recently resumed one.
    val resumed = LinkedHashMap<ActivityKey, Long>()
    var state: Foreground = Foreground.None
    var lastEventMs: Long? = null

    fun bringToFront(activity: ActivityKey, atMs: Long): Foreground = state.let { current ->
        when {
            current is Foreground.Open && current.activity.packageName == activity.packageName ->
                Foreground.Open(activity, current.startMs)
            current is Foreground.Paused && current.packageName == activity.packageName &&
                atMs - current.pausedAtMs <= HANDOFF_MS -> Foreground.Open(activity, current.startMs)
            else -> {
                close(current, atMs)
                Foreground.Open(activity, atMs)
            }
        }
    }

    for (event in events.sortedBy { it.atMs }) {
        val activity = event.activity
        state = when (event.kind) {
            UsageEventKind.Resumed -> {
                resumed.remove(activity)
                resumed[activity] = event.atMs
                bringToFront(activity, event.atMs)
            }
            UsageEventKind.Paused -> {
                resumed.remove(activity)
                val stillResumed = resumed.keys.lastOrNull()
                val current = state
                when {
                    stillResumed != null -> bringToFront(stillResumed, event.atMs)
                    current is Foreground.Open && current.activity.packageName == event.packageName ->
                        Foreground.Paused(event.packageName, current.startMs, event.atMs)
                    else -> current
                }
            }
            // What was resumed before the screen went off may go without a pause logged, so it is no
            // fallback afterwards. What resumes while the screen is off, as on the lock screen, still is.
            UsageEventKind.ScreenOff -> {
                close(state, event.atMs)
                resumed.clear()
                Foreground.None
            }
            // A boot with no shutdown before it: the device died, so the open app ends at the last sign of life.
            UsageEventKind.Startup -> {
                close(state, lastEventMs ?: event.atMs)
                resumed.clear()
                Foreground.None
            }
            // The activity in front stopping with no pause logged ends it.
            UsageEventKind.Stopped -> {
                resumed.remove(activity)
                val current = state
                val stillResumed = resumed.keys.lastOrNull()
                when {
                    current !is Foreground.Open || current.activity != activity -> current
                    stillResumed != null -> bringToFront(stillResumed, event.atMs)
                    else -> {
                        close(current, event.atMs)
                        Foreground.None
                    }
                }
            }
            UsageEventKind.Other -> state
        }
        lastEventMs = event.atMs
    }

    val stateCursor = when (val last = state) {
        Foreground.None -> untilMs
        is Foreground.Open -> {
            emit(last.activity.packageName, last.startMs, untilMs)
            last.startMs
        }
        is Foreground.Paused -> {
            close(last, untilMs)
            if (untilMs - last.pausedAtMs > HANDOFF_MS) untilMs else last.startMs
        }
    }
    // The next pass must see every still-resumed activity resume again to fall back to it.
    val cursor = minOf(stateCursor, resumed.values.minOrNull() ?: stateCursor)
    return ForegroundPass(out, cursor)
}

/**
 * Android 8 and 9 log no DEVICE_STARTUP event. The boot time, when it falls inside the window being
 * read, stands in for one.
 */
fun estimatedStartup(nowMs: Long, elapsedRealtimeMs: Long, fromMs: Long): UsageEvent? {
    val bootMs = nowMs - elapsedRealtimeMs
    return if (bootMs > fromMs) UsageEvent(UsageEventKind.Startup, "android", bootMs) else null
}
