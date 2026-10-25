package day.spent.android

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import java.time.LocalDate
import java.time.ZoneId
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.spentday_core.DeviceInfo
import uniffi.spentday_core.Summary
import uniffi.spentday_core.SummaryQuery

private const val HOUR_MS = 3_600_000L
private const val DAY_MS = 24 * HOUR_MS

enum class DayRange(val label: String) {
    Today("Today"),
    Yesterday("Yesterday"),
    Last7Days("Last 7 days");

    /** Local-midnight bounds. A 7-day range uses 24 h buckets, so a DST change shifts one day's edge by an hour. */
    fun query(deviceId: String?, today: LocalDate = LocalDate.now(), zone: ZoneId = ZoneId.systemDefault()): SummaryQuery {
        fun midnight(day: LocalDate): Long = day.atStartOfDay(zone).toInstant().toEpochMilli()
        val (first, last, bucket) = when (this) {
            Today -> Triple(today, today, HOUR_MS)
            Yesterday -> Triple(today.minusDays(1), today.minusDays(1), HOUR_MS)
            Last7Days -> Triple(today.minusDays(6), today, DAY_MS)
        }
        return SummaryQuery(midnight(first), midnight(last.plusDays(1)), bucket, deviceId)
    }
}

/** Where the dashboard reads from: the real core, or canned data in debug builds. */
interface UsageSource {
    fun refresh() {}
    fun devices(): List<DeviceInfo>
    fun summary(query: SummaryQuery): Summary
}

private class CoreSource(private val app: Application) : UsageSource {
    private val spentDay get() = app.spentDayApp.spentDay
    override fun refresh() {
        UsageCollector.collect(app, spentDay)
    }
    override fun devices() = spentDay.devices()
    override fun summary(query: SummaryQuery) = spentDay.summary(query)
}

data class DashboardState(
    val range: DayRange = DayRange.Today,
    val deviceId: String? = null,
    val devices: List<DeviceInfo> = emptyList(),
    val summary: Summary? = null,
    val loading: Boolean = true,
    val error: String? = null,
    /** The database is from an early build, so the error card offers App info, where the user can clear storage. */
    val oldDatabase: Boolean = false,
    val usageAccess: Boolean = true,
    val webTracking: Boolean = false,
    val webCardDismissed: Boolean = false,
)

class DashboardViewModel(app: Application, demo: Boolean) : AndroidViewModel(app) {
    private val source: UsageSource = if (demo) DemoSource() else CoreSource(app)
    private val prefs = app.getSharedPreferences("ui", Application.MODE_PRIVATE)
    private val _state = MutableStateFlow(DashboardState(webCardDismissed = prefs.getBoolean("web_card_dismissed", false)))
    val state: StateFlow<DashboardState> = _state
    private var loadJob: Job? = null

    /** Re-reads permissions, ingests new UsageStats events and reloads the summary. */
    fun refresh() {
        val app = getApplication<Application>()
        _state.update {
            it.copy(
                loading = true,
                usageAccess = source is DemoSource || UsageCollector.hasPermission(app),
                webTracking = source is DemoSource || WebHintService.isEnabled(app),
            )
        }
        load(ingest = true)
    }

    fun selectRange(range: DayRange) {
        _state.update { it.copy(range = range) }
        load(ingest = false)
    }

    fun selectDevice(deviceId: String?) {
        _state.update { it.copy(deviceId = deviceId) }
        load(ingest = false)
    }

    fun dismissWebCard() {
        prefs.edit().putBoolean("web_card_dismissed", true).apply()
        _state.update { it.copy(webCardDismissed = true) }
    }

    private fun load(ingest: Boolean) {
        loadJob?.cancel()
        loadJob = viewModelScope.launch {
            val current = _state.value
            val result = withContext(Dispatchers.IO) {
                runCatching {
                    if (ingest) source.refresh()
                    val devices = source.devices()
                    val deviceId = current.deviceId?.takeIf { id -> devices.any { it.id == id } }
                    Triple(devices, deviceId, source.summary(current.range.query(deviceId)))
                }
            }
            _state.update { state ->
                result.fold(
                    onSuccess = { (devices, deviceId, summary) ->
                        state.copy(devices = devices, deviceId = deviceId, summary = summary, loading = false, error = null, oldDatabase = false)
                    },
                    onFailure = {
                        state.copy(loading = false, error = describeLoadFailure(it), oldDatabase = (it as? CantOpenData)?.oldDatabase == true)
                    },
                )
            }
        }
    }
}
