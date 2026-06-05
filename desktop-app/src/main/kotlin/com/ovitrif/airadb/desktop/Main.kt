package com.ovitrif.airadb.desktop

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.defaultMinSize
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
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
import androidx.compose.material3.Checkbox
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.Typography
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
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
import java.awt.BasicStroke
import java.awt.GraphicsEnvironment
import java.awt.MouseInfo
import java.awt.Point
import java.awt.Rectangle
import java.awt.RenderingHints
import java.awt.SystemTray
import java.awt.TrayIcon
import java.awt.Color as AwtColor
import java.awt.event.MouseAdapter
import java.awt.event.MouseEvent
import java.awt.event.WindowAdapter
import java.awt.event.WindowEvent
import java.awt.image.BufferedImage
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
private val AiradbTypography = Typography().withoutLetterSpacing()
private val PopoverSize = DpSize(430.dp, 360.dp)
private val AboutWindowSize = DpSize(260.dp, 190.dp)
private const val TrayClickFocusLossGraceNanos = 300_000_000L
private const val TrayMenuWidthPx = 150
private const val TrayMenuItemHeightPx = 24
private const val TrayMenuSeparatorHeightPx = 5
private const val TrayMenuVerticalPaddingPx = 12
private const val AboutWindowWidthPx = 260
private const val AboutWindowHeightPx = 190

private val json = Json {
    ignoreUnknownKeys = true
}

private fun Typography.withoutLetterSpacing(): Typography =
    copy(
        displayLarge = displayLarge.noTracking(),
        displayMedium = displayMedium.noTracking(),
        displaySmall = displaySmall.noTracking(),
        headlineLarge = headlineLarge.noTracking(),
        headlineMedium = headlineMedium.noTracking(),
        headlineSmall = headlineSmall.noTracking(),
        titleLarge = titleLarge.noTracking(),
        titleMedium = titleMedium.noTracking(),
        titleSmall = titleSmall.noTracking(),
        bodyLarge = bodyLarge.noTracking(),
        bodyMedium = bodyMedium.noTracking(),
        bodySmall = bodySmall.noTracking(),
        labelLarge = labelLarge.noTracking(),
        labelMedium = labelMedium.noTracking(),
        labelSmall = labelSmall.noTracking(),
    )

private fun TextStyle.noTracking(): TextStyle = copy(letterSpacing = 0.sp)

