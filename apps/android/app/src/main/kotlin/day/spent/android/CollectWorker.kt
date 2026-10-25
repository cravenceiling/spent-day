package day.spent.android

import android.content.Context
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/** Every 15 minutes: ingest new UsageStats events. [SyncWorker] sends them. */
class CollectWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result = withContext(Dispatchers.IO) {
        // The dashboard explains a database that won't open. Here there is nothing to do but stop.
        val spentDay = try {
            applicationContext.spentDayApp.spentDay
        } catch (_: CantOpenData) {
            return@withContext Result.failure()
        }
        UsageCollector.collect(applicationContext, spentDay)
        Result.success()
    }

    companion object {
        fun schedule(context: Context) {
            val request = PeriodicWorkRequestBuilder<CollectWorker>(15, TimeUnit.MINUTES).build()
            WorkManager.getInstance(context)
                .enqueueUniquePeriodicWork("collect", ExistingPeriodicWorkPolicy.UPDATE, request)
        }
    }
}
