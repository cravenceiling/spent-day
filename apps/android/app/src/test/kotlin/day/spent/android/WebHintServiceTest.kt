package day.spent.android

import android.app.Application
import android.view.accessibility.AccessibilityNodeInfo
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers
import org.robolectric.util.ReflectionHelpers.ClassParameter

@RunWith(RobolectricTestRunner::class)
@Config(application = Application::class)
class WebHintServiceTest {
    private val firefox = browsers.getValue("org.mozilla.firefox")

    private fun node(id: String?, visible: Boolean = true, description: String? = null, vararg children: AccessibilityNodeInfo) =
        AccessibilityNodeInfo.obtain().apply {
            viewIdResourceName = id
            isVisibleToUser = visible
            contentDescription = description
            children.forEach { shadowOf(this).addChild(it) }
            ReflectionHelpers.callInstanceMethod<Unit>(this, "setSealed", ClassParameter.from(Boolean::class.javaPrimitiveType, true))
        }

    private fun firefoxToolbar(vararg children: AccessibilityNodeInfo) = node("org.mozilla.firefox:id/composable_toolbar", children = children)

    private fun shown(toolbar: AccessibilityNodeInfo) = shownPage(firefox, readScreen(node(null, children = arrayOf(toolbar)), firefox, toolbar))

    @Test
    fun aVisibleBadgeAfterAnInvisibleOneIsPrivate() {
        val toolbar = firefoxToolbar(
            node("TabCounterTestTags.privacy_badge", visible = false),
            node("TabCounterTestTags.privacy_badge"),
            node("ADDRESSBAR_URL_BOX", description = "example.com/secret"),
        )
        assertEquals(ShownPage.NoPage, shown(toolbar))
    }

    @Test
    fun aVisibleUrlBarAfterAnInvisibleOneIsRead() {
        val toolbar = firefoxToolbar(
            node("ADDRESSBAR_URL_BOX", visible = false, description = "old.example.com"),
            node("ADDRESSBAR_URL_BOX", description = "new.example.com"),
        )
        assertEquals(ShownPage.Web("https://new.example.com"), shown(toolbar))
    }
}
