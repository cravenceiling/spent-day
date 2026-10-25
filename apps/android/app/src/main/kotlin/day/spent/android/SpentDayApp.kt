package day.spent.android

import android.app.Application
import android.os.Build
import android.provider.Settings
import java.io.File
import uniffi.spentday_core.SpentDay
import uniffi.spentday_core.SpentDayException
import uniffi.spentday_core.Platform

class SpentDayApp : Application() {
    /** Opened on first use, off the main thread. A failed open throws [CantOpenData] and is retried on the next access. */
    val spentDay: SpentDay by lazy {
        try {
            SpentDay.open(File(filesDir, "spentday.db").path, deviceName(), Platform.ANDROID)
        } catch (e: SpentDayException) {
            throw CantOpenData(e)
        }
    }

    override fun onCreate() {
        super.onCreate()
        CollectWorker.schedule(this)
        SyncWorker.schedule(this)
    }

    private fun deviceName(): String =
        Settings.Global.getString(contentResolver, Settings.Global.DEVICE_NAME)?.takeIf { it.isNotBlank() }
            ?: Build.MODEL
}

val android.content.Context.spentDayApp: SpentDayApp get() = applicationContext as SpentDayApp
