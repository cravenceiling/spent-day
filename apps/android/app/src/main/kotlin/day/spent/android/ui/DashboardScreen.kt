package day.spent.android.ui

import android.os.Build
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyListScope
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Computer
import androidx.compose.material.icons.outlined.Devices
import androidx.compose.material.icons.outlined.ExpandMore
import androidx.compose.material.icons.outlined.Language
import androidx.compose.material.icons.outlined.Laptop
import androidx.compose.material.icons.outlined.PhoneAndroid
import androidx.compose.material.icons.outlined.Sync
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilterChip
import androidx.compose.material3.FilterChipDefaults
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.TopAppBarDefaults
import androidx.compose.material3.pulltorefresh.PullToRefreshBox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.nestedscroll.nestedScroll
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import day.spent.android.DashboardState
import day.spent.android.DayRange
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import uniffi.spentday_core.AppUsage
import uniffi.spentday_core.DeviceInfo
import uniffi.spentday_core.DomainUsage
import uniffi.spentday_core.Platform
import uniffi.spentday_core.Summary

class DashboardActions(
    val refresh: () -> Unit,
    val selectRange: (DayRange) -> Unit,
    val selectDevice: (String?) -> Unit,
    val openSync: () -> Unit,
    val grantUsageAccess: () -> Unit,
    val enableWebTracking: () -> Unit,
    val openAppInfo: () -> Unit,
    val dismissWebCard: () -> Unit,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun DashboardScreen(state: DashboardState, actions: DashboardActions) {
    val scroll = TopAppBarDefaults.pinnedScrollBehavior()
    val context = LocalContext.current
    val icons = remember { AppIcons(context.packageManager) }
    Scaffold(
        modifier = Modifier.nestedScroll(scroll.nestedScrollConnection),
        topBar = {
            TopAppBar(
                title = { Text("Spent Day", fontWeight = FontWeight.SemiBold) },
                actions = {
                    IconButton(onClick = actions.openSync) { Icon(Icons.Outlined.Sync, contentDescription = "Sync settings") }
                },
                scrollBehavior = scroll,
            )
        },
    ) { padding ->
        PullToRefreshBox(
            isRefreshing = state.loading && state.summary != null,
            onRefresh = actions.refresh,
            modifier = Modifier.padding(padding).fillMaxSize(),
        ) {
            LazyColumn(contentPadding = PaddingValues(bottom = 32.dp)) {
                onboarding(state, actions)
                item { Filters(state, actions) }
                state.error?.let { error ->
                    item { ErrorCard(error, appInfo = actions.openAppInfo.takeIf { state.oldDatabase }) }
                }
                state.summary?.let { summary -> usage(summary, state, icons) }
            }
        }
    }
}

private fun LazyListScope.onboarding(state: DashboardState, actions: DashboardActions) {
    if (!state.usageAccess) {
        item {
            NoticeCard(
                title = "Allow usage access",
                body = "Spent Day counts screen time from Android's usage history. In the list that opens, pick Spent Day and turn on “Permit usage access”.",
                primary = "Open settings" to actions.grantUsageAccess,
                highlighted = true,
            )
        }
    } else if (!state.webTracking && !state.webCardDismissed) {
        item {
            NoticeCard(
                title = "See time per website (optional)",
                body = "Spent Day's accessibility service reads the address bar in Chrome, Firefox and DuckDuckGo, skips private tabs, and reads nothing else. " +
                    "It keeps only the domain, such as youtube.com, never the full address or the page title. " +
                    "Without it, a browser shows up as one app.",
                // Android 13+ greys out accessibility for sideloaded apps until the user allows it in App info.
                note = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    "If the switch is greyed out: App info → ⋮ → Allow restricted settings"
                } else {
                    null
                },
                primary = "Turn on" to actions.enableWebTracking,
                secondary = listOfNotNull(
                    "Not now" to actions.dismissWebCard,
                    ("App info" to actions.openAppInfo).takeIf { Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU },
                ),
            )
        }
    }
}

