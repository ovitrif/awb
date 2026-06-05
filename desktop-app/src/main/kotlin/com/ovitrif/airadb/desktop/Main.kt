package com.ovitrif.airadb.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.foundation.verticalScroll
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.filled.Build
import androidx.compose.material.icons.filled.CheckCircle
import androidx.compose.material.icons.filled.Computer
import androidx.compose.material.icons.filled.Download
import androidx.compose.material.icons.filled.PlayArrow
import androidx.compose.material.icons.filled.Refresh
import androidx.compose.material.icons.filled.Settings
import androidx.compose.material.icons.filled.Stop
import androidx.compose.material.icons.filled.Terminal
import androidx.compose.material.icons.filled.Warning
import androidx.compose.material3.Button
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Card
import androidx.compose.material3.CardDefaults
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.PrimaryTabRow
import androidx.compose.material3.Surface
import androidx.compose.material3.Tab
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Tray
import androidx.compose.ui.window.Window
import androidx.compose.ui.window.WindowPosition
import androidx.compose.ui.window.WindowState
import androidx.compose.ui.window.application
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import java.awt.GraphicsEnvironment
import java.awt.MouseInfo
import java.awt.Point
import java.awt.Rectangle
import java.awt.event.WindowAdapter
import java.awt.event.WindowEvent
import java.nio.file.Files
import java.nio.file.Path
import java.time.LocalTime
import java.time.format.DateTimeFormatter
import javax.swing.SwingUtilities

private val AiradbPink = Color(0xFFF4DCE7)
private val Panel = Color(0xFFFFF8FB)
private val PanelMuted = Color(0xFFF2E6EC)
private val Ink = Color(0xFF283044)
private val Muted = Color(0xFF737589)
private val Blue = Color(0xFF3F7EEE)
private val AndroidGreen = Color(0xFF38C976)
private val PopoverSize = DpSize(560.dp, 720.dp)

private val json = Json {
    ignoreUnknownKeys = true
}

