package day.spent.android

import android.net.Uri
import java.net.IDN
import java.net.InetAddress
import java.text.Normalizer
import java.util.Locale

/** A node in a browser's accessibility tree. */
sealed interface UiNode {
    /** What the node reports as its `viewIdResourceName`. */
    val resourceName: String

    /** An Android view id, found with the framework's id search. */
    data class ViewId(val id: String) : UiNode {
        override val resourceName get() = id
    }

    /**
     * A Compose test tag exposed as a view id. The framework's id search can't see those, so the
     * service walks [container]'s subtree for it.
     */
    data class TestTag(val container: String, val tag: String) : UiNode {
        override val resourceName get() = tag
    }
}

sealed interface PrivateMode {
    /** Private tabs show [marker] and normal tabs don't. */
    data class Marker(val marker: UiNode) : PrivateMode

    /** The browser has no private tabs. */
    data object None : PrivateMode
}

/**
 * How to read one browser. A null [privateMode] means no known way to tell a private tab from a
 * normal one, so the service reads nothing from that browser until a marker is added here.
 * A change inside one of the [watched] views, which hold the URL bar, the private marker or the new
 * tab page, triggers a read; with none, only changes to the URL bar itself do. [newTabPage] is shown
 * on a new tab that has no toolbar.
 */
data class BrowserUi(
    val urlBar: UiNode,
    val privateMode: PrivateMode?,
    val watched: Set<String> = emptySet(),
    val newTabPage: UiNode? = null,
    val urlInDescription: Boolean = false,
)

private fun chromium(pkg: String) = BrowserUi(
    urlBar = UiNode.ViewId("$pkg:id/url_bar"),
    privateMode = PrivateMode.Marker(UiNode.ViewId("$pkg:id/location_bar_incognito_badge")),
    // The new tab page's feed renders after the toolbar has gone, so its changes count too.
    watched = setOf("$pkg:id/toolbar", "$pkg:id/feed_stream_recycler_view"),
    newTabPage = UiNode.ViewId("$pkg:id/search_box_text"),
)

/** Browser package to how its URL bar and private mode look. */
val browsers: Map<String, BrowserUi> = mapOf(
    "com.android.chrome" to chromium("com.android.chrome"),
    "com.chrome.beta" to chromium("com.chrome.beta"),
    // The address bar is Compose: the URL is the start of its content description. The home page
    // and the page view each have their own toolbar.
    "org.mozilla.firefox" to BrowserUi(
        urlBar = UiNode.TestTag("org.mozilla.firefox:id/composable_toolbar", "ADDRESSBAR_URL_BOX"),
        privateMode = PrivateMode.Marker(UiNode.TestTag("org.mozilla.firefox:id/composable_toolbar", "TabCounterTestTags.privacy_badge")),
        watched = setOf("org.mozilla.firefox:id/composable_toolbar"),
        urlInDescription = true,
    ),
    // A private tab's toolbar has the same views as a normal one.
    "com.brave.browser" to BrowserUi(UiNode.ViewId("com.brave.browser:id/url_bar"), privateMode = null),
    "com.microsoft.emmx" to BrowserUi(UiNode.ViewId("com.microsoft.emmx:id/url_bar"), privateMode = null),
    "com.sec.android.app.sbrowser" to BrowserUi(UiNode.ViewId("com.sec.android.app.sbrowser:id/location_bar_edit_text"), privateMode = null),
    "com.opera.browser" to BrowserUi(UiNode.ViewId("com.opera.browser:id/url_field"), privateMode = null),
    "com.duckduckgo.mobile.android" to BrowserUi(
        UiNode.ViewId("com.duckduckgo.mobile.android:id/omnibarTextInput"),
        privateMode = PrivateMode.None,
    ),
)

/** The browsers the service reads. The accessibility config lists the same packages. */
val readBrowsers: Map<String, BrowserUi> = browsers.filterValues { it.privateMode != null }

/** How far up from a changed node to look for a watched view. Each step is a call into the browser. */
const val MAX_ANCESTRY = 25

/**
 * Whether a content change can change what [browser]'s toolbar shows. [sourceUp] is the view id of
 * the changed node, then of each of its ancestors.
 */
fun isToolbarChange(browser: BrowserUi, sourceUp: Sequence<String?>): Boolean {
    val marker = (browser.privateMode as? PrivateMode.Marker)?.marker?.resourceName
    val watched = browser.watched + setOfNotNull(browser.urlBar.resourceName, browser.newTabPage?.resourceName)
    return sourceUp.take(MAX_ANCESTRY).withIndex().any { (i, id) -> id in watched || i == 0 && id == marker }
}

/** What a browser's URL bar node shows. */
data class UrlBar(val text: CharSequence?, val description: CharSequence?, val focused: Boolean)

/** The parts of a browser's window that tell which page it shows. Each is absent when not visible. */
data class BrowserScreen(val privateMarker: Boolean = false, val newTabPage: Boolean = false, val bar: UrlBar?)

sealed interface ShownPage {
    data class Web(val url: String) : ShownPage

    /** A new tab, an internal page, search text being shown, or a private tab. */
    data object NoPage : ShownPage
}

/**
 * The page to report for what [browser] shows, or null when there is nothing to report: the URL
 * bar is out of view or being edited. A private tab is always [ShownPage.NoPage], whatever its URL.
 */