@Composable
private fun NoticeCard(
    title: String,
    body: String,
    primary: Pair<String, () -> Unit>,
    secondary: List<Pair<String, () -> Unit>> = emptyList(),
    note: String? = null,
    highlighted: Boolean = false,
) {
    val colors = if (highlighted) {
        CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.primaryContainer)
    } else {
        CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)
    }
    Card(colors = colors, modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp).fillMaxWidth()) {
        Column(Modifier.padding(start = 20.dp, end = 20.dp, top = 18.dp, bottom = 10.dp)) {
            Text(title, style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(6.dp))
            Text(body, style = MaterialTheme.typography.bodyMedium)
            note?.let {
                Spacer(Modifier.height(8.dp))
                Text(it, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
            }
            Row(Modifier.fillMaxWidth().padding(top = 8.dp), horizontalArrangement = Arrangement.End) {
                secondary.forEach { (label, onClick) -> TextButton(onClick = onClick) { Text(label) } }
                Spacer(Modifier.width(8.dp))
                Button(onClick = primary.second) { Text(primary.first) }
            }
        }
    }
}

@Composable
private fun ErrorCard(message: String, appInfo: (() -> Unit)?) {
    Card(
        colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.errorContainer),
        modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp).fillMaxWidth(),
    ) {
        Column(Modifier.padding(20.dp)) {
            Text("Couldn't load your screen time", style = MaterialTheme.typography.titleSmall)
            Text(message, style = MaterialTheme.typography.bodyMedium)
            appInfo?.let {
                Row(Modifier.fillMaxWidth().padding(top = 8.dp), horizontalArrangement = Arrangement.End) {
                    Button(onClick = it) { Text("App info") }
                }
            }
        }
    }
}

@Composable
private fun Filters(state: DashboardState, actions: DashboardActions) {
    Column(Modifier.padding(top = 4.dp)) {
        LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            items(DayRange.entries) { range ->
                FilterChip(
                    selected = state.range == range,
                    onClick = { actions.selectRange(range) },
                    label = { Text(range.label) },
                )
            }
        }
        if (state.devices.size > 1) {
            LazyRow(contentPadding = PaddingValues(horizontal = 16.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                item {
                    DeviceChip("All devices", Icons.Outlined.Devices, state.deviceId == null) { actions.selectDevice(null) }
                }
                items(state.devices, key = { it.id }) { device ->
                    DeviceChip(device.name, device.icon(), state.deviceId == device.id) { actions.selectDevice(device.id) }
                }
            }
        }
    }
}

@Composable
private fun DeviceChip(label: String, icon: ImageVector, selected: Boolean, onClick: () -> Unit) {
    FilterChip(
        selected = selected,
        onClick = onClick,
        label = { Text(label) },
        leadingIcon = { Icon(icon, contentDescription = null, Modifier.size(FilterChipDefaults.IconSize)) },
    )
}

private fun DeviceInfo.icon(): ImageVector = when (platform) {
    Platform.ANDROID, Platform.IOS -> Icons.Outlined.PhoneAndroid
    Platform.MACOS -> Icons.Outlined.Laptop
    Platform.LINUX, Platform.WINDOWS -> Icons.Outlined.Computer
}

private fun LazyListScope.usage(summary: Summary, state: DashboardState, icons: AppIcons) {
    item {
        val scope = state.devices.firstOrNull { it.id == state.deviceId }?.name ?: "All devices"
        Column(Modifier.padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 8.dp)) {
            Text("${state.range.label} · $scope", style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(formatDuration(summary.totalMs), style = MaterialTheme.typography.displayMedium, fontWeight = FontWeight.SemiBold)
        }
    }
    item {
        Card(
            colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer),
            modifier = Modifier.padding(horizontal = 16.dp).fillMaxWidth(),
        ) {
            val bucketMs = (summary.endMs - summary.startMs) / summary.buckets.size.coerceAtLeast(1)
            UsageChart(summary.buckets, summary.startMs, bucketMs, Modifier.padding(16.dp))
        }
    }
    if (summary.totalMs == 0L) {
        item {
            Text(
                "Nothing recorded for this range yet.",
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(20.dp),
            )
        }
        return
    }
    val apps = summary.apps.filter { it.totalMs >= 1_000 }
    val sites = summary.sites.filter { it.totalMs >= 1_000 }
    val top = apps.firstOrNull()?.totalMs ?: 1L
    item { SectionHeader("Apps") }
    items(apps, key = { "app:" + it.appKey }) { app -> AppRow(app, top, icons) }
    if (sites.isNotEmpty()) {
        item { SectionHeader("Sites") }
        val topSite = sites.first().totalMs
        items(sites, key = { "site:" + it.domain }) { site -> SiteRow(site, topSite) }
    }
}

