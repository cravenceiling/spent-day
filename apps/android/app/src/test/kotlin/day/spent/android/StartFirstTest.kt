package day.spent.android

import android.app.Application
import android.content.Intent
import android.content.pm.ActivityInfo
import android.content.pm.ResolveInfo
import android.net.Uri
import android.provider.Settings
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(application = Application::class)
class StartFirstTest {
    private val app = RuntimeEnvironment.getApplication().also { shadowOf(it).checkActivities(true) }
    private fun screen(intent: Intent) = intent.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    private val usageAccess = screen(Intent(Settings.ACTION_USAGE_ACCESS_SETTINGS))
    private val appInfo = screen(Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", "day.spent.android", null)))
    private val settings = screen(Intent(Settings.ACTION_SETTINGS))

    private fun installScreenFor(intent: Intent) = shadowOf(app.packageManager).addResolveInfoForIntent(
        intent,
        ResolveInfo().apply { activityInfo = ActivityInfo().apply { packageName = "com.android.settings"; name = "Screen" } },
    )

    @Test
    fun aDeviceWithNoneOfTheScreensOpensNothingAndDoesNotCrash() {
        assertFalse(app.startFirst(usageAccess, appInfo))
        assertNull(shadowOf(app).nextStartedActivity)
    }

    @Test
    fun theFirstScreenTheDeviceHasOpens() {
        installScreenFor(settings)
        assertTrue(app.startFirst(usageAccess, appInfo, settings))
        assertEquals(Settings.ACTION_SETTINGS, shadowOf(app).nextStartedActivity.action)
    }
}
