package day.spent.android.ui

import android.app.Application
import androidx.compose.ui.test.assertIsDisplayed
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithText
import androidx.compose.ui.test.performClick
import day.spent.android.DashboardState
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(application = Application::class)
class DashboardErrorTest {
    @get:Rule val compose = createComposeRule()

    private var appInfoTaps = 0
    private val actions = DashboardActions({}, {}, {}, {}, {}, {}, { appInfoTaps++ }, {})

    /** Permissions granted, so no onboarding card brings its own App info button. */
    private fun show(oldDatabase: Boolean) = compose.setContent {
        DashboardScreen(DashboardState(loading = false, webTracking = true, error = "It broke.", oldDatabase = oldDatabase), actions)
    }

    @Test
    fun anOldDatabaseOffersAppInfo() {
        show(oldDatabase = true)
        compose.onNodeWithText("App info").assertIsDisplayed().performClick()
        assertEquals(1, appInfoTaps)
    }

    @Test
    fun otherErrorsDoNot() {
        show(oldDatabase = false)
        compose.onNodeWithText("It broke.").assertIsDisplayed()
        compose.onNodeWithText("App info").assertDoesNotExist()
    }
}
