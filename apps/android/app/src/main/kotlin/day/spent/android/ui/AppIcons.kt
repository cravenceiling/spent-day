package day.spent.android.ui

import android.content.pm.PackageManager
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.core.graphics.drawable.toBitmap
import java.util.concurrent.ConcurrentHashMap
import uniffi.spentday_core.AppUsage

/**
 * Finds the local icon for a dashboard row from the platform ids behind it. Rows for apps that only
 * exist on other devices get no icon. Reads never wait on a load, so the first draw doesn't block.
 */
class AppIcons(private val pm: PackageManager) {
    /** ConcurrentHashMap holds no nulls, so a row without a local app is stored as [Icon] with no bitmap. */
    private class Icon(val bitmap: ImageBitmap?)

    private val cache = ConcurrentHashMap<String, Icon>()

    fun peek(app: AppUsage): ImageBitmap? = cache[app.appKey]?.bitmap

    fun load(app: AppUsage): ImageBitmap? {
        cache[app.appKey]?.let { return it.bitmap }
        val bitmap = app.appIds.firstNotNullOfOrNull { id ->
            runCatching { pm.getApplicationIcon(id).toBitmap(96, 96).asImageBitmap() }.getOrNull()
        }
        return cache.getOrPut(app.appKey) { Icon(bitmap) }.bitmap
    }
}
