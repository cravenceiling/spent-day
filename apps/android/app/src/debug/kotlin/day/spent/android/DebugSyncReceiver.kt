package day.spent.android

import android.app.Activity
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import uniffi.spentday_core.SyncConfig

/**
 * Lets scripts join a sync account and sync without typing a 69-character code over adb:
 *
 *   adb shell am broadcast -n day.spent.android/.DebugSyncReceiver [--es server URL --es code CODE]
 *
 * Stores the config when both extras are given, then ingests and syncs as the sync screen does.
 * `am broadcast` prints `result=-1` and the sync screen's message on success, `result=0` and the
 * error otherwise.
 *
 * The code travels as an `am` argument, so the shell and root can see it in the process list
 * while the command runs. Use it only with test accounts. Release builds don't include this
 * receiver.
 */
class DebugSyncReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val pending = goAsync()
        val server = intent.getStringExtra("server")
        val code = intent.getStringExtra("code")
        Thread {
            val outcome = runCatching {
                val spentDay = context.spentDayApp.spentDay
                if (server != null && code != null) spentDay.setSyncConfig(SyncConfig(server, code))
                UsageCollector.collect(context, spentDay)
                spentDay.syncNow()
            }
            SyncLog.record(context, outcome)
            pending.resultCode = if (outcome.isSuccess) Activity.RESULT_OK else Activity.RESULT_CANCELED
            pending.resultData = outcome.fold(::describe, ::describe)
            pending.finish()
        }.start()
    }
}