fun main() = application {
    var popoverVisible by remember { mutableStateOf(false) }
    var lastFocusDismissAt by remember { mutableStateOf(0L) }
    val popoverState = remember {
        WindowState(
            size = PopoverSize,
            position = menuBarPopoverPosition(anchor = null),
        )
    }
    val aboutState = remember {
        WindowState(
            size = AboutWindowSize,
            position = centeredWindowPosition(AboutWindowWidthPx, AboutWindowHeightPx),
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
    var optionsExpanded by remember { mutableStateOf(false) }
    var trayMenuVisible by remember { mutableStateOf(false) }
    var trayMenuPosition by remember { mutableStateOf<WindowPosition>(WindowPosition.Absolute(0.dp, 0.dp)) }
    var aboutVisible by remember { mutableStateOf(false) }
    var aboutVersion by remember { mutableStateOf<String?>(null) }
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
        lastFocusDismissAt = 0L
        trayMenuVisible = false
        popoverState.position = menuBarPopoverPosition()
        popoverVisible = true
        refreshStatus()
    }

    fun togglePopover() {
        trayMenuVisible = false
        if (popoverVisible) {
            popoverVisible = false
        } else if (System.nanoTime() - lastFocusDismissAt < TrayClickFocusLossGraceNanos) {
            lastFocusDismissAt = 0L
        } else {
            showPopover()
        }
    }

    fun showTrayMenu(anchor: Point) {
        trayMenuPosition = trayMenuWindowPosition(anchor, running)
        trayMenuVisible = true
    }

    fun launchAiradb(label: String, args: List<String>) {
        controller.stopActiveProcess()
        logs.clear()
        activeCommand = label
        running = true
        selectedTab = AppTab.Console
        popoverState.position = menuBarPopoverPosition()
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

    fun showAbout() {
        trayMenuVisible = false
        aboutVersion = status?.airadbVersion ?: aboutVersion
        aboutState.position = centeredWindowPosition(AboutWindowWidthPx, AboutWindowHeightPx)
        aboutVisible = true
        if (aboutVersion == null) {
            scope.launch {
                runCatching { controller.loadVersion() }
                    .onSuccess { aboutVersion = it }
            }
        }
    }

    LaunchedEffect(Unit) {
        refreshStatus()
    }

    DisposableEffect(Unit) {
        onDispose { controller.stopActiveProcess() }
    }

    AiradbTray(
        onTogglePopover = ::togglePopover,
        onShowMenu = ::showTrayMenu,
    )

    if (trayMenuVisible) {
        TrayContextMenuWindow(
            position = trayMenuPosition,
            popoverVisible = popoverVisible,
            running = running,
            onDismiss = { trayMenuVisible = false },
            onTogglePopover = ::togglePopover,
            onPairAndMirror = {
                trayMenuVisible = false
                pairAndMirror()
            },
            onStableMirror = {
                trayMenuVisible = false
                stableMirror()
            },
            onRefresh = {
                trayMenuVisible = false
                refreshStatus()
            },
            onStop = {
                trayMenuVisible = false
                stopActive()
            },
            onQuit = {
                controller.stopActiveProcess()
                exitApplication()
            },
            onAbout = ::showAbout,
        )
    }

    if (aboutVisible) {
        Window(
            title = "About airadb",
            icon = AndroidAppPainter,
            state = aboutState,
            resizable = false,
            alwaysOnTop = true,
            onCloseRequest = { aboutVisible = false },
        ) {
            AboutWindow(version = aboutVersion ?: status?.airadbVersion)
        }
    }

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
                window.background = AwtColor(0, 0, 0, 0)
                val listener = object : WindowAdapter() {
                    override fun windowLostFocus(event: WindowEvent?) {
                        lastFocusDismissAt = System.nanoTime()
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
                typography = AiradbTypography,
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
                    optionsExpanded = optionsExpanded,
                    onSettingsChange = { settings = it },
                    onOptionsExpandedChange = { optionsExpanded = it },
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
private fun AiradbTray(
    onTogglePopover: () -> Unit,
    onShowMenu: (Point) -> Unit,
) {
    val currentToggle by rememberUpdatedState(onTogglePopover)
    val currentShowMenu by rememberUpdatedState(onShowMenu)
    val trayIcon = remember {
        TrayIcon(androidTrayImage(), "airadb").apply {
            isImageAutoSize = true
        }
    }

    DisposableEffect(Unit) {
        if (!SystemTray.isSupported()) {
            return@DisposableEffect onDispose {}
        }

        val listener = object : MouseAdapter() {
            override fun mouseClicked(event: MouseEvent) {
                val shouldShowMenu =
                    event.button == MouseEvent.BUTTON3 ||
                        event.isPopupTrigger ||
                        (event.button == MouseEvent.BUTTON1 && event.isControlDown)

                if (shouldShowMenu) {
                    SwingUtilities.invokeLater { currentShowMenu(Point(event.xOnScreen, event.yOnScreen)) }
                } else if (event.button == MouseEvent.BUTTON1) {
                    SwingUtilities.invokeLater { currentToggle() }
                }
            }
        }

        trayIcon.addMouseListener(listener)
        SystemTray.getSystemTray().add(trayIcon)

        onDispose {
            trayIcon.removeMouseListener(listener)
            SystemTray.getSystemTray().remove(trayIcon)
        }
    }
}

@Composable
private fun TrayContextMenuWindow(
    position: WindowPosition,
    popoverVisible: Boolean,
    running: Boolean,
    onDismiss: () -> Unit,
    onTogglePopover: () -> Unit,
    onPairAndMirror: () -> Unit,
    onStableMirror: () -> Unit,
    onRefresh: () -> Unit,
    onStop: () -> Unit,
    onQuit: () -> Unit,
    onAbout: () -> Unit,
) {
    Window(
        title = "airadb menu",
        state = WindowState(size = DpSize(TrayMenuWidthPx.dp, trayContextMenuHeight(running).dp), position = position),
        undecorated = true,
        transparent = true,
        resizable = false,
        alwaysOnTop = true,
        onCloseRequest = onDismiss,
    ) {
        DisposableEffect(window) {
            window.background = AwtColor(0, 0, 0, 0)
            val listener = object : WindowAdapter() {
                override fun windowLostFocus(event: WindowEvent?) {
                    onDismiss()
                }
            }

            window.addWindowFocusListener(listener)
            window.requestFocus()

            onDispose {
                window.removeWindowFocusListener(listener)
            }
        }

        Surface(
            modifier = Modifier.fillMaxSize(),
            color = Color.White,
            shape = RoundedCornerShape(8.dp),
        ) {
            Column(modifier = Modifier.padding(6.dp)) {
                ContextMenuItem(if (popoverVisible) "Hide airadb" else "Show airadb", onTogglePopover)
                ContextMenuSeparator()
                ContextMenuItem("Pair and mirror", onPairAndMirror)
                ContextMenuItem("Stable mirror", onStableMirror)
                ContextMenuItem("Refresh status", onRefresh)
                if (running) {
                    ContextMenuItem("Stop command", onStop)
                }
                ContextMenuSeparator()
                ContextMenuItem("Quit", onQuit)
                ContextMenuItem("About", onAbout)
            }
        }
    }
}

@Composable
private fun AboutWindow(version: String?) {
    MaterialTheme(
        colorScheme = lightColorScheme(
            primary = Blue,
            secondary = AndroidGreen,
            surface = Panel,
            background = Color.White,
            onSurface = Ink,
            onBackground = Ink,
        ),
        typography = AiradbTypography,
    ) {
        Surface(
            modifier = Modifier.fillMaxSize(),
            color = Color.White,
        ) {
            Column(
                modifier = Modifier
                    .fillMaxSize()
                    .padding(20.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
                verticalArrangement = Arrangement.Center,
            ) {
                Icon(
                    painter = AndroidAppPainter,
                    contentDescription = null,
                    modifier = Modifier.size(64.dp),
                    tint = Color.Unspecified,
                )
                Spacer(Modifier.height(12.dp))
                Text("airadb", color = Ink, fontSize = 20.sp, fontWeight = FontWeight.Bold)
                Spacer(Modifier.height(4.dp))
                Text(
                    text = version?.let { "v$it" } ?: "Version unavailable",
                    color = Muted,
                    fontSize = 13.sp,
                )
            }
        }
    }
}

@Composable
private fun ContextMenuItem(label: String, onClick: () -> Unit) {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .height(24.dp)
            .clip(RoundedCornerShape(5.dp))
            .clickable(onClick = onClick)
            .padding(horizontal = 8.dp),
        contentAlignment = Alignment.CenterStart,
    ) {
        Text(
            label,
            color = Ink,
            fontSize = 12.sp,
            letterSpacing = 0.sp,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun ContextMenuSeparator() {
    Box(
        modifier = Modifier
            .fillMaxWidth()
            .height(5.dp),
        contentAlignment = Alignment.Center,
    ) {
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .height(1.dp)
                .background(Color(0xFFEAEAF0)),
        )
    }
}

private fun androidTrayImage(size: Int = 22): BufferedImage {
    val image = BufferedImage(size, size, BufferedImage.TYPE_INT_ARGB)
    val graphics = image.createGraphics()
    graphics.setRenderingHint(RenderingHints.KEY_ANTIALIASING, RenderingHints.VALUE_ANTIALIAS_ON)
    graphics.color = AwtColor.WHITE
    graphics.stroke = BasicStroke((size * 0.08f).coerceAtLeast(1f), BasicStroke.CAP_ROUND, BasicStroke.JOIN_ROUND)

    graphics.drawLine((size * 0.34f).toInt(), (size * 0.25f).toInt(), (size * 0.24f).toInt(), (size * 0.10f).toInt())
    graphics.drawLine((size * 0.66f).toInt(), (size * 0.25f).toInt(), (size * 0.76f).toInt(), (size * 0.10f).toInt())
    graphics.fillRoundRect((size * 0.20f).toInt(), (size * 0.25f).toInt(), (size * 0.60f).toInt(), (size * 0.36f).toInt(), 8, 8)
    graphics.fillRoundRect((size * 0.28f).toInt(), (size * 0.56f).toInt(), (size * 0.44f).toInt(), (size * 0.26f).toInt(), 5, 5)
    graphics.fillRoundRect((size * 0.12f).toInt(), (size * 0.58f).toInt(), (size * 0.12f).toInt(), (size * 0.22f).toInt(), 4, 4)
    graphics.fillRoundRect((size * 0.76f).toInt(), (size * 0.58f).toInt(), (size * 0.12f).toInt(), (size * 0.22f).toInt(), 4, 4)

    graphics.color = AwtColor(42, 48, 68)
    graphics.fillOval((size * 0.36f).toInt(), (size * 0.39f).toInt(), 2, 2)
    graphics.fillOval((size * 0.58f).toInt(), (size * 0.39f).toInt(), 2, 2)
    graphics.dispose()

    return image
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
    optionsExpanded: Boolean,
    onSettingsChange: (AiradbSettings) -> Unit,
    onOptionsExpandedChange: (Boolean) -> Unit,
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
        modifier = Modifier.fillMaxSize(),
        color = AiradbPink,
        shape = RoundedCornerShape(18.dp),
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(10.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Header(running = running, onRefresh = onRefresh)

            CompactTabBar(selectedTab = selectedTab, onSelectTab = onSelectTab)

            when (selectedTab) {
                AppTab.Tools -> ToolsPanel(
                    status = status,
                    statusLoading = statusLoading,
                    statusError = statusError,
                    running = running,
                    settings = settings,
                    optionsExpanded = optionsExpanded,
                    onSettingsChange = onSettingsChange,
                    onOptionsExpandedChange = onOptionsExpandedChange,
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
private fun CompactTabBar(
    selectedTab: AppTab,
    onSelectTab: (AppTab) -> Unit,
) {
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .height(30.dp),
        color = Panel,
        shape = RoundedCornerShape(8.dp),
    ) {
        Row(
            modifier = Modifier.padding(2.dp),
            horizontalArrangement = Arrangement.spacedBy(2.dp),
        ) {
            AppTab.entries.forEach { tab ->
                val selected = selectedTab == tab
                Box(
                    modifier = Modifier
                        .weight(1f)
                        .fillMaxSize()
                        .clip(RoundedCornerShape(6.dp))
                        .background(if (selected) Color.White else Color.Transparent)
                        .clickable { onSelectTab(tab) },
                    contentAlignment = Alignment.Center,
                ) {
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(
                            imageVector = if (tab == AppTab.Tools) Icons.Filled.Build else Icons.Filled.Terminal,
                            contentDescription = null,
                            modifier = Modifier.size(13.dp),
                            tint = if (selected) Blue else Muted,
                        )
                        Text(
                            tab.label,
                            color = if (selected) Blue else Muted,
                            fontSize = 11.sp,
                            letterSpacing = 0.sp,
                            fontWeight = FontWeight.SemiBold,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun Header(
    running: Boolean,
    onRefresh: () -> Unit,
) {
    Row(
        modifier = Modifier.fillMaxWidth(),
        horizontalArrangement = Arrangement.SpaceBetween,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(7.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                modifier = Modifier
                    .size(30.dp)
                    .clip(CircleShape)
                    .background(Color.White),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    painter = AndroidAppPainter,
                    contentDescription = null,
                    modifier = Modifier.size(22.dp),
                    tint = Color.Unspecified,
                )
            }
            Column {
                Text("airadb", color = Ink, fontSize = 19.sp, fontWeight = FontWeight.Bold, lineHeight = 20.sp)
                Text(
                    "Android wireless debugging",
                    fontSize = 10.sp,
                    lineHeight = 11.sp,
                    letterSpacing = 0.sp,
                    color = Muted,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }

        Row(
            horizontalArrangement = Arrangement.spacedBy(4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (running) {
                StatusChip(text = "Running", good = true)
            }
            Box(
                modifier = Modifier
                    .size(24.dp)
                    .clip(CircleShape)
                    .clickable(onClick = onRefresh),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    Icons.Filled.Refresh,
                    contentDescription = "Refresh",
                    modifier = Modifier.size(15.dp),
                    tint = Ink,
                )
            }
        }
    }
}

@Composable
private fun ToolsPanel(
    status: StatusSnapshot?,
    statusLoading: Boolean,
    statusError: String?,
    running: Boolean,
    settings: AiradbSettings,
    optionsExpanded: Boolean,
    onSettingsChange: (AiradbSettings) -> Unit,
    onOptionsExpandedChange: (Boolean) -> Unit,
    onPairAndMirror: () -> Unit,
    onStableMirror: () -> Unit,
    onMirrorAndWait: () -> Unit,
    onResetAdb: () -> Unit,
    onInstallShell: () -> Unit,
    onStop: () -> Unit,
    airadbBinary: String,
) {
    val devices = status?.devices.orEmpty()
    val scrollModifier = if (optionsExpanded || devices.size > 1) {
        Modifier.verticalScroll(rememberScrollState())
    } else {
        Modifier
    }

    Column(
        modifier = Modifier
            .fillMaxSize()
            .then(scrollModifier),
        verticalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        Surface(
            modifier = Modifier.fillMaxWidth(),
            color = Panel,
            shape = RoundedCornerShape(8.dp),
        ) {
            Column(
                modifier = Modifier.padding(7.dp),
                verticalArrangement = Arrangement.spacedBy(5.dp),
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Installed", color = Muted, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                    Text(
                        if (statusLoading) "Refreshing" else "${status?.readyDeviceCount() ?: 0} devices",
                        color = Blue,
                        fontSize = 12.sp,
                        letterSpacing = 0.sp,
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
                modifier = Modifier.padding(7.dp),
                verticalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                Row(
                    modifier = Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.SpaceBetween,
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text("Actions", color = Muted, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                    Row(
                        horizontalArrangement = Arrangement.spacedBy(4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        CompactInlineButton(
                            label = if (optionsExpanded) "Done" else "Options",
                            icon = Icons.Filled.Settings,
                            onClick = { onOptionsExpandedChange(!optionsExpanded) },
                        )
                        if (running) {
                            CompactInlineButton("Stop", Icons.Filled.Stop, onStop, danger = true)
                        }
                    }
                }

                Column(verticalArrangement = Arrangement.spacedBy(5.dp)) {
                    Row(horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                        ActionButton("Pair and mirror", Icons.Filled.PlayArrow, running, onPairAndMirror, Modifier.weight(1f))
                        ActionButton("Stable mirror", Icons.Filled.CheckCircle, running, onStableMirror, Modifier.weight(1f))
                        ActionButton("Mirror and wait", Icons.Filled.Terminal, running, onMirrorAndWait, Modifier.weight(1f))
                    }
                    Row(horizontalArrangement = Arrangement.spacedBy(5.dp)) {
                        ActionButton("Reset ADB", Icons.Filled.Refresh, running, onResetAdb, Modifier.weight(1f))
                        ActionButton("Install shell", Icons.Filled.Download, running, onInstallShell, Modifier.weight(1f))
                        Spacer(Modifier.weight(1f))
                    }
                }
            }
        }

        if (optionsExpanded) {
            Surface(
                modifier = Modifier.fillMaxWidth(),
                color = Panel,
                shape = RoundedCornerShape(8.dp),
            ) {
                Column(
                    modifier = Modifier.padding(7.dp),
                    verticalArrangement = Arrangement.spacedBy(6.dp),
                ) {
                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(6.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(
                            Icons.Filled.Settings,
                            contentDescription = null,
                            modifier = Modifier.size(13.dp),
                            tint = Muted,
                        )
                        Column(modifier = Modifier.weight(1f)) {
                            Text("scrcpy options", color = Muted, fontSize = 13.sp, fontWeight = FontWeight.SemiBold)
                            Text(
                                settings.summary(),
                                color = Muted,
                                fontSize = 11.sp,
                                letterSpacing = 0.sp,
                                maxLines = 1,
                                overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }

                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                    ) {
                        OutlinedTextField(
                            value = settings.windowTitle,
                            onValueChange = { onSettingsChange(settings.copy(windowTitle = it)) },
                            modifier = Modifier.weight(1f),
                            label = { Text("Title") },
                            singleLine = true,
                        )
                        OutlinedTextField(
                            value = settings.windowWidth,
                            onValueChange = { onSettingsChange(settings.copy(windowWidth = it.onlyDigits())) },
                            modifier = Modifier.width(68.dp),
                            label = { Text("W") },
                            singleLine = true,
                        )
                        OutlinedTextField(
                            value = settings.windowHeight,
                            onValueChange = { onSettingsChange(settings.copy(windowHeight = it.onlyDigits())) },
                            modifier = Modifier.width(74.dp),
                            label = { Text("H") },
                            singleLine = true,
                        )
                    }

                    Row(
                        modifier = Modifier.fillMaxWidth(),
                        horizontalArrangement = Arrangement.SpaceBetween,
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        ToggleRow(
                            label = "Top",
                            checked = settings.alwaysOnTop,
                            onCheckedChange = { onSettingsChange(settings.copy(alwaysOnTop = it)) },
                        )
                        ToggleRow(
                            label = "Plain",
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
            modifier = Modifier.padding(10.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
        ) {
            Row(
                modifier = Modifier.fillMaxWidth(),
                horizontalArrangement = Arrangement.SpaceBetween,
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Column {
                    Text("Console", color = Muted, fontSize = 15.sp, fontWeight = FontWeight.SemiBold)
                    Text(
                        activeCommand ?: "Idle",
                        fontSize = 12.sp,
                        letterSpacing = 0.sp,
                        color = if (running) Blue else Muted,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                    )
                }
                if (running) {
                    Button(
                        onClick = onStop,
                        modifier = Modifier
                            .height(30.dp)
                            .defaultMinSize(minWidth = 1.dp, minHeight = 1.dp),
                        contentPadding = PaddingValues(horizontal = 10.dp, vertical = 0.dp),
                        colors = ButtonDefaults.buttonColors(containerColor = Color(0xFFD94F5C)),
                    ) {
                        Icon(Icons.Filled.Stop, contentDescription = null, modifier = Modifier.size(14.dp))
                        Spacer(Modifier.width(4.dp))
                        Text("Stop", fontSize = 12.sp)
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
                            .padding(8.dp),
                        verticalArrangement = Arrangement.spacedBy(2.dp),
                    ) {
                        if (logs.isEmpty()) {
                            item {
                                Text(
                                    "No process output yet.",
                                    color = Color(0xFFADB4C7),
                                    fontFamily = FontFamily.Monospace,
                                    fontSize = 11.sp,
                                    letterSpacing = 0.sp,
                                )
                            }
                        } else {
                            items(logs) { line ->
                                Text(
                                    line,
                                    color = Color(0xFFE9EDF7),
                                    fontFamily = FontFamily.Monospace,
                                    fontSize = 11.sp,
                                    letterSpacing = 0.sp,
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
    Surface(
        modifier = Modifier
            .fillMaxWidth()
            .height(34.dp),
        shape = RoundedCornerShape(8.dp),
        color = if (available) Color.White else PanelMuted,
    ) {
        Row(
            modifier = Modifier
                .fillMaxWidth()
                .padding(horizontal = 7.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Box(
                modifier = Modifier
                    .size(22.dp)
                    .clip(CircleShape)
                    .background(if (available) Color(0xFFE2F8EB) else Color(0xFFFFE8EC)),
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    icon,
                    contentDescription = null,
                    modifier = Modifier.size(12.dp),
                    tint = if (available) AndroidGreen else Color(0xFFD94F5C),
                )
            }
            Text(
                name,
                fontSize = 12.sp,
                lineHeight = 13.sp,
                letterSpacing = 0.sp,
                fontWeight = FontWeight.Bold,
                color = Ink,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(
                detail,
                modifier = Modifier.weight(1f),
                color = Muted,
                fontSize = 11.sp,
                lineHeight = 12.sp,
                letterSpacing = 0.sp,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
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
    modifier: Modifier = Modifier,
) {
    Button(
        onClick = onClick,
        enabled = !running,
        modifier = modifier
            .height(28.dp)
            .defaultMinSize(minWidth = 1.dp, minHeight = 1.dp),
        shape = RoundedCornerShape(8.dp),
        contentPadding = PaddingValues(horizontal = 7.dp, vertical = 0.dp),
        colors = ButtonDefaults.buttonColors(containerColor = Blue),
    ) {
        Icon(icon, contentDescription = null, modifier = Modifier.size(12.dp))
        Spacer(Modifier.width(4.dp))
        Text(
            label,
            fontSize = 11.sp,
            lineHeight = 12.sp,
            letterSpacing = 0.sp,
            fontWeight = FontWeight.SemiBold,
            maxLines = 1,
            overflow = TextOverflow.Ellipsis,
        )
    }
}

@Composable
private fun CompactInlineButton(
    label: String,
    icon: androidx.compose.ui.graphics.vector.ImageVector,
    onClick: () -> Unit,
    danger: Boolean = false,
) {
    val tint = if (danger) Color(0xFFA33A45) else Blue
    Box(
        modifier = Modifier
            .height(22.dp)
            .clip(RoundedCornerShape(6.dp))
            .background(if (danger) Color(0xFFFFE8EC) else Color.White)
            .clickable(onClick = onClick)
            .padding(horizontal = 7.dp),
        contentAlignment = Alignment.Center,
    ) {
        Row(
            horizontalArrangement = Arrangement.spacedBy(4.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(icon, contentDescription = null, modifier = Modifier.size(12.dp), tint = tint)
            Text(
                label,
                color = tint,
                fontSize = 11.sp,
                letterSpacing = 0.sp,
                fontWeight = FontWeight.SemiBold,
                maxLines = 1,
            )
        }
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
        Checkbox(checked = checked, onCheckedChange = onCheckedChange, modifier = Modifier.size(30.dp))
        Text(label, color = Ink, fontSize = 11.sp, letterSpacing = 0.sp, maxLines = 1)
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
            modifier = Modifier.padding(horizontal = 6.dp, vertical = 2.dp),
            color = if (good) Color(0xFF157A43) else Color(0xFFA33A45),
            fontSize = 10.sp,
            lineHeight = 11.sp,
            letterSpacing = 0.sp,
            fontWeight = FontWeight.SemiBold,
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
    fun summary(): String {
        val size = listOf(windowWidth, windowHeight)
            .takeIf { values -> values.all(String::isNotBlank) }
            ?.joinToString("x")
            ?: "auto"
        val mode = buildList {
            if (alwaysOnTop) add("top")
            if (plainWindow) add("plain")
        }.joinToString(", ").ifBlank { "borderless" }

        return "$windowTitle · $size · $mode"
    }

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

    suspend fun loadVersion(): String {
        val result = runCapture(listOf(airadbBinary, "--version"), timeoutMillis = 5_000)
        if (result.exitCode != 0) {
            error(result.stderr.ifBlank { result.stdout }.ifBlank { "airadb --version failed" })
        }

        return result.stdout
            .trim()
            .removePrefix("airadb")
            .trim()
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

private fun trayContextMenuHeight(running: Boolean): Int {
    val itemCount = 6 + if (running) 1 else 0
    return itemCount * TrayMenuItemHeightPx + 2 * TrayMenuSeparatorHeightPx + TrayMenuVerticalPaddingPx
}

private fun trayMenuWindowPosition(anchor: Point, running: Boolean): WindowPosition {
    val screen = screenBoundsFor(anchor)
    val width = TrayMenuWidthPx
    val height = trayContextMenuHeight(running)
    val margin = 8
    val minX = screen.x + margin
    val maxX = (screen.x + screen.width - width - margin).coerceAtLeast(minX)
    val minY = screen.y + margin
    val maxY = (screen.y + screen.height - height - margin).coerceAtLeast(minY)
    val x = (anchor.x - width / 2).coerceIn(minX, maxX)
    val y = (anchor.y + 8).coerceIn(minY, maxY)

    return WindowPosition.Absolute(x.dp, y.dp)
}

private fun currentPointerLocation(): Point? =
    runCatching { MouseInfo.getPointerInfo()?.location }.getOrNull()

private fun centeredWindowPosition(width: Int, height: Int, anchor: Point? = currentPointerLocation()): WindowPosition {
    val screen = screenBoundsFor(anchor)
    val x = screen.x + (screen.width - width) / 2
    val y = screen.y + (screen.height - height) / 2

    return WindowPosition.Absolute(x.dp, y.dp)
}

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
