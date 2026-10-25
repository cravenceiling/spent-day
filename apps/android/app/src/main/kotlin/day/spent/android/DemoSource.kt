package day.spent.android

import uniffi.spentday_core.AppUsage
import uniffi.spentday_core.DeviceInfo
import uniffi.spentday_core.DeviceUsage
import uniffi.spentday_core.DomainUsage
import uniffi.spentday_core.Platform
import uniffi.spentday_core.Summary
import uniffi.spentday_core.SummaryQuery
import uniffi.spentday_core.SyncConfig

/** Canned data for screenshots and UI work. Debug builds use it when launched with `--ez demo true`. */
class DemoSource : UsageSource {
    private val now = System.currentTimeMillis()
    private val devices = listOf(
        DeviceInfo("phone", "Pixel 9", Platform.ANDROID, now),
        DeviceInfo("mac", "MacBook Pro", Platform.MACOS, now - 600_000),
        DeviceInfo("linux", "ThinkPad", Platform.LINUX, now - 86_400_000),
    )
    private val share = mapOf("phone" to 0.45, "mac" to 0.4, "linux" to 0.15)

    /** Minutes per hour of a typical day, midnight to 23:00. */
    private val dayShape = listOf(0, 0, 0, 0, 0, 0, 2, 14, 22, 38, 44, 31, 18, 36, 47, 41, 29, 20, 26, 33, 41, 28, 12, 3)

    override fun devices() = devices

    override fun summary(query: SummaryQuery): Summary {
        val scale = query.deviceId?.let { share[it] } ?: 1.0
        val slots = ((query.endMs - query.startMs) / query.bucketMs).toInt()
        val buckets = List(slots) { i ->
            val slotStart = query.startMs + i * query.bucketMs
            val ms = if (query.bucketMs >= 86_400_000L) {
                dayShape.sum() * 60_000L * (80 + (i * 37) % 45) / 100
            } else {
                dayShape[i % 24] * 60_000L
            }
            if (slotStart > now) 0L else (ms * scale).toLong()
        }
        val total = buckets.sum()
        fun part(f: Double) = (total * f).toLong()
        val youtube = DomainUsage("youtube.com", part(0.11))
        val domains = listOf(
            DomainUsage("github.com", part(0.09)),
            youtube,
            DomainUsage("news.ycombinator.com", part(0.04)),
            DomainUsage("en.wikipedia.org", part(0.025)),
            DomainUsage("docs.rs", part(0.015)),
        ).sortedByDescending { it.totalMs }
        val chrome = AppUsage(
            "chrome", "Chrome", true, domains.sumOf { it.totalMs } + part(0.01),
            (domains + DomainUsage("", part(0.01))).sortedByDescending { it.totalMs }, listOf("com.android.chrome"),
        )
        val firefox = AppUsage(
            "firefox", "Firefox", true, part(0.06),
            listOf(DomainUsage("developer.mozilla.org", part(0.04)), DomainUsage("reddit.com", part(0.02))),
            listOf("org.mozilla.firefox"),
        )
        val apps = listOf(
            chrome,
            AppUsage("com.google.android.youtube", "YouTube", false, part(0.14), emptyList(), listOf("com.google.android.youtube")),
            AppUsage("com.microsoft.VSCode", "Visual Studio Code", false, part(0.13), emptyList(), listOf("com.microsoft.VSCode")),
            AppUsage("com.tinyspeck.slackmacgap", "Slack", false, part(0.09), emptyList(), listOf("com.tinyspeck.slackmacgap")),
            firefox,
            AppUsage("com.google.android.gm", "Gmail", false, part(0.05), emptyList(), listOf("com.google.android.gm")),
            AppUsage("com.google.android.apps.maps", "Maps", false, part(0.03), emptyList(), listOf("com.google.android.apps.maps")),
            AppUsage("com.android.settings", "Settings", false, part(0.01), emptyList(), listOf("com.android.settings")),
        ).sortedByDescending { it.totalMs }
        val sites = (domains + firefox.domains).groupBy { it.domain }
            .map { (domain, rows) -> DomainUsage(domain, rows.sumOf { it.totalMs }) }
            .sortedByDescending { it.totalMs }
        val byDevice = devices.filter { query.deviceId == null || it.id == query.deviceId }
            .map { DeviceUsage(it, if (query.deviceId == null) (total * share.getValue(it.id)).toLong() else total) }
        return Summary(query.startMs, query.endMs, total, byDevice, apps, sites, buckets)
    }
}

/** What the sync screen shows in demo mode. The server and code are made up, so screenshots show no real secret. */
fun demoSyncState(nowMs: Long = System.currentTimeMillis()) = SyncState(
    config = SyncConfig("https://sync.example.com", List(14) { "DEMO" }.joinToString("-")),
    history = SyncHistory(lastGood = SyncResult(nowMs - 4 * 60_000, "Sent 12, received 37")),
)
