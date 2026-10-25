package day.spent.android.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.material3.MaterialTheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.dp
import java.time.Instant
import java.time.ZoneId
import java.time.format.DateTimeFormatter

private const val DAY_MS = 86_400_000L
private val niceSteps = listOf(5, 10, 15, 30, 60, 120, 180, 240, 360, 480, 600, 720, 960, 1440).map { it * 60_000L }

/** Bars per hour for a day, or per day for a week. Gridlines at half and full scale. */
@Composable
fun UsageChart(buckets: List<Long>, startMs: Long, bucketMs: Long, modifier: Modifier = Modifier) {
    val colors = MaterialTheme.colorScheme
    val labelStyle = MaterialTheme.typography.labelSmall.copy(color = colors.onSurfaceVariant)
    val measurer = rememberTextMeasurer()
    val daily = bucketMs >= DAY_MS
    val peak = buckets.maxOrNull() ?: 0L
    val scaleMax = niceSteps.firstOrNull { it >= peak } ?: peak
    val labels = remember(startMs, bucketMs, buckets.size) {
        val zone = ZoneId.systemDefault()
        val format = DateTimeFormatter.ofPattern(if (daily) "EEE" else "h a")
        List(buckets.size) { i ->
            val labelled = daily || i % 6 == 0
            if (labelled) format.format(Instant.ofEpochMilli(startMs + i * bucketMs).atZone(zone)) else null
        }
    }

    Canvas(
        modifier
            .fillMaxWidth()
            .height(176.dp)
            .semantics { contentDescription = "Usage chart, peak ${formatDuration(peak)}" },
    ) {
        val axisWidth = 40.dp.toPx()
        val labelHeight = 22.dp.toPx()
        val top = 8.dp.toPx()
        val bottom = size.height - labelHeight
        val chartWidth = size.width - axisWidth
        val chartHeight = bottom - top
        val dash = PathEffect.dashPathEffect(floatArrayOf(4.dp.toPx(), 4.dp.toPx()))

        for (fraction in listOf(0.5f, 1f)) {
            val y = bottom - chartHeight * fraction
            drawLine(colors.outlineVariant, Offset(0f, y), Offset(chartWidth, y), 1.dp.toPx(), pathEffect = dash)
            val text = measurer.measure(formatDuration((scaleMax * fraction).toLong()), labelStyle)
            drawText(text, topLeft = Offset(size.width - text.size.width, y - text.size.height / 2))
        }
        drawLine(colors.outlineVariant, Offset(0f, bottom), Offset(chartWidth, bottom), 1.dp.toPx())

        val slot = chartWidth / buckets.size.coerceAtLeast(1)
        val barWidth = slot * if (daily) 0.5f else 0.62f
        val radius = minOf(barWidth / 2, 4.dp.toPx())
        buckets.forEachIndexed { i, value ->
            val left = i * slot + (slot - barWidth) / 2
            if (value > 0 && scaleMax > 0) {
                val height = (chartHeight * value / scaleMax).coerceAtLeast(2.dp.toPx())
                val bar = Path().apply {
                    addRoundRect(
                        RoundRect(
                            left, bottom - height, left + barWidth, bottom,
                            topLeftCornerRadius = CornerRadius(radius),
                            topRightCornerRadius = CornerRadius(radius),
                        ),
                    )
                }
                drawPath(bar, colors.primary)
            }
            labels[i]?.let { label ->
                val text = measurer.measure(label, labelStyle)
                val x = if (daily) left + barWidth / 2 - text.size.width / 2 else left
                drawText(text, topLeft = Offset(x.coerceIn(0f, chartWidth - text.size.width), bottom + 6.dp.toPx()))
            }
        }
    }
}
