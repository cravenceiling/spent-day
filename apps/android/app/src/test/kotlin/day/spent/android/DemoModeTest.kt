package day.spent.android

import android.app.Application
import android.content.Intent
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createEmptyComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import androidx.test.core.app.ActivityScenario
import androidx.test.core.app.ApplicationProvider
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * Demo mode exists for screenshots, so its sync screen must not hold the real sync code. The app
 * here is a plain Application, so anything that reached for the real core would fail.
 */
@RunWith(RobolectricTestRunner::class)
@Config(application = Application::class)
class DemoModeTest {
    @get:Rule val compose = createEmptyComposeRule()

    @Test
    fun demoSyncNeverBuildsTheRealSyncViewModel() {
        val intent = Intent(ApplicationProvider.getApplicationContext(), MainActivity::class.java).putExtra("demo", true)
        ActivityScenario.launch<MainActivity>(intent).use { scenario ->
            compose.onNodeWithContentDescription("Sync settings").performClick()
            compose.onNodeWithText("https://sync.example.com").assertIsDisplayed()
            scenario.onActivity { activity ->
                val keys = activity.viewModelStore.keys()
                // The dashboard's model shows up here, so a SyncViewModel would too.
                assertTrue(keys.toString(), keys.any { DashboardViewModel::class.java.name in it })
                assertFalse(keys.toString(), keys.any { SyncViewModel::class.java.name in it })
            }
        }
    }
}
