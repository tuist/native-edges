package dev.tuist.edges

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.consumeWindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawingPadding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.automirrored.filled.ArrowBack
import androidx.compose.material.icons.automirrored.filled.Send
import androidx.compose.material.icons.filled.Build
import androidx.compose.material.icons.filled.Edit
import androidx.compose.material.icons.filled.MoreVert
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.ExtendedFloatingActionButton
import androidx.compose.material3.FilledIconButton
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalClipboardManager
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import dev.tuist.edges.core.ChatMessage
import dev.tuist.edges.core.Destination

private val Purple = Color(0xFF6F2CFF)
private val Lavender = Color(0xFFCDB8FF)

class MainActivity : ComponentActivity() {
    private var model: AppModel? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val link = intent?.data?.toString()
        setContent {
            val scope = rememberCoroutineScope()
            val model = remember {
                AppModel(scope, filesDir.path, Keystore(this)).also {
                    this.model = it
                    it.open(link ?: AppModel.ROOT)
                }
            }
            val colors = if (isSystemInDarkTheme()) darkColorScheme(primary = Lavender) else lightColorScheme(primary = Purple)
            MaterialTheme(colorScheme = colors) { App(model) }
        }
    }

    // Deep links into any destination: adb shell am start -a android.intent.action.VIEW -d edges://conversations/<id>
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        intent.data?.toString()?.let { model?.open(it) }
    }
}

@Composable
private fun App(model: AppModel) {
    val destination = model.stack.lastOrNull()
    BackHandler(enabled = model.stack.size > 1) { model.back() }
    Surface(Modifier.fillMaxSize()) {
        when {
            !model.signedIn -> SignIn(model)
            destination != null -> {
                LaunchedEffect(destination.url) { model.load(destination) }
                if (destination.route == "conversations") Conversations(model) else Chat(model, destination)
            }
        }
    }
}