fun shownPage(browser: BrowserUi, screen: BrowserScreen): ShownPage? {
    if (browser.privateMode == null) return null
    if (screen.privateMarker) return ShownPage.NoPage
    val bar = screen.bar ?: return ShownPage.NoPage.takeIf { screen.newTabPage }
    if (bar.focused) return null
    val text = if (browser.urlInDescription) {
        bar.description?.trim()?.split(Regex("\\s+"))?.firstOrNull()?.trimEnd('.')
    } else {
        bar.text
    }
    return urlFromUrlBar(text)?.let(ShownPage::Web) ?: ShownPage.NoPage
}

/**
 * Turns what a URL bar shows ("example.com/a", "https://x.org", "münchen.de") into an http(s) URL
 * with a lowercased host. Returns null for search text, other schemes and anything else that is
 * not a web address. Text with no scheme counts only when it starts with a host, so
 * "user@example.com" is not a page. Hosts stay in Unicode, in NFC; the core turns them into its
 * domain key.
 */
fun urlFromUrlBar(text: CharSequence?): String? {
    val raw = Normalizer.normalize(text?.trim() ?: "", Normalizer.Form.NFC)
    if (raw.isEmpty() || raw.any { it.isWhitespace() }) return null
    val uri = Uri.parse(if (SCHEMELESS.matches(raw)) "https://$raw" else raw)
    val scheme = uri.scheme?.lowercase(Locale.ROOT)
    if (scheme != "http" && scheme != "https") return null
    val hostPort = uri.encodedAuthority?.substringAfterLast('@') ?: return null
    val host = webHost(uri.host ?: return null) ?: return null
    val hasPort = ':' in hostPort.substringAfterLast(']')
    if (hasPort && uri.port !in 1..65_535) return null
    val port = if (hasPort) ":${uri.port}" else ""
    val query = uri.encodedQuery?.let { "?$it" }.orEmpty()
    val fragment = uri.encodedFragment?.let { "#$it" }.orEmpty()
    return "$scheme://$host$port${uri.encodedPath.orEmpty()}$query$fragment"
}

/** A host, then an optional port, then a path, query or fragment. No userinfo, so no `@` before the path. */
private val SCHEMELESS = Regex("""(\[[0-9A-Fa-f:.]+]|[^/?#@:\[\]]+)(:\d+)?([/?#].*)?""")

/** Dots that IDNA treats as label separators, as typed on CJK keyboards. */
private val IDEOGRAPHIC_DOTS = Regex("[\u3002\uFF0E\uFF61]")

/** [host] lowercased with ASCII dots, or null when it isn't a domain, `localhost`, an IPv4 or a bracketed IPv6 address. */
private fun webHost(host: String): String? {
    if (host.startsWith('[')) return host.lowercase(Locale.ROOT).takeIf { isIpv6Literal(it.removeSurrounding("[", "]")) }
    val unicode = host.replace(IDEOGRAPHIC_DOTS, ".").lowercase(Locale.ROOT)
    val ascii = runCatching { IDN.toASCII(unicode, IDN.USE_STD3_ASCII_RULES) }.getOrNull() ?: return null
    val labels = ascii.removeSuffix(".").split('.')
    val ok = ascii == "localhost" || labels.size > 1 && labels.all { it.isNotEmpty() }
    return unicode.takeIf { ok }
}

/**
 * Only hex digits, colons and dots reach [InetAddress], so it parses a literal and never looks up a
 * name. It returns an IPv4-mapped literal such as `::ffff:192.0.2.1` as an IPv4 address.
 */
private fun isIpv6Literal(text: String): Boolean =
    ':' in text && text.all { it in '0'..'9' || it in 'a'..'f' || it == ':' || it == '.' } &&
        runCatching { InetAddress.getByName(text) }.isSuccess

/** Passes a browser's page only when it differs from the last one passed. */
class WebHintGate {
    private val last = mutableMapOf<String, ShownPage>()

    fun admit(browser: String, page: ShownPage): Boolean {
        if (last[browser] == page) return false
        last[browser] = page
        return true
    }

    /**
     * The next page passes even if unchanged. Called when the browser's window changes, so a
     * session that returns to a page left open longer than the core's 30-minute lookback still
     * gets a hint at its start.
     */
    fun forget(browser: String) {
        last.remove(browser)
    }
}

/**
 * When to read one browser's toolbar. A tab switch changes it in several steps, so a read waits
 * until changes have stopped for [QUIET_MS], but no longer than [MAX_WAIT_MS] after the first one.
 * A read that finds the bar hidden or being edited runs again [RETRY_MS] later, up to [RETRIES]
 * times. Times are on one monotonic clock.
 */
class ReadSchedule {
    /** When the next read is due, or null when none is. */
    var dueMs: Long? = null
        private set
    private var firstChangeMs: Long? = null
    private var retriesLeft = 0

    /** Whether changes are being gathered for a read, rather than waiting to retry one or idle. */
    val gathering get() = firstChangeMs != null

    fun changed(nowMs: Long) {
        val first = firstChangeMs ?: nowMs
        firstChangeMs = first
        retriesLeft = RETRIES
        dueMs = minOf(nowMs + QUIET_MS, first + MAX_WAIT_MS)
    }

    /** The read due ran at [nowMs]. [resolved] is false when it found no page to report. */
    fun read(nowMs: Long, resolved: Boolean) {
        firstChangeMs = null
        if (resolved || retriesLeft == 0) {
            dueMs = null
            return
        }
        retriesLeft--
        dueMs = nowMs + RETRY_MS
    }

    companion object {
        const val QUIET_MS = 300L
        const val MAX_WAIT_MS = 1_000L
        const val RETRY_MS = 500L
        const val RETRIES = 3
    }
}
