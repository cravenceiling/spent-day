package day.spent.android

import android.content.Context
import android.os.Build
import androidx.core.content.edit
import uniffi.spentday_core.SpentDay
import uniffi.spentday_core.SpentDayException
import uniffi.spentday_core.SyncReport

data class SyncResult(val atMs: Long, val message: String)

/**
 * The last sync that worked, and the last failure when it came after that. A failed run never
 * replaces the last good one, so the screen can say both "synced at 9:00" and "failed at 9:15".
 */
data class SyncHistory(val lastGood: SyncResult? = null, val failure: SyncResult? = null)

object SyncLog {
    private const val PREFS = "sync"

    fun history(context: Context): SyncHistory {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        fun read(key: String) = if (prefs.contains("${key}_at")) {
            SyncResult(prefs.getLong("${key}_at", 0), prefs.getString("${key}_message", "").orEmpty())
        } else {
            null
        }
        return SyncHistory(read("good"), read("failed"))
    }

    /** Runs a sync if one is configured and records the outcome. */
    fun syncIfConfigured(context: Context, spentDay: SpentDay) {
        if (spentDay.syncConfig() == null) return
        val outcome = try {
            Result.success(spentDay.syncNow())
        } catch (e: Exception) {
            Result.failure(e)
        }
        record(context, outcome)
    }

    /**
     * Keeps [outcome] only if it is newer than what is stored. The worker and the sync screen can
     * sync at once, and the one that started first may finish last.
     */
    @Synchronized
    fun record(context: Context, outcome: Result<SyncReport>, nowMs: Long = System.currentTimeMillis()) {
        val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
        val goodAt = prefs.getLong("good_at", Long.MIN_VALUE)
        val failedAt = prefs.getLong("failed_at", Long.MIN_VALUE)
        outcome.fold(
            onSuccess = { report ->
                if (report.finishedAtMs <= goodAt) return
                prefs.edit {
                    putLong("good_at", report.finishedAtMs)
                    putString("good_message", describe(report))
                    if (failedAt <= report.finishedAtMs) {
                        remove("failed_at")
                        remove("failed_message")
                    }
                }
            },
            onFailure = { e ->
                if (nowMs <= goodAt || nowMs <= failedAt) return
                prefs.edit {
                    putLong("failed_at", nowMs)
                    putString("failed_message", describe(e))
                }
            },
        )
    }
}

fun describe(report: SyncReport): String {
    val counts = "Sent ${report.pushedSpans}, received ${report.pulledSpans}"
    return when (val skipped = report.skippedBatches) {
        0uL -> counts
        1uL -> "$counts. Skipped 1 batch from the server that couldn't be read."
        else -> "$counts. Skipped $skipped batches from the server that couldn't be read."
    }
}

/** The core couldn't open the database. [cause] says why. */
class CantOpenData(override val cause: SpentDayException) : Exception(cause.message, cause) {
    /** Only a database from an early build is safe to tell the user to delete. A full disk is not. */
    val oldDatabase: Boolean get() = cause is SpentDayException.OldDatabase
}

/**
 * What the dashboard says when it can't load. Only there does a database that won't open get the
 * full story, because only there is the App info button that leads to Clear storage.
 */
fun describeLoadFailure(e: Throwable, sdk: Int = Build.VERSION.SDK_INT): String = when {
    e is CantOpenData && e.oldDatabase -> {
        val storage = if (sdk >= Build.VERSION_CODES.S) "Storage & cache" else "Storage"
        "This data was made by an early test build of Spent Day and can't be opened. " +
            "To start over, tap App info, then $storage, then Clear storage. " +
            "That deletes the screen time and sync settings on this phone."
    }
    e is CantOpenData -> "The local data didn't open (${detail(e.cause)}). Pull down to try again. " +
        "If it keeps failing, the data on this phone may be damaged or from a newer version of Spent Day."
    else -> describe(e)
}

/** Text the core made on this phone. A server's answer never gets here, because opening the data doesn't sync. */
private fun detail(e: SpentDayException): String = when (e) {
    is SpentDayException.Storage -> e.v1
    is SpentDayException.InvalidArgument -> e.v1
    is SpentDayException.Network -> e.v1
    is SpentDayException.OldDatabase -> e.path
    is SpentDayException.Server -> "HTTP ${e.status}"
    is SpentDayException.Decrypt,
    is SpentDayException.InvalidSyncCode,
    is SpentDayException.SyncNotConfigured,
    -> e.javaClass.simpleName
}

fun describe(e: Throwable): String = when (e) {
    is CantOpenData -> "Can't open the local data. See the main screen."
    is SpentDayException -> describeCore(e)
    else -> e.message ?: e.javaClass.simpleName
}

private fun describeCore(e: SpentDayException): String = when (e) {
    is SpentDayException.Network -> "Couldn't reach the server. Check the address and your connection."
    is SpentDayException.Server -> "The server refused the request (HTTP ${e.status})."
    is SpentDayException.Decrypt -> "Data on the server can't be read with this code. Is it the right code?"
    is SpentDayException.InvalidSyncCode -> "That isn't a valid sync code."
    is SpentDayException.SyncNotConfigured -> "Sync isn't set up yet."
    is SpentDayException.Storage -> "Couldn't read the local database: ${e.v1}"
    is SpentDayException.InvalidArgument -> "Spent Day couldn't use that value. ${e.v1}"
    is SpentDayException.OldDatabase -> "Can't open the local data. See the main screen."
}