fun main() = application {
    var popoverVisible by remember { mutableStateOf(true) }
    val popoverState = remember {
        WindowState(
            size = PopoverSize,
            position = menuBarPopoverPosition(anchor = null),
        )
    }
    val controller = remember { AiradbController() }
    var selectedTab by remember { mutableStateOf(AppTab.Tools) }
    var status by remember { mutableStateOf<StatusSnapshot?>(null) }
    var statusLoading by remember { mutableStateOf(false) }
    var statusError by remember { mutableStateOf<String?>(null) }
    var activeCommand by remember { mutableStateOf<String?>(null) }
    var running by remember { mutableStateOf(false) }
    val logs = remember { mutableStateListOf<String>() }
    var settings by remember { mutableStateOf(AiradbSettings()) }
    val scope = rememberCoroutineScope()

    fun appendLog(line: String) {
        logs.add(line)
        while (logs.size > 600) {
            logs.removeAt(0)
        }
    }

    fun refreshStatus() {
        scope.launch {
            statusLoading = true
            statusError = null
            runCatching { controller.loadStatus() }
                .onSuccess { status = it }
                .onFailure { statusError = it.message ?: "Status failed" }
            statusLoading = false
        }
    }

    fun showPopover() {
        popoverState.position = menuBarPopoverPosition()
        popoverVisible = true
        refreshStatus()
    }

    fun launchAiradb(label: String, args: List<String>) {
        controller.stopActiveProcess()
        logs.clear()
        activeCommand = label
        running = true
        selectedTab = AppTab.Console
        popoverVisible = true
        appendLog(timestamped("Starting ${printableCommand(args)}"))

        scope.launch(Dispatchers.IO) {
            controller.stream(
                args = args,
                onLine = { line -> SwingUtilities.invokeLater { appendLog(line) } },
                onExit = { exitCode ->
                    SwingUtilities.invokeLater {
                        appendLog(timestamped("$label exited with code $exitCode"))
                        activeCommand = null
                        running = false
                        refreshStatus()
                    }
                },
            )
        }
    }

    fun runAiradb(label: String, commandArgs: List<String>) {
        launchAiradb(label, listOf(controller.airadbBinary) + commandArgs)
    }

    fun stopActive() {
        controller.stopActiveProcess()
        activeCommand = null
        running = false
        appendLog(timestamped("Stopped active command"))
        refreshStatus()
    }

    fun pairAndMirror() {
        runAiradb("Pair and mirror", settings.toCliArgs() + "--background")
    }

    fun stableMirror() {
        runAiradb("Stable mirror", settings.toCliArgs() + "--stable")
    }

    fun mirrorAndWait() {
        runAiradb("Mirror and wait", settings.toCliArgs() + "--foreground")
    }

    fun resetAdb() {
        runAiradb("Reset ADB", listOf("reset-adb"))
    }

    fun installShell() {
        runAiradb("Install shell", listOf("install-shell", "--force"))
    }

    LaunchedEffect(Unit) {
        refreshStatus()
    }

    DisposableEffect(Unit) {
        onDispose { controller.stopActiveProcess() }
    }

    Tray(
        icon = AndroidTrayPainter,
        tooltip = "airadb",
        onAction = ::showPopover,
        menu = {
            Item("Show airadb", onClick = ::showPopover)
            Separator()
            Item("Pair and mirror", onClick = ::pairAndMirror)
            Item("Stable mirror", onClick = ::stableMirror)
            Item("Refresh status", onClick = ::refreshStatus)
            if (running) {
                Item("Stop command", onClick = ::stopActive)
            }
            Separator()
            Item("Quit", onClick = {
                controller.stopActiveProcess()
                exitApplication()
            })
        },
    )

    if (popoverVisible) {
        Window(
            title = "airadb",
            icon = AndroidAppPainter,
            state = popoverState,
            undecorated = true,
            transparent = true,
            resizable = false,
            alwaysOnTop = true,
            onCloseRequest = { popoverVisible = false },
        ) {
            DisposableEffect(window) {
                val listener = object : WindowAdapter() {
                    override fun windowLostFocus(event: WindowEvent?) {
                        popoverVisible = false
                    }
                }

                window.addWindowFocusListener(listener)
                window.requestFocus()

                onDispose {
                    window.removeWindowFocusListener(listener)
                }
            }

            MaterialTheme(
                colorScheme = lightColorScheme(
                    primary = Blue,
                    secondary = AndroidGreen,
                    surface = Panel,
                    background = AiradbPink,
                    onPrimary = Color.White,
                    onSurface = Ink,
                    onBackground = Ink,
                ),
            ) {
                AiradbWindow(
                    selectedTab = selectedTab,
                    onSelectTab = { selectedTab = it },
                    status = status,
                    statusLoading = statusLoading,
                    statusError = statusError,
                    running = running,
                    activeCommand = activeCommand,
                    logs = logs,
                    settings = settings,
                    onSettingsChange = { settings = it },
                    onRefresh = ::refreshStatus,
                    onPairAndMirror = ::pairAndMirror,
                    onStableMirror = ::stableMirror,
                    onMirrorAndWait = ::mirrorAndWait,
                    onResetAdb = ::resetAdb,
                    onInstallShell = ::installShell,
                    onStop = ::stopActive,
                    airadbBinary = controller.airadbBinary,
                )
            }
        }
    }
}

