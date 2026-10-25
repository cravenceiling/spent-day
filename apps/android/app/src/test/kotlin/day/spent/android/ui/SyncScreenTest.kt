package day.spent.android.ui

import android.app.Application
import androidx.compose.ui.semantics.SemanticsProperties
import androidx.compose.ui.test.SemanticsMatcher
import androidx.compose.ui.test.assert
import androidx.compose.ui.test.hasSetTextAction
import androidx.compose.ui.test.hasText
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onNodeWithContentDescription
import androidx.compose.ui.test.performClick
import androidx.compose.ui.test.performTextInput
import androidx.compose.ui.text.AnnotatedString
import day.spent.android.SyncState
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

@RunWith(RobolectricTestRunner::class)
@Config(application = Application::class)
class SyncScreenTest {
    @get:Rule val compose = createComposeRule()

    private val isPassword = SemanticsMatcher.keyIsDefined(SemanticsProperties.Password)
    private fun shows(text: String) = SemanticsMatcher.expectValue(SemanticsProperties.EditableText, AnnotatedString(text))

    @Test
    fun aTypedSyncCodeIsMaskedForAccessibilityUntilRevealed() {
        compose.setContent { SyncScreen(SyncState(), SyncActions({}, {}, { _, _ -> }, {}, {})) }
        val field = compose.onNode(hasSetTextAction() and hasText("Sync code"))
        field.performTextInput("abcd-efgh")

        field.assert(isPassword and shows("•••••••••"))

        compose.onNodeWithContentDescription("Show sync code").performClick()
        field.assert(shows("abcd-efgh"))

        compose.onNodeWithContentDescription("Hide sync code").performClick()
        field.assert(isPassword and shows("•••••••••"))
    }
}
