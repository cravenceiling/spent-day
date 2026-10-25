package day.spent.android

import android.app.Application
import java.io.File
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(application = Application::class)
class WebHintsTest {
    @Test
    fun barTextBecomesHttpsUrl() {
        assertEquals("https://example.com/a?b=1", urlFromUrlBar("example.com/a?b=1"))
        assertEquals("https://www.youtube.com", urlFromUrlBar("  www.youtube.com "))
        assertEquals("http://intranet.local:8080/x", urlFromUrlBar("http://intranet.local:8080/x"))
        assertEquals("https://localhost:3000", urlFromUrlBar("localhost:3000"))
        assertEquals("https://example.com/Path?Q=1", urlFromUrlBar("HTTPS://Example.COM/Path?Q=1"))
    }

    @Test
    fun internationalHostsAreKept() {
        assertEquals("https://münchen.de/karte", urlFromUrlBar("münchen.de/karte"))
        assertEquals("https://xn--mnchen-3ya.de", urlFromUrlBar("xn--mnchen-3ya.de"))
        assertEquals("https://пример.рф", urlFromUrlBar("https://ПРИМЕР.РФ"))
    }

    @Test
    fun searchTextAndInternalPagesAreDropped() {
        assertNull(urlFromUrlBar("how to bake bread"))
        assertNull(urlFromUrlBar("bread"))
        assertNull(urlFromUrlBar("chrome://flags"))
        assertNull(urlFromUrlBar("about:blank"))
        assertNull(urlFromUrlBar("chrome-native://newtab/"))
        assertNull(urlFromUrlBar("Search or type URL"))
        assertNull(urlFromUrlBar("file:///sdcard/a.pdf"))
        assertNull(urlFromUrlBar("example.com:http"))
        assertNull(urlFromUrlBar(""))
        assertNull(urlFromUrlBar(null))
    }

    @Test
    fun anExplicitSchemeOtherThanHttpIsNoPage() {
        assertNull(urlFromUrlBar("data:,user@example.com"))
        assertNull(urlFromUrlBar("javascript:user@example.com"))
        assertNull(urlFromUrlBar("javascript:alert(1)//example.com"))
        assertNull(urlFromUrlBar("ftp://example.com/a"))
    }

    @Test
    fun schemelessTextNeedsAHostBeforeAnyAt() {
        assertNull(urlFromUrlBar("user@example.com"))
        assertNull(urlFromUrlBar("user:pass@example.com"))
        assertEquals("https://example.com/@user", urlFromUrlBar("example.com/@user"))
    }

    @Test
    fun portsMustBeInRange() {
        assertNull(urlFromUrlBar("example.com:65536"))
        assertNull(urlFromUrlBar("https://example.com:0/"))
        assertEquals("https://example.com:65535", urlFromUrlBar("example.com:65535"))
    }

    @Test
    fun bracketedIpv6HostsAreKept() {
        assertEquals("http://[::1]/", urlFromUrlBar("http://[::1]/"))
        assertEquals("https://[2001:db8::1]:8080/x", urlFromUrlBar("https://[2001:db8::1]:8080/x"))
        assertNull(urlFromUrlBar("https://[not-an-ip]/"))
    }

    @Test
    fun ipv4MappedIpv6HostsAreKept() {
        assertEquals("https://[::ffff:192.0.2.1]/a", urlFromUrlBar("[::ffff:192.0.2.1]/a"))
        assertEquals("http://[::ffff:c000:201]:8080", urlFromUrlBar("http://[::FFFF:C000:201]:8080"))
    }

    @Test
    fun hostsAreNormalizedToOneForm() {
        assertEquals("https://\u00e9xample.com", urlFromUrlBar("e\u0301xample.com"))
        assertEquals("https://example.com/a", urlFromUrlBar("example\u3002com/a"))
    }

    @Test
    fun onlyAChangedPageIsSent() {
        val gate = WebHintGate()
        val a = ShownPage.Web("https://a.com")
        assertTrue(gate.admit("chrome", a))
        assertFalse(gate.admit("chrome", ShownPage.Web("https://a.com")))
        assertTrue(gate.admit("firefox", a))
        assertTrue(gate.admit("chrome", ShownPage.NoPage))
        assertFalse(gate.admit("chrome", ShownPage.NoPage))
        assertTrue(gate.admit("chrome", a))
    }

    @Test
    fun aWindowChangeResendsTheSamePage() {
        val gate = WebHintGate()
        val a = ShownPage.Web("https://a.com")
        assertTrue(gate.admit("chrome", a))
        gate.forget("chrome")
        assertTrue(gate.admit("chrome", a))
        assertFalse(gate.admit("chrome", a))
    }

    private val chrome = browsers.getValue("com.android.chrome")
    private val firefox = browsers.getValue("org.mozilla.firefox")

    @Test
    fun aPrivateTabIsNoPageWhateverItsUrl() {
        val bar = UrlBar("example.com/secret", null, focused = false)
        assertEquals(ShownPage.NoPage, shownPage(chrome, BrowserScreen(privateMarker = true, bar = bar)))
        assertEquals(ShownPage.NoPage, shownPage(chrome, BrowserScreen(privateMarker = true, bar = null)))
        assertEquals(ShownPage.NoPage, shownPage(chrome, BrowserScreen(privateMarker = true, bar = bar.copy(focused = true))))
        assertEquals(ShownPage.Web("https://example.com/secret"), shownPage(chrome, BrowserScreen(bar = bar)))
    }

