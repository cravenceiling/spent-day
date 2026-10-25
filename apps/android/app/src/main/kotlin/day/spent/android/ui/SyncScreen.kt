package day.spent.android.ui

import android.content.ClipData
import android.content.ClipDescription
import android.os.Build
import android.os.PersistableBundle
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.outlined.ArrowBack
import androidx.compose.material.icons.outlined.CheckCircle
import androidx.compose.material.icons.outlined.ContentCopy
import androidx.compose.material.icons.outlined.ErrorOutline
import androidx.compose.material.icons.outlined.Visibility
import androidx.compose.material.icons.outlined.VisibilityOff
import androidx.compose.material3.Button
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.FilledTonalButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.ClipEntry
import androidx.compose.ui.platform.LocalAutofillManager
import androidx.compose.ui.platform.LocalClipboard
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.unit.dp
import day.spent.android.SyncHistory
import day.spent.android.SyncState
import kotlinx.coroutines.launch

class SyncActions(
    val back: () -> Unit,
    val create: (serverUrl: String) -> Unit,
    val join: (serverUrl: String, code: String) -> Unit,
    val syncNow: () -> Unit,
    val turnOff: () -> Unit,
)

@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun SyncScreen(state: SyncState, actions: SyncActions) {
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text("Sync") },
                navigationIcon = {
                    IconButton(onClick = actions.back) { Icon(Icons.AutoMirrored.Outlined.ArrowBack, contentDescription = "Back") }
                },
            )
        },
    ) { padding ->
        Column(
            Modifier.padding(padding).verticalScroll(rememberScrollState()).padding(horizontal = 20.dp, vertical = 8.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Text(
                "Sync shows every device's screen time on every device. It sends app names, domains, times and this phone's name, encrypted with the sync code before they leave this phone. The server can't read them.",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            val config = state.config
            if (config == null) SetUp(state, actions) else Configured(config.serverUrl, config.syncCode, state, actions)
            state.error?.let { Text(it, color = MaterialTheme.colorScheme.error, style = MaterialTheme.typography.bodyMedium) }
        }
    }
}

@Composable
private fun SetUp(state: SyncState, actions: SyncActions) {
    var serverUrl by rememberSaveable { mutableStateOf("https://") }
    var code by rememberSaveable { mutableStateOf("") }
    val urlReady = serverUrl.trim().let { it.startsWith("http") && it.substringAfter("://").isNotBlank() }
    OutlinedTextField(
        value = serverUrl,
        onValueChange = { serverUrl = it },
        label = { Text("Server address") },
        singleLine = true,
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri),
        modifier = Modifier.fillMaxWidth(),
    )
    Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(12.dp)) {
            Text("First device", style = MaterialTheme.typography.titleMedium)
            Text("Create a sync code here, then enter it on your other devices.", style = MaterialTheme.typography.bodyMedium)
            Button(onClick = { actions.create(serverUrl) }, enabled = urlReady && !state.busy) { Text("Create sync code") }
            HorizontalDivider(Modifier.padding(vertical = 4.dp))
            Text("Already syncing elsewhere", style = MaterialTheme.typography.titleMedium)
            SyncCodeField(code, onValueChange = { code = it })
            FilledTonalButton(onClick = { actions.join(serverUrl, code) }, enabled = urlReady && code.isNotBlank() && !state.busy) {
                Text("Join")
            }
        }
    }
    if (state.busy) CircularProgressIndicator()
}

