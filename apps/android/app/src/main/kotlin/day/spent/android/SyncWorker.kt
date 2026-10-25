package day.spent.android

import android.content.Context
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Every 15 minutes while online: ingest new UsageStats events, then sync if sync is set up. */
class SyncWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        // The dashboard explains a database that won't open. Here there is nothing to do but stop.
        val spentDay = try {
            applicationContext.spentDayApp.spentDay
        } catch (_: CantOpenData) {
            return@withContext Result.failure()
        }
        UsageCollector.collect(applicationContext, spentDay)
        SyncLog.syncIfConfigured(applicationContext, spentDay)
        Result.success()
    }

    companion object {
        fun schedule(context: Context) {
            val online = Constraints.Builder().setRequiredNetworkType(NetworkType.CONNECTED).build()
            val request = PeriodicWorkRequestBuilder<SyncWorker>(15, TimeUnit.MINUTES).setConstraints(online).build()
            WorkManager.getInstance(context)
                .enqueueUniquePeriodicWork("sync", ExistingPeriodicWorkPolicy.UPDATE, request)
        }
    }
}
