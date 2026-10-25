package day.spent.android

import android.accessibilityservice.AccessibilityService
import android.content.ComponentName
import android.content.Context
import android.os.Handler
import android.os.Looper
import android.os.SystemClock
import android.provider.Settings
import android.util.Log
import android.view.accessibility.AccessibilityEvent
import android.view.accessibility.AccessibilityNodeInfo
import java.util.concurrent.Executors
import uniffi.spentday_core.WebPage

/**
 * Reads the address bar of known browsers and reports the page to the core, which splits the
 * browser's UsageStats time by site. A private tab reports no page. Off until the user turns it on
 * in accessibility settings.
 */
class WebHintService : AccessibilityService() {
    private val gate = WebHintGate()
    private val io = Executors.newSingleThreadExecutor()
    private val main = Handler(Looper.getMainLooper())
    private val schedules = mutableMapOf<String, ReadSchedule>()
    private val reads = mutableMapOf<String, Runnable>()

    /** The URL bar's container the latest change came from, per browser. The id search misses Compose test tags. */
    private val lastToolbar = mutableMapOf<String, AccessibilityNodeInfo>()

    override fun onAccessibilityEvent(event: AccessibilityEvent) {
        val browser = event.packageName?.toString() ?: return
        val ui = readBrowsers[browser] ?: return
        val schedule = schedules.getOrPut(browser, ::ReadSchedule)
        when (event.eventType) {
            AccessibilityEvent.TYPE_WINDOW_STATE_CHANGED -> gate.forget(browser)
            // While a read waits, any change only delays it, so there's no need to walk the tree.
            AccessibilityEvent.TYPE_WINDOW_CONTENT_CHANGED ->
                if (!schedule.gathering && !noteToolbarChange(browser, ui, event.source)) return
        }
        schedule.changed(SystemClock.uptimeMillis())
        post(browser, ui, schedule)
    }

    /** Whether a change at [source] can change [browser]'s toolbar. Keeps the URL bar's container when it passes one. */
    private fun noteToolbarChange(browser: String, ui: BrowserUi, source: AccessibilityNodeInfo?): Boolean {
        val sourceUp = generateSequence(source) { it.parent }
        if (!isToolbarChange(ui, sourceUp.map { it.viewIdResourceName })) return false
        val container = (ui.urlBar as? UiNode.TestTag)?.container ?: return true
        sourceUp.take(MAX_ANCESTRY).firstOrNull { it.viewIdResourceName == container }?.let { lastToolbar[browser] = it }
        return true
    }

    private fun post(browser: String, ui: BrowserUi, schedule: ReadSchedule) {
        val read = reads.getOrPut(browser) {
            Runnable {
                schedule.read(SystemClock.uptimeMillis(), resolved = report(browser, ui))
                post(browser, ui, schedule)
            }
        }
        main.removeCallbacks(read)
        schedule.dueMs?.let { main.postAtTime(read, it) }
    }

    /** Returns false when the URL bar was hidden or being edited, so a later read may find the page. */
    private fun report(browser: String, ui: BrowserUi): Boolean {
        val root = rootInActiveWindow?.takeIf { it.packageName == browser } ?: return true
        val screen = readScreen(root, ui, lastToolbar[browser]?.takeIf { it.refresh() })
        val page = shownPage(ui, screen) ?: return false
        if (!gate.admit(browser, page)) return true
        val now = System.currentTimeMillis()
        io.execute {
            runCatching {
                when (page) {
                    is ShownPage.Web -> spentDayApp.spentDay.webHint(now, browser, WebPage(page.url))
                    ShownPage.NoPage -> spentDayApp.spentDay.webCleared(now, browser)
                }
            // Logs only the error's type, so nothing about the page reaches logcat.
            }.onFailure { Log.w("SpentDay", "web hint failed: ${it.javaClass.simpleName}") }
        }
        return true
    }

    override fun onInterrupt() = Unit

    override fun onDestroy() {
        main.removeCallbacksAndMessages(null)
        lastToolbar.clear()
        io.shutdown()
        super.onDestroy()
    }

    companion object {
        fun isEnabled(context: Context): Boolean {
            val enabled = Settings.Secure.getString(context.contentResolver, Settings.Secure.ENABLED_ACCESSIBILITY_SERVICES)
                ?: return false
            val me = ComponentName(context, WebHintService::class.java)
            return enabled.split(':').any { ComponentName.unflattenFromString(it) == me }
        }
    }
}

/**
 * What [root] shows of [ui]. Pages behind the one shown keep their own toolbars in the tree, so only
 * a visible match counts. [toolbar] is a candidate container the id search may have missed.
 */
internal fun readScreen(root: AccessibilityNodeInfo, ui: BrowserUi, toolbar: AccessibilityNodeInfo?): BrowserScreen {
    fun find(node: UiNode) = root.findVisible(node, toolbar)
    return BrowserScreen(
        privateMarker = (ui.privateMode as? PrivateMode.Marker)?.let { find(it.marker) } != null,
        newTabPage = ui.newTabPage?.let(::find) != null,
        bar = find(ui.urlBar)?.let { UrlBar(it.text, it.contentDescription, it.isFocused) },
    )
}

private fun AccessibilityNodeInfo.findVisible(node: UiNode, toolbar: AccessibilityNodeInfo?): AccessibilityNodeInfo? = when (node) {
    is UiNode.ViewId -> findAccessibilityNodeInfosByViewId(node.id).firstOrNull { it.isVisibleToUser }
    is UiNode.TestTag -> (findAccessibilityNodeInfosByViewId(node.container) + listOfNotNull(toolbar))
        .filter { it.viewIdResourceName == node.container && it.isVisibleToUser }
        .firstNotNullOfOrNull { it.visibleDescendant(node.tag) }
}

private fun AccessibilityNodeInfo.visibleDescendant(viewId: String): AccessibilityNodeInfo? {
    val queue = ArrayDeque(listOf(this))
    while (queue.isNotEmpty()) {
        val node = queue.removeFirst()
        if (node.viewIdResourceName == viewId && node.isVisibleToUser) return node
        for (i in 0 until node.childCount) node.getChild(i)?.let(queue::addLast)
    }
    return null
}
