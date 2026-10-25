package day.spent.android

import android.Manifest
import android.app.AppOpsManager
import android.app.usage.UsageEvents
import android.app.usage.UsageStatsManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Process
import android.os.SystemClock
import java.lang.reflect.Method
import androidx.core.content.edit
import uniffi.spentday_core.Activity
import uniffi.spentday_core.SpentDay

/** Reads UsageStats events from a stored cursor and records foreground intervals in the core. */
object UsageCollector {
    private const val PREFS = "collector"
    private const val CURSOR = "cursor_ms"
    private const val FIRST_RUN_LOOKBACK_MS = 7 * 24 * 3_600_000L

    /** UsageEvents.Event.ACTIVITY_DESTROYED, a system API. Logged since Android 10. */
    private const val ACTIVITY_DESTROYED = 24

    fun hasPermission(context: Context): Boolean {
        val appOps = context.getSystemService(AppOpsManager::class.java)
        @Suppress("DEPRECATION")
        val mode = appOps.checkOpNoThrow(AppOpsManager.OPSTR_GET_USAGE_STATS, Process.myUid(), context.packageName)
        return when (mode) {
            AppOpsManager.MODE_ALLOWED -> true
            // Some devices leave the op at its default and grant through the permission instead.
            AppOpsManager.MODE_DEFAULT ->
                context.checkSelfPermission(Manifest.permission.PACKAGE_USAGE_STATS) == PackageManager.PERMISSION_GRANTED
            else -> false
        }
    }

    /** Returns how many intervals it recorded. The cursor moves only after every one of them was stored. */
    @Synchronized
    fun collect(context: Context, spentDay: SpentDay, nowMs: Long = System.currentTimeMillis()): Int {
        if (!hasPermission(context)) return 0
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val cursor = prefs.getLong(CURSOR, nowMs - FIRST_RUN_LOOKBACK_MS)
        val pass = foregroundIntervals(readEvents(context, cursor, nowMs), untilMs = nowMs)

        val pm = context.packageManager
        val launcher = homePackage(pm)
        val labels = mutableMapOf<String, String>()
        val recorded = pass.intervals.filter { it.packageName != launcher }
        record(spentDay, recorded) { packageName -> labels.getOrPut(packageName) { appLabel(pm, packageName) } }
        prefs.edit { putLong(CURSOR, pass.nextCursorMs) }
        return recorded.size
    }

    fun record(spentDay: SpentDay, intervals: List<ForegroundInterval>, label: (packageName: String) -> String) {
        for (interval in intervals) {
            spentDay.recordInterval(
                interval.startMs,
                interval.endMs,
                Activity(appId = interval.packageName, appName = label(interval.packageName), web = null),
            )
        }
    }

    private fun readEvents(context: Context, fromMs: Long, untilMs: Long): List<UsageEvent> {
        val usm = context.getSystemService(UsageStatsManager::class.java)
        val events = usm.queryEvents(fromMs, untilMs)
        val event = UsageEvents.Event()
        val out = mutableListOf<UsageEvent>()
        while (events.getNextEvent(event)) {
            val kind = when (event.eventType) {
                UsageEvents.Event.ACTIVITY_RESUMED -> UsageEventKind.Resumed
                UsageEvents.Event.ACTIVITY_PAUSED -> UsageEventKind.Paused
                UsageEvents.Event.ACTIVITY_STOPPED, ACTIVITY_DESTROYED -> UsageEventKind.Stopped
                UsageEvents.Event.SCREEN_NON_INTERACTIVE,
                UsageEvents.Event.KEYGUARD_SHOWN,
                UsageEvents.Event.DEVICE_SHUTDOWN -> UsageEventKind.ScreenOff
                UsageEvents.Event.DEVICE_STARTUP -> UsageEventKind.Startup
                else -> UsageEventKind.Other
            }
            out += UsageEvent(kind, event.packageName, event.timeStamp, event.className, instanceId(event))
        }
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
            estimatedStartup(System.currentTimeMillis(), SystemClock.elapsedRealtime(), fromMs)?.let { out += it }
        }
        return out
    }

    /** Android 10 logs an activity instance id with each event but keeps its getter hidden. */
    private val instanceIdGetter: Method? =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            runCatching { UsageEvents.Event::class.java.getMethod("getInstanceId") }.getOrNull()
        } else {
            null
        }

    private fun instanceId(event: UsageEvents.Event): Int? =
        instanceIdGetter?.let { runCatching { it.invoke(event) as Int }.getOrNull() }

    private fun homePackage(pm: PackageManager): String? =
        pm.resolveActivity(Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_HOME), PackageManager.MATCH_DEFAULT_ONLY)
            ?.activityInfo?.packageName

    private fun appLabel(pm: PackageManager, packageName: String): String =
        runCatching { pm.getApplicationLabel(pm.getApplicationInfo(packageName, 0)).toString() }
            .getOrDefault(packageName)
}