@Composable
private fun AiradbWindow(
    selectedTab: AppTab,
    onSelectTab: (AppTab) -> Unit,
    status: StatusSnapshot?,
    statusLoading: Boolean,
    statusError: String?,
    running: Boolean,
    activeCommand: String?,
    logs: List<String>,
    settings: AiradbSettings,
    onSettingsChange: (AiradbSettings) -> Unit,
    onRefresh: () -> Unit,
    onPairAndMirror: () -> Unit,
    onStableMirror: () -> Unit,
    onMirrorAndWait: () -> Unit,
    onResetAdb: () -> Unit,
    onInstallShell: () -> Unit,
    onStop: () -> Unit,
    airadbBinary: String,
) {
    Surface(
        modifier = Modifier
            .fillMaxSize()
            .padding(8.dp)
            .shadow(22.dp, RoundedCornerShape(28.dp)),
        color = AiradbPink,
        shape = RoundedCornerShape(28.dp),
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(22.dp),
            verticalArrangement = Arrangement.spacedBy(16.dp),
        ) {
            Header(status = status, running = running, onRefresh = onRefresh)

            Surface(
                modifier = Modifier.fillMaxWidth(),
                color = Panel,
                shape = RoundedCornerShape(8.dp),
            ) {
                PrimaryTabRow(
                    selectedTabIndex = selectedTab.ordinal,
                    containerColor = Color.Transparent,
                    contentColor = Blue,
                ) {
                    AppTab.entries.forEach { tab ->
                        Tab(
                            selected = selectedTab == tab,
                            onClick = { onSelectTab(tab) },
                            text = { Text(tab.label, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                            icon = {
                                Icon(
                                    imageVector = if (tab == AppTab.Tools) Icons.Filled.Build else Icons.Filled.Terminal,
                                    contentDescription = null,
                                )
                            },
                        )
                    }
                }
            }

            when (selectedTab) {
                AppTab.Tools -> ToolsPanel(
                    status = status,
                    statusLoading = statusLoading,
                    statusError = statusError,
                    running = running,
                    settings = settings,
                    onSettingsChange = onSettingsChange,
                    onPairAndMirror = onPairAndMirror,
                    onStableMirror = onStableMirror,
                    onMirrorAndWait = onMirrorAndWait,
                    onResetAdb = onResetAdb,
                    onInstallShell = onInstallShell,
                    onStop = onStop,
                    airadbBinary = airadbBinary,
                )

                AppTab.Console -> ConsolePanel(
                    running = running,
                    activeCommand = activeCommand,
                    logs = logs,
                    onStop = onStop,
                )
            }
        }
    }
}

@Composable
private fun Header(
    status: StatusSnapshot?,
    running: Boolean,
    onRefresh: () -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                modifier = Modifier
                    .size(58.dp)
                    .clip(CircleShape)
                    .background(Color.White),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    painter = AndroidAppPainter,
                    contentDescription = null,
                    modifier = Modifier.size(42.dp),
                    tint = Color.Unspecified,
                )
            }
            Column {
                Text("airadb", style = MaterialTheme.typography.headlineMedium, fontWeight = FontWeight.Bold)
                Text(
                    "Android wireless debugging",
                    style = MaterialTheme.typography.bodyMedium,
                    color = Muted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }

        Row(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            StatusChip(text = if (running) "Running" else "Local", good = true)
            if (status?.airadbVersion != null) {
                StatusChip(text = "v${status.airadbVersion}", good = true)
            }
            IconButton(onClick = onRefresh) {
                Icon(Icons.Filled.Refresh, contentDescription = "Refresh")
            }
        }
    }
}