@Composable
private fun SignIn(model: AppModel) {
    val context = LocalContext.current
    val clipboard = LocalClipboardManager.current
    Column(
        Modifier.fillMaxSize().safeDrawingPadding().padding(24.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Spacer(Modifier.weight(1f))
        Text("Edges", style = MaterialTheme.typography.displaySmall, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(8.dp))
        Text(
            "Chat with ChatGPT. Conversations stay on this device.",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            textAlign = TextAlign.Center,
        )
        Spacer(Modifier.weight(1f))
        val code = model.deviceCode
        if (code != null) {
            Surface(color = MaterialTheme.colorScheme.surfaceContainerHigh, shape = RoundedCornerShape(28.dp)) {
                Column(Modifier.fillMaxWidth().padding(24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("Enter this code at ChatGPT", color = MaterialTheme.colorScheme.onSurfaceVariant)
                    Spacer(Modifier.height(8.dp))
                    Text(code.userCode, style = MaterialTheme.typography.headlineLarge, fontFamily = FontFamily.Monospace)
                    TextButton(onClick = { clipboard.setText(AnnotatedString(code.userCode)) }) { Text("Copy code") }
                    LinearProgressIndicator(Modifier.fillMaxWidth().padding(top = 8.dp))
                }
            }
            Spacer(Modifier.height(16.dp))
            Button(
                onClick = { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(code.verificationUrl))) },
                modifier = Modifier.fillMaxWidth().height(56.dp),
            ) { Text("Open ChatGPT") }
        } else {
            Button(onClick = { model.signIn() }, modifier = Modifier.fillMaxWidth().height(56.dp)) {
                Text("Sign in with ChatGPT")
            }
        }
        model.message?.let {
            Spacer(Modifier.height(16.dp))
            Text(it, color = MaterialTheme.colorScheme.error)
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Conversations(model: AppModel) {
    var menu by remember { mutableStateOf(false) }
    val rows = model.list?.rows.orEmpty()
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(model.list?.title ?: "Conversations") },
                actions = {
                    IconButton(onClick = { menu = true }) { Icon(Icons.Default.MoreVert, "More options") }
                    DropdownMenu(expanded = menu, onDismissRequest = { menu = false }) {
                        DropdownMenuItem(text = { Text("Sign out") }, onClick = {
                            menu = false
                            model.signOut()
                        })
                    }
                },
            )
        },
        floatingActionButton = {
            ExtendedFloatingActionButton(
                onClick = { model.open("edges://conversations/new") },
                icon = { Icon(Icons.Default.Edit, null) },
                text = { Text("New chat") },
            )
        },
    ) { padding ->
        if (model.list != null && rows.isEmpty()) {
            Column(
                Modifier.fillMaxSize().padding(padding).padding(32.dp),
                verticalArrangement = Arrangement.Center,
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Text("No conversations yet", style = MaterialTheme.typography.titleLarge)
                Text(
                    "Conversations you start are stored on this device.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    textAlign = TextAlign.Center,
                )
            }
        }
        LazyColumn(Modifier.fillMaxSize(), contentPadding = padding) {
            items(rows, key = { it.id }) { row ->
                // A tap opens the deep link the core built for the row, like any other link.
                ListItem(
                    headlineContent = { Text(row.title, maxLines = 1, overflow = TextOverflow.Ellipsis) },
                    supportingContent = { Text(row.subtitle, maxLines = 2, overflow = TextOverflow.Ellipsis) },
                    modifier = Modifier.clickable { model.open(row.url) },
                )
                HorizontalDivider()
            }
        }
        model.message?.let { Text(it, Modifier.padding(padding).padding(24.dp)) }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun Chat(model: AppModel, destination: Destination) {
    var draft by rememberSaveable(destination.url) { mutableStateOf("") }
    val conversation = model.conversations[destination.url]
    val messages = conversation?.messages.orEmpty()
    val send = {
        if (draft.isNotBlank() && model.sending == null) {
            model.send(draft.trim(), destination)
            draft = ""
        }
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(if (destination.route == "new") "New chat" else conversation?.title.orEmpty(), maxLines = 1, overflow = TextOverflow.Ellipsis)
                },
                navigationIcon = {
                    IconButton(onClick = { model.back() }) { Icon(Icons.AutoMirrored.Filled.ArrowBack, "Back") }
                },
            )
        },
    ) { padding ->
        Column(Modifier.fillMaxSize().padding(padding).consumeWindowInsets(padding).imePadding()) {
            LazyColumn(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 16.dp), reverseLayout = true) {
                model.sending?.let { text ->
                    item { Thinking() }
                    item { Bubble(ChatMessage("user", text)) }
                }
                items(messages.reversed()) { Bubble(it) }
            }
            model.message?.let { Text(it, Modifier.padding(horizontal = 16.dp), color = MaterialTheme.colorScheme.error) }
            Row(Modifier.padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
                OutlinedTextField(
                    draft,
                    { draft = it },
                    Modifier.weight(1f),
                    placeholder = { Text("Message") },
                    shape = RoundedCornerShape(28.dp),
                    maxLines = 5,
                )
                Spacer(Modifier.size(8.dp))
                FilledIconButton(onClick = send, enabled = model.sending == null && draft.isNotBlank()) {
                    Icon(Icons.AutoMirrored.Filled.Send, "Send")
                }
            }
        }
    }
}

@Composable
private fun Thinking() {
    Box(Modifier.fillMaxWidth().padding(vertical = 6.dp)) {
        CircularProgressIndicator(
            Modifier
                .background(MaterialTheme.colorScheme.surfaceContainerHigh, RoundedCornerShape(20.dp))
                .padding(12.dp)
                .size(20.dp),
            strokeWidth = 2.dp,
        )
    }
}

@Composable
private fun Bubble(message: ChatMessage) {
    when (message.role) {
        "tool" -> Row(
            Modifier.fillMaxWidth().padding(vertical = 4.dp),
            horizontalArrangement = Arrangement.Center,
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(Icons.Default.Build, null, Modifier.size(14.dp), tint = MaterialTheme.colorScheme.onSurfaceVariant)
            Spacer(Modifier.size(6.dp))
            Text(
                "Used ${message.text.replace('_', ' ')}",
                style = MaterialTheme.typography.labelMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
        }
        else -> {
            val mine = message.role == "user"
            Box(
                Modifier.fillMaxWidth().padding(vertical = 4.dp),
                contentAlignment = if (mine) Alignment.CenterEnd else Alignment.CenterStart,
            ) {
                Text(
                    message.text,
                    color = if (mine) MaterialTheme.colorScheme.onPrimary else MaterialTheme.colorScheme.onSurface,
                    modifier = Modifier
                        .widthIn(max = 300.dp)
                        .background(
                            if (mine) MaterialTheme.colorScheme.primary else MaterialTheme.colorScheme.surfaceContainerHigh,
                            RoundedCornerShape(20.dp),
                        )
                        .padding(horizontal = 14.dp, vertical = 10.dp),
                )
            }
        }
    }
}