@Composable
private fun SectionHeader(title: String) {
    Text(
        title,
        style = MaterialTheme.typography.titleMedium,
        modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 28.dp, bottom = 4.dp),
    )
}

@Composable
private fun AppRow(app: AppUsage, top: Long, icons: AppIcons) {
    var expanded by rememberSaveable(app.appKey) { mutableStateOf(false) }
    val expandable = app.isBrowser && app.domains.any { it.domain.isNotEmpty() }
    val chevron by animateFloatAsState(if (expanded) 180f else 0f, label = "chevron")
    Column {
        UsageRow(
            leading = { AppIcon(app, icons) },
            title = app.appName,
            totalMs = app.totalMs,
            fraction = app.totalMs.toFloat() / top,
            trailing = if (expandable) {
                { Icon(Icons.Outlined.ExpandMore, contentDescription = if (expanded) "Hide sites" else "Show sites", Modifier.rotate(chevron)) }
            } else {
                null
            },
            onClick = if (expandable) ({ expanded = !expanded }) else null,
        )
        AnimatedVisibility(expanded) {
            Column(Modifier.padding(start = 56.dp)) {
                app.domains.forEach { domain ->
                    UsageRow(
                        leading = null,
                        title = domain.domain.ifEmpty { "Other" },
                        totalMs = domain.totalMs,
                        fraction = domain.totalMs.toFloat() / app.totalMs.coerceAtLeast(1),
                        dense = true,
                    )
                }
            }
        }
    }
}

@Composable
private fun SiteRow(site: DomainUsage, top: Long) {
    UsageRow(
        leading = { Monogram(Icons.Outlined.Language) },
        title = site.domain,
        totalMs = site.totalMs,
        fraction = site.totalMs.toFloat() / top,
    )
}

@Composable
private fun UsageRow(
    leading: (@Composable () -> Unit)?,
    title: String,
    totalMs: Long,
    fraction: Float,
    dense: Boolean = false,
    trailing: (@Composable () -> Unit)? = null,
    onClick: (() -> Unit)? = null,
) {
    Row(
        verticalAlignment = Alignment.CenterVertically,
        modifier = Modifier
            .fillMaxWidth()
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(horizontal = 20.dp, vertical = if (dense) 6.dp else 10.dp),
    ) {
        leading?.let {
            it()
            Spacer(Modifier.width(16.dp))
        }
        Column(Modifier.weight(1f)) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    title,
                    style = if (dense) MaterialTheme.typography.bodyMedium else MaterialTheme.typography.bodyLarge,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                Text(
                    formatDuration(totalMs),
                    style = if (dense) MaterialTheme.typography.bodyMedium else MaterialTheme.typography.bodyLarge,
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    modifier = Modifier.padding(start = 12.dp),
                )
            }
            Spacer(Modifier.height(6.dp))
            LinearProgressIndicator(
                progress = { fraction.coerceIn(0f, 1f) },
                strokeCap = StrokeCap.Round,
                gapSize = 0.dp,
                drawStopIndicator = {},
                trackColor = MaterialTheme.colorScheme.surfaceContainer,
                modifier = Modifier.fillMaxWidth().height(4.dp),
            )
        }
        Box(Modifier.width(36.dp), contentAlignment = Alignment.CenterEnd) { trailing?.invoke() }
    }
}

@Composable
private fun AppIcon(app: AppUsage, icons: AppIcons) {
    val bitmap by produceState(icons.peek(app), app.appKey) {
        value = withContext(Dispatchers.IO) { icons.load(app) }
    }
    val image = bitmap
    if (image != null) {
        Image(image, contentDescription = null, modifier = Modifier.size(40.dp))
    } else {
        Box(
            contentAlignment = Alignment.Center,
            modifier = Modifier.size(40.dp).clip(RoundedCornerShape(12.dp)).background(MaterialTheme.colorScheme.secondaryContainer),
        ) {
            Text(
                app.appName.take(1).uppercase(),
                style = MaterialTheme.typography.titleMedium,
                color = MaterialTheme.colorScheme.onSecondaryContainer,
            )
        }
    }
}

@Composable
private fun Monogram(icon: ImageVector) {
    Box(
        contentAlignment = Alignment.Center,
        modifier = Modifier.size(40.dp).clip(CircleShape).background(MaterialTheme.colorScheme.secondaryContainer),
    ) {
        Icon(icon, contentDescription = null, tint = MaterialTheme.colorScheme.onSecondaryContainer, modifier = Modifier.size(20.dp))
    }
}