@Composable
@OptIn(ExperimentalLayoutApi::class)
private fun ToolsPanel(
    status: StatusSnapshot?,
    statusLoading: Boolean,
    statusError: String?,
    running: Boolean,
    settings: AiradbSettings,
    onSettingsChange: (AiradbSettings) -> Unit,
    onPairAndMirror: () -> Unit,
    onStableMirror: () -> Unit,
    onMirrorAndWait: () -> Unit,
    onResetAdb: () -> Unit,
    onInstallShell: () -> Unit,
    onStop: () -> Unit,
    airadbBinary: String,
) {
    Column(
        modifier = Modifier
            .fillMaxSize()
            .verticalScroll(rememberScrollState()),
        verticalArrangement = Arrangement.spacedBy(12.dp),
    ) {
        Surface(
            modifier = Modifier.fillMaxWidth(),
            color = Panel,
            shape = RoundedCornerShape(8.dp),
        ) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Installed", style = MaterialTheme.typography.titleMedium, color = Muted)
                    Text(
                        if (statusLoading) "Refreshing" else "${status?.readyDeviceCount() ?: 0} devices",
                        color = Blue,
                        fontWeight = FontWeight.SemiBold,
                    )
                }

                ToolStatusRow(
                    name = "ADB",
                    detail = status?.adb?.version ?: status?.adb?.path ?: "Not found",
                    available = status?.adb?.available == true,
                    icon = Icons.Filled.Computer,
                )
                ToolStatusRow(
                    name = "scrcpy",
                    detail = status?.scrcpy?.path ?: "Not found",
                    available = status?.scrcpy?.available == true,
                    icon = Icons.Filled.PlayArrow,
                )

                val devices = status?.devices.orEmpty()
                if (devices.isEmpty()) {
                    ToolStatusRow(
                        name = "Android phone",
                        detail = statusError ?: status?.adb?.devicesError ?: "No connected device",
                        available = false,
                        icon = Icons.Filled.Warning,
                    )
                } else {
                    devices.forEach { device ->
                        ToolStatusRow(
                            name = device.displayName,
                            detail = device.serial,
                            available = device.state == "device",
                            icon = Icons.Filled.CheckCircle,
                        )
                    }
                }
            }
        }

        Surface(
            modifier = Modifier.fillMaxWidth(),
            color = Panel,
            shape = RoundedCornerShape(8.dp),
        ) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(14.dp),
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Actions", style = MaterialTheme.typography.titleMedium, color = Muted)
                    if (running) {
                        TextButton(onClick = onStop) {
                            Icon(Icons.Filled.Stop, contentDescription = null, modifier = Modifier.size(18.dp))
                            Spacer(Modifier.width(6.dp))
                            Text("Stop")
                        }
                    }
                }

                FlowRow(
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                    verticalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    ActionButton("Pair and mirror", Icons.Filled.PlayArrow, running, onPairAndMirror)
                    ActionButton("Stable mirror", Icons.Filled.CheckCircle, running, onStableMirror)
                    ActionButton("Mirror and wait", Icons.Filled.Terminal, running, onMirrorAndWait)
                    ActionButton("Reset ADB", Icons.Filled.Refresh, running, onResetAdb)
                    ActionButton("Install shell", Icons.Filled.Download, running, onInstallShell)
                }
            }
        }

        Surface(
            modifier = Modifier.fillMaxWidth(),
            color = Panel,
            shape = RoundedCornerShape(8.dp),
        ) {
            Column(
                modifier = Modifier.padding(16.dp),
                verticalArrangement = Arrangement.spacedBy(12.dp),
            ) {
                Row(
                    horizontalArrangement = Arrangement.spacedBy(8.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(Icons.Filled.Settings, contentDescription = null, tint = Muted)
                    Text("scrcpy", style = MaterialTheme.typography.titleMedium, color = Muted)
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(10.dp),
                ) {
                    OutlinedTextField(
                        value = settings.windowTitle,
                        onValueChange = { onSettingsChange(settings.copy(windowTitle = it)) },
                        modifier = Modifier.weight(1f),
                        label = { Text("Window title") },
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = settings.windowWidth,
                        onValueChange = { onSettingsChange(settings.copy(windowWidth = it.onlyDigits())) },
                        modifier = Modifier.width(96.dp),
                        label = { Text("Width") },
                        singleLine = true,
                    )
                    OutlinedTextField(
                        value = settings.windowHeight,
                        onValueChange = { onSettingsChange(settings.copy(windowHeight = it.onlyDigits())) },
                        modifier = Modifier.width(104.dp),
                        label = { Text("Height") },
                        singleLine = true,
                    )
                }

                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    ToggleRow(
                        label = "Always on top",
                        checked = settings.alwaysOnTop,
                        onCheckedChange = { onSettingsChange(settings.copy(alwaysOnTop = it)) },
                    )
                    ToggleRow(
                        label = "Plain window",
                        checked = settings.plainWindow,
                        onCheckedChange = { onSettingsChange(settings.copy(plainWindow = it)) },
                    )
                }

                Text(
                    airadbBinary,
                    style = MaterialTheme.typography.labelSmall,
                    color = Muted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@Composable
private fun ConsolePanel(
    running: Boolean,
    activeCommand: String?,
    logs: List<String>,
    onStop: () -> Unit,
) {
    Surface(
        modifier = Modifier.fillMaxSize(),
        color = Panel,
        shape = RoundedCornerShape(8.dp),
    ) {
        Column(
            modifier = Modifier.padding(16.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column {
                    Text("Console", style = MaterialTheme.typography.titleMedium, color = Muted)
                    Text(
                        activeCommand ?: "Idle",
                        style = MaterialTheme.typography.bodyMedium,
                        color = if (running) Blue else Muted,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                if (running) {
                    Button(
                        onClick = onStop,
                        colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFD94F5C)),
                    ) {
                        Icon(Icons.Filled.Stop, contentDescription = null, modifier = Modifier.size(18.dp))
                        Spacer(Modifier.width(6.dp))
                        Text("Stop")
                    }
                }
            }

            Surface(
                modifier = Modifier
                    .fillMaxWidth()
                    .weight(1f)
                    .border(1.dp, Color(0xFFE4D5DD), RoundedCornerShape(8.dp)),
                color = Color(0xFF1F2430),
                shape = RoundedCornerShape(8.dp),
            ) {
                SelectionContainer {
                    LazyColumn(
                        modifier = Modifier
                            .fillMaxSize()
                            .padding(12.dp),
                        verticalArrangement = Arrangement.spacedBy(2.dp),
                    ) {
                        if (logs.isEmpty()) {
                            item {
                                Text(
                                    "No process output yet.",
                                    color = Color(0xFFADB4C7),
                                    fontFamily = FontFamily.Monospace,
                                )
                            }
                        } else {
                            items(logs) { line ->
                                Text(
                                    line,
                                    color = Color(0xFFE9EDF7),
                                    fontFamily = FontFamily.Monospace,
                                    style = MaterialTheme.typography.bodySmall,
                                )
                            }
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun ToolStatusRow(
    name: String,
    detail: String,
    available: Boolean,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
) {
    Card(
        modifier = Modifier.fillMaxWidth(),
        shape = RoundedCornerShape(8.dp),
        colors = CardDefaults.cardColors(containerColor = if (available) Color.White else PanelMuted),
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(12.dp),
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                modifier = Modifier
                    .size(42.dp)
                    .clip(CircleShape)
                    .background(if (available) Color(0xFFE2F8EB) else Color(0xFFFFE8EC)),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    icon,
                    contentDescription = null,
                    tint = if (available) AndroidGreen else Color(0xFFD94F5C),
                )
            }
            Column(modifier = Modifier.weight(1f)) {
                Text(name, fontWeight = FontWeight.Bold, color = Ink, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(detail, color = Muted, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            StatusChip(if (available) "Ready" else "Check", available)
        }
    }
}

@Composable
private fun ActionButton(
    label: String,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    running: Boolean,
    onClick: () -> Unit,
) {
    Button(
        onClick = onClick,
        enabled = !running,
        shape = RoundedCornerShape(8.dp),
        colors = ButtonDefaults.buttonColors(containerColor = Blue),
    ) {
        Icon(icon, contentDescription = null, modifier = Modifier.size(18.dp))
        Spacer(Modifier.width(8.dp))
        Text(label, maxLines = 1)
    }
}

@Composable
private fun ToggleRow(
    label: String,
    checked: Boolean,
    onCheckedChange: (Boolean) -> Unit,
) {
    Row(
        horizontalArrangement = Arrangement.spacedBy(6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Checkbox(checked = checked, onCheckedChange = onCheckedChange)
        Text(label, color = Ink, maxLines = 1)
    }
}

@Composable
private fun StatusChip(text: String, good: Boolean) {
    Surface(
        color = if (good) Color(0xFFE4F7ED) else Color(0xFFFFE8EC),
        shape = RoundedCornerShape(8.dp),
    ) {
        Text(
            text = text,
            modifier = Modifier.padding(horizontal = 10.dp, vertical = 5.dp),
            color = if (good) Color(0xFF157A43) else Color(0xFFA33A45),
            style = MaterialTheme.typography.labelMedium,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

private enum class AppTab(val label: String) {
    Tools("Tools"),
    Console("Console"),
}

private data class AiradbSettings(
    val alwaysOnTop: Boolean = true,
    val plainWindow: Boolean = false,
    val windowTitle: String = "Pixel 10 Pro",
    val windowWidth: String = "480",
    val windowHeight: String = "1071",
) {
    fun toCliArgs(): List<String> = buildList {
        if (alwaysOnTop) {
            add("--always-on-top")
        }
        if (plainWindow) {
            add("--plain-window")
        }
        if (windowTitle.isNotBlank()) {
            add("--window-title")
            add(windowTitle)
        }
        if (windowWidth.isNotBlank()) {
            add("--window-width")
            add(windowWidth)
        }
        if (windowHeight.isNotBlank()) {
            add("--window-height")
            add(windowHeight)
        }
    }
}

private class AiradbController {
    val airadbBinary: String = resolveAiradbBinary()
    private var activeProcess: Process? = null

    suspend fun loadStatus(): StatusSnapshot {
        val result = runCapture(listOf(airadbBinary, "status", "--json"), timeoutMillis = 25_000)
        if (result.exitCode != 0) {
            error(result.stderr.ifBlank { result.stdout }.ifBlank { "airadb status failed" })
        }

        return json.decodeFromString(StatusSnapshot.serializer(), result.stdout)
    }

    suspend fun stream(
        args: List<String>,
        onLine: (String) -> Unit,
        onExit: (Int) -> Unit,
    ) {
        val process = withContext(Dispatchers.IO) {
            ProcessBuilder(args)
                .redirectErrorStream(true)
                .start()
        }
        activeProcess = process

        try {
            withContext(Dispatchers.IO) {
                process.inputStream.bufferedReader().useLines { lines ->
                    lines.forEach(onLine)
                }
            }
            val exitCode = withContext(Dispatchers.IO) { process.waitFor() }
            onExit(exitCode)
        } catch (error: Throwable) {
            onLine(timestamped(error.message ?: "Command failed"))
            onExit(1)
        } finally {
            if (activeProcess == process) {
                activeProcess = null
            }
        }
    }

    fun stopActiveProcess() {
        activeProcess?.destroy()
        activeProcess = null
    }
}

private data class CommandResult(
    val exitCode: Int,
    val stdout: String,
    val stderr: String,
)

private suspend fun runCapture(command: List<String>, timeoutMillis: Long): CommandResult = coroutineScope {
    val process = withContext(Dispatchers.IO) {
        ProcessBuilder(command)
            .redirectErrorStream(false)
            .start()
    }
    val stdout = async(Dispatchers.IO) { process.inputStream.bufferedReader().readText() }
    val stderr = async(Dispatchers.IO) { process.errorStream.bufferedReader().readText() }
    val exited = withTimeoutOrNull(timeoutMillis) {
        withContext(Dispatchers.IO) { process.waitFor() }
        true
    } ?: false

    if (!exited) {
        process.destroyForcibly()
        error("${printableCommand(command)} timed out")
    }

    CommandResult(
        exitCode = process.exitValue(),
        stdout = stdout.await(),
        stderr = stderr.await(),
    )
}

private fun resolveAiradbBinary(): String {
    System.getenv("AIRADB_BIN")
        ?.takeIf { it.isNotBlank() }
        ?.let { return it }

    val userDir = Path.of(System.getProperty("user.dir")).toAbsolutePath()
    val candidates = listOfNotNull(
        userDir.resolve("target/release/airadb"),
        userDir.parent?.resolve("target/release/airadb"),
        Path.of(System.getProperty("user.home"), ".local/bin/airadb"),
    )

    return candidates
        .firstOrNull { Files.isExecutable(it) }
        ?.toString()
        ?: "airadb"
}

@Serializable
private data class StatusSnapshot(
    @SerialName("airadb_version")
    val airadbVersion: String? = null,
    val adb: AdbStatus = AdbStatus(),
    val scrcpy: ToolStatus = ToolStatus(),
    val devices: List<DeviceStatus> = emptyList(),
) {
    fun readyDeviceCount(): Int = devices.count { it.state == "device" }
}

@Serializable
private data class AdbStatus(
    val path: String? = null,
    val available: Boolean = false,
    val version: String? = null,
    @SerialName("mdns_available")
    val mdnsAvailable: Boolean? = null,
    val error: String? = null,
    @SerialName("mdns_error")
    val mdnsError: String? = null,
    @SerialName("devices_error")
    val devicesError: String? = null,
)

@Serializable
private data class ToolStatus(
    val path: String? = null,
    val available: Boolean = false,
    val error: String? = null,
)

@Serializable
private data class DeviceStatus(
    val serial: String = "",
    val state: String = "",
    @SerialName("display_name")
    val displayName: String = serial,
    val product: String? = null,
    val model: String? = null,
    val device: String? = null,
    @SerialName("transport_id")
    val transportId: String? = null,
)

private object AndroidTrayPainter : Painter() {
    override val intrinsicSize = Size(64f, 64f)

    override fun DrawScope.onDraw() {
        drawAndroid(size, tray = true)
    }
}

private object AndroidAppPainter : Painter() {
    override val intrinsicSize = Size(256f, 256f)

    override fun DrawScope.onDraw() {
        drawAndroid(size, tray = false)
    }
}

private fun DrawScope.drawAndroid(size: Size, tray: Boolean) {
    val main = if (tray) Color.White else AndroidGreen
    val eye = if (tray) Color(0xFF222738) else Color.White
    val antenna = if (tray) Color.White else Color(0xFF1D8E55)
    val stroke = size.minDimension * 0.055f
    val headTop = size.height * 0.24f
    val headLeft = size.width * 0.20f
    val headSize = Size(size.width * 0.60f, size.height * 0.42f)
    val bodyTop = size.height * 0.57f

    drawLine(
        color = antenna,
        start = Offset(size.width * 0.34f, size.height * 0.25f),
        end = Offset(size.width * 0.22f, size.height * 0.10f),
        strokeWidth = stroke,
    )
    drawLine(
        color = antenna,
        start = Offset(size.width * 0.66f, size.height * 0.25f),
        end = Offset(size.width * 0.78f, size.height * 0.10f),
        strokeWidth = stroke,
    )
    drawRoundRect(
        color = main,
        topLeft = Offset(headLeft, headTop),
        size = headSize,
        cornerRadius = CornerRadius(size.width * 0.18f, size.width * 0.18f),
    )
    drawRoundRect(
        color = main,
        topLeft = Offset(size.width * 0.28f, bodyTop),
        size = Size(size.width * 0.44f, size.height * 0.28f),
        cornerRadius = CornerRadius(size.width * 0.07f, size.width * 0.07f),
    )
    drawRoundRect(
        color = main,
        topLeft = Offset(size.width * 0.12f, bodyTop),
        size = Size(size.width * 0.12f, size.height * 0.24f),
        cornerRadius = CornerRadius(size.width * 0.06f, size.width * 0.06f),
    )
    drawRoundRect(
        color = main,
        topLeft = Offset(size.width * 0.76f, bodyTop),
        size = Size(size.width * 0.12f, size.height * 0.24f),
        cornerRadius = CornerRadius(size.width * 0.06f, size.width * 0.06f),
    )
    drawCircle(eye, radius = size.width * 0.045f, center = Offset(size.width * 0.39f, size.height * 0.42f))
    drawCircle(eye, radius = size.width * 0.045f, center = Offset(size.width * 0.61f, size.height * 0.42f))
}

private fun String.onlyDigits(): String = filter { it.isDigit() }.take(5)

private fun menuBarPopoverPosition(anchor: Point? = currentPointerLocation()): WindowPosition {
    val screen = screenBoundsFor(anchor)
    val width = PopoverSize.width.value.toInt()
    val height = PopoverSize.height.value.toInt()
    val margin = 12
    val menuBarGap = 10
    val minX = screen.x + margin
    val maxX = (screen.x + screen.width - width - margin).coerceAtLeast(minX)
    val minY = screen.y + margin
    val maxY = (screen.y + screen.height - height - margin).coerceAtLeast(minY)
    val fallbackX = maxX
    val anchorX = anchor?.x?.minus(width / 2) ?: fallbackX
    val x = anchorX.coerceIn(minX, maxX)
    val y = if (anchor != null && anchor.y <= screen.y + 96) {
        anchor.y + menuBarGap
    } else {
        screen.y + 32
    }.coerceIn(minY, maxY)

    return WindowPosition.Absolute(x.dp, y.dp)
}

private fun currentPointerLocation(): Point? =
    runCatching { MouseInfo.getPointerInfo()?.location }.getOrNull()

private fun screenBoundsFor(anchor: Point?): Rectangle {
    val environment = GraphicsEnvironment.getLocalGraphicsEnvironment()
    val devices = environment.screenDevices

    if (anchor != null) {
        devices
            .firstOrNull { device -> device.defaultConfiguration.bounds.contains(anchor) }
            ?.let { return it.defaultConfiguration.bounds }
    }

    return environment.defaultScreenDevice.defaultConfiguration.bounds
}

private fun printableCommand(command: List<String>): String =
    command.joinToString(" ") { part ->
        if (part.any(Char::isWhitespace)) {
            "'${part.replace("'", "'\\''")}'"
        } else {
            part
        }
    }

private fun timestamped(message: String): String {
    val time = LocalTime.now().format(DateTimeFormatter.ofPattern("HH:mm:ss"))
    return "[$time] $message"
}
