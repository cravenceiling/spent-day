package day.spent.android

import android.app.Application
import androidx.lifecycle.AndroidViewModel
import androidx.lifecycle.viewModelScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import uniffi.spentday_core.SyncConfig

data class SyncState(
    val config: SyncConfig? = null,
    val history: SyncHistory = SyncHistory(),
    val busy: Boolean = false,
    val error: String? = null,
)

class SyncViewModel(app: Application) : AndroidViewModel(app) {
    private val spentDay get() = getApplication<Application>().spentDayApp.spentDay
    private val _state = MutableStateFlow(SyncState(history = SyncLog.history(app)))
    val state: StateFlow<SyncState> = _state

    init {
        perform { spentDay.syncConfig() }
    }

    fun create(serverUrl: String) = perform {
        spentDay.setSyncConfig(SyncConfig(serverUrl.trim(), spentDay.newSyncCode()))
        syncNow()
    }

    fun join(serverUrl: String, code: String) = perform {
        spentDay.setSyncConfig(SyncConfig(serverUrl.trim(), code.trim()))
        syncNow()
    }

    fun sync() = perform { syncNow() }

    fun turnOff() = perform { spentDay.setSyncConfig(null) }

    private fun syncNow() {
        val app = getApplication<Application>()
        UsageCollector.collect(app, spentDay)
        SyncLog.syncIfConfigured(app, spentDay)
    }

    /** Runs [block] off the main thread, then re-reads the config and last result. */
    private fun perform(block: () -> Unit) {
        _state.update { it.copy(busy = true, error = null) }
        viewModelScope.launch {
            val app = getApplication<Application>()
            val outcome = withContext(Dispatchers.IO) {
                runCatching {
                    block()
                    spentDay.syncConfig()
                }
            }
            _state.update { state ->
                outcome.fold(
                    onSuccess = { state.copy(config = it, history = SyncLog.history(app), busy = false) },
                    onFailure = { state.copy(busy = false, error = describe(it)) },
                )
            }
        }
    }
}
