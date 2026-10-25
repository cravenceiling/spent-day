package day.spent.android

import android.content.ActivityNotFoundException
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.Settings
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.lifecycle.compose.LifecycleResumeEffect
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.viewmodel.compose.viewModel
import day.spent.android.ui.DashboardActions
import day.spent.android.ui.DashboardScreen
import day.spent.android.ui.SpentDayTheme
import day.spent.android.ui.SyncActions
import day.spent.android.ui.SyncScreen

class MainActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        enableEdgeToEdge()
        // Demo mode shows made-up data for screenshots. Release builds ignore the extra.
        val demo = BuildConfig.DEBUG && intent.getBooleanExtra("demo", false)
        setContent {
            SpentDayTheme {
                var showSync by rememberSaveable { mutableStateOf(false) }
                if (showSync) {
                    BackHandler { showSync = false }
                    if (demo) DemoSync(back = { showSync = false }) else RealSync(back = { showSync = false })
                } else {
                    val model: DashboardViewModel = viewModel { DashboardViewModel(application, demo) }
                    val state by model.state.collectAsStateWithLifecycle()
                    LifecycleResumeEffect(Unit) {
                        model.refresh()
                        onPauseOrDispose {}
                    }
                    DashboardScreen(
                        state,
                        DashboardActions(
                            refresh = model::refresh,
                            selectRange = model::selectRange,
                            selectDevice = model::selectDevice,
                            openSync = { showSync = true },
                            grantUsageAccess = { openSettings(Intent(Settings.ACTION_USAGE_ACCESS_SETTINGS)) },
                            enableWebTracking = { openSettings(Intent(Settings.ACTION_ACCESSIBILITY_SETTINGS)) },
                            openAppInfo = { openSettings() },
                            dismissWebCard = model::dismissWebCard,
                        ),
                    )
                }
            }
        }
    }

    /** The real sync settings. They show the sync code, so screenshots and screen recordings are blocked. */
    @Composable
    private fun RealSync(back: () -> Unit) {
        DisposableEffect(Unit) {
            window.addFlags(WindowManager.LayoutParams.FLAG_SECURE)
            onDispose { window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE) }
        }
        val model: SyncViewModel = viewModel()
        val state by model.state.collectAsStateWithLifecycle()
        SyncScreen(
            state,
            SyncActions(back = back, create = model::create, join = model::join, syncNow = model::sync, turnOff = model::turnOff),
        )
    }

    /** Made-up sync settings for screenshots. It never reads the real config, and its buttons do nothing. */
    @Composable
    private fun DemoSync(back: () -> Unit) {
        val state = remember { demoSyncState() }
        SyncScreen(state, SyncActions(back = back, create = {}, join = { _, _ -> }, syncNow = {}, turnOff = {}))
    }

    /** Some devices ship without some settings screens, so this falls back to App info, then Settings. */
    private fun openSettings(vararg screens: Intent) {
        startFirst(
            *screens,
            Intent(Settings.ACTION_APPLICATION_DETAILS_SETTINGS, Uri.fromParts("package", packageName, null)),
            Intent(Settings.ACTION_SETTINGS),
        )
    }
}

/** Opens the first of [intents] the device has a screen for. Returns false when it has none. */
fun Context.startFirst(vararg intents: Intent): Boolean = intents.any { intent ->
    try {
        startActivity(intent)
        true
    } catch (_: ActivityNotFoundException) {
        false
    }
}