    @Test
    fun privateMarkersAreTheBadgesSeenOnDevice() {
        assertEquals(
            PrivateMode.Marker(UiNode.ViewId("com.android.chrome:id/location_bar_incognito_badge")),
            chrome.privateMode,
        )
        assertEquals(
            PrivateMode.Marker(UiNode.TestTag("org.mozilla.firefox:id/composable_toolbar", "TabCounterTestTags.privacy_badge")),
            firefox.privateMode,
        )
        assertEquals(PrivateMode.None, browsers.getValue("com.duckduckgo.mobile.android").privateMode)
    }

    @Test
    fun browsersWithoutAKnownPrivateMarkerAreNotRead() {
        val brave = browsers.getValue("com.brave.browser")
        assertNull(shownPage(brave, BrowserScreen(bar = UrlBar("example.com", null, focused = false))))
        assertEquals(
            setOf("com.android.chrome", "com.chrome.beta", "org.mozilla.firefox", "com.duckduckgo.mobile.android"),
            readBrowsers.keys,
        )
    }

    @Test
    fun accessibilityConfigListsExactlyTheReadBrowsers() {
        val xml = File("src/main/res/xml/web_hint_service.xml").readText()
        val listed = Regex("""android:packageNames="([^"]*)"""").find(xml)!!.groupValues[1].split(',').toSet()
        assertEquals(readBrowsers.keys, listed)
    }

    @Test
    fun aFocusedOrHiddenBarReportsNothing() {
        assertNull(shownPage(chrome, BrowserScreen(bar = UrlBar("example.com", null, focused = true))))
        assertNull(shownPage(chrome, BrowserScreen(bar = null)))
    }

    @Test
    fun newTabAndSearchTextAreNoPage() {
        assertEquals(ShownPage.NoPage, shownPage(chrome, BrowserScreen(bar = UrlBar("Search or type URL", null, focused = false))))
        assertEquals(ShownPage.NoPage, shownPage(chrome, BrowserScreen(bar = UrlBar("chrome://newtab", null, focused = false))))
    }

    @Test
    fun chromesNewTabPageIsNoPageThoughItHidesTheToolbar() {
        assertEquals(ShownPage.NoPage, shownPage(chrome, BrowserScreen(newTabPage = true, bar = null)))
        assertEquals(
            ShownPage.Web("https://example.com"),
            shownPage(chrome, BrowserScreen(newTabPage = true, bar = UrlBar("example.com", null, focused = false))),
        )
    }

    @Test
    fun aToolbarOutOfViewKeepsTheLastPage() {
        assertNull(shownPage(firefox, BrowserScreen(bar = null)))
        assertNull(shownPage(chrome, BrowserScreen(bar = null)))
    }

    private fun ids(vararg chain: String?) = chain.asSequence()

    @Test
    fun changesInsideTheToolbarAreRead() {
        val ff = "org.mozilla.firefox:id/composable_toolbar"
        assertTrue(isToolbarChange(firefox, ids("ADDRESSBAR_URL_BOX", null, null, null, null, ff, "org.mozilla.firefox:id/browserLayout")))
        assertTrue(isToolbarChange(firefox, ids(null, null, ff, null, null, "org.mozilla.firefox:id/homepageView")))
        assertTrue(isToolbarChange(firefox, ids("TabCounterTestTags.privacy_badge")))
        assertTrue(isToolbarChange(chrome, ids("com.android.chrome:id/toolbar_buttons", "com.android.chrome:id/toolbar", null)))
        assertTrue(isToolbarChange(chrome, ids("com.android.chrome:id/location_bar_incognito_badge")))
        assertTrue(isToolbarChange(chrome, ids("com.android.chrome:id/url_bar")))
        assertTrue(isToolbarChange(chrome, ids(null, null, null, "com.android.chrome:id/feed_stream_recycler_view", null)))
    }

    @Test
    fun changesOutsideTheToolbarAreNot() {
        assertFalse(isToolbarChange(firefox, ids("top_sites_list", "homepage.view", null, null, "org.mozilla.firefox:id/homepageView")))
        assertFalse(isToolbarChange(chrome, ids(null, null, null)))
        assertFalse(isToolbarChange(chrome, ids("com.android.chrome:id/compositor_view_holder", null)))
    }

    @Test
    fun theWalkStopsAtTheFirstWatchedAncestor() {
        var walked = 0
        val chain = ids("x", "com.android.chrome:id/toolbar", "y", "z").onEach { walked++ }
        assertTrue(isToolbarChange(chrome, chain))
        assertEquals(2, walked)
    }

    @Test
    fun aWatchedAncestorBeyondTheDepthCapIsNotWalkedTo() {
        var walked = 0
        val deep = (List<String?>(MAX_ANCESTRY) { null } + "com.android.chrome:id/toolbar").asSequence().onEach { walked++ }
        assertFalse(isToolbarChange(chrome, deep))
        assertEquals(MAX_ANCESTRY, walked)
        assertTrue(isToolbarChange(chrome, (List<String?>(MAX_ANCESTRY - 1) { null } + "com.android.chrome:id/toolbar").asSequence()))
    }

    @Test
    fun firefoxUrlIsReadFromTheDescription() {
        assertEquals(
            ShownPage.Web("https://en.wikipedia.org/wiki/Munich"),
            shownPage(firefox, BrowserScreen(bar = UrlBar(null, " en.wikipedia.org/wiki/Munich. Search or enter address", focused = false))),
        )
        assertEquals(ShownPage.NoPage, shownPage(firefox, BrowserScreen(bar = UrlBar(null, "Search", focused = false))))
    }
}