/** Masked, so accessibility services read bullets, until the user reveals it. */
@Composable
private fun SyncCodeField(code: String, onValueChange: (String) -> Unit) {
    var shown by rememberSaveable { mutableStateOf(false) }
    // The code is a shared secret, not an account password, so a password manager must not offer to save it.
    val autofill = LocalAutofillManager.current
    DisposableEffect(autofill) { onDispose { autofill?.cancel() } }
    OutlinedTextField(
        value = code,
        onValueChange = onValueChange,
        label = { Text("Sync code") },
        maxLines = 3,
        visualTransformation = if (shown) VisualTransformation.None else PasswordVisualTransformation(),
        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false),
        trailingIcon = {
            IconButton(onClick = { shown = !shown }) {
                Icon(
                    if (shown) Icons.Outlined.VisibilityOff else Icons.Outlined.Visibility,
                    contentDescription = if (shown) "Hide sync code" else "Show sync code",
                )
            }
        },
        textStyle = MaterialTheme.typography.bodyLarge.copy(fontFamily = FontFamily.Monospace),
        modifier = Modifier.fillMaxWidth(),
    )
}

@Composable
private fun Configured(serverUrl: String, code: String, state: SyncState, actions: SyncActions) {
    val clipboard = LocalClipboard.current
    val scope = rememberCoroutineScope()
    Card(colors = CardDefaults.cardColors(containerColor = MaterialTheme.colorScheme.surfaceContainer)) {
        Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
            Text("Server", style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(serverUrl, style = MaterialTheme.typography.bodyLarge)
            Spacer(Modifier.height(12.dp))
            Text("Sync code", style = MaterialTheme.typography.labelLarge, color = MaterialTheme.colorScheme.onSurfaceVariant)
            var shown by rememberSaveable { mutableStateOf(false) }
            Row(verticalAlignment = Alignment.CenterVertically) {
                Text(
                    if (shown) code else code.replace(Regex("[^-]"), "•"),
                    style = MaterialTheme.typography.titleMedium.copy(fontFamily = FontFamily.Monospace),
                    modifier = Modifier.weight(1f),
                )
                IconButton(onClick = { scope.launch { clipboard.setClipEntry(ClipEntry(sensitiveClip(code))) } }) {
                    Icon(Icons.Outlined.ContentCopy, contentDescription = "Copy sync code")
                }
            }
            TextButton(onClick = { shown = !shown }) { Text(if (shown) "Hide" else "Show") }
            Text(
                "Anyone with this code can read your synced screen time. Enter it only on your own devices.",
                style = MaterialTheme.typography.bodySmall,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
    }
    Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
        Button(onClick = actions.syncNow, enabled = !state.busy) { Text("Sync now") }
        if (state.busy) CircularProgressIndicator(Modifier.size(24.dp), strokeWidth = 2.dp)
    }
    SyncOutcome(state.history)
    TextButton(onClick = actions.turnOff, enabled = !state.busy) { Text("Turn off sync on this device") }
}

@Composable
private fun SyncOutcome(history: SyncHistory) {
    history.lastGood?.let { Outcome(ok = true, it.message, "Last sync ${formatTimestamp(it.atMs)}") }
    history.failure?.let { Outcome(ok = false, it.message, "Failed ${formatTimestamp(it.atMs)}") }
}

@Composable
private fun Outcome(ok: Boolean, message: String, time: String) {
    Row(verticalAlignment = Alignment.CenterVertically) {
        Icon(
            if (ok) Icons.Outlined.CheckCircle else Icons.Outlined.ErrorOutline,
            contentDescription = if (ok) "Succeeded" else "Failed",
            tint = if (ok) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.error,
            modifier = Modifier.size(20.dp),
        )
        Column(Modifier.padding(start = 12.dp)) {
            Text(message, style = MaterialTheme.typography.bodyMedium)
            Text(time, style = MaterialTheme.typography.bodySmall, color = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

/** Marked sensitive so the clipboard preview and keyboard suggestions don't show the code. */
private fun sensitiveClip(code: String) = ClipData.newPlainText("Spent Day sync code", code).apply {
    val key = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
        ClipDescription.EXTRA_IS_SENSITIVE
    } else {
        "android.content.extra.IS_SENSITIVE"
    }
    description.extras = PersistableBundle().apply { putBoolean(key, true) }
}
