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
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Button
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.ListItem
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.OutlinedTextField
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
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
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import dev.tuist.edges.core.ChatMessage
import dev.tuist.edges.core.Screen

private val Purple = Color(0xFF6F2CFF)

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
            MaterialTheme(colorScheme = lightColorScheme(primary = Purple)) { App(model) }
        }
    }

    // Deep links into any screen: adb shell am start -a android.intent.action.VIEW -d edges://conversations/<id>
    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        intent.data?.toString()?.let { model?.open(it) }
    }
}

@Composable
private fun App(model: AppModel) {
    val screen = model.stack.lastOrNull()
    BackHandler(enabled = model.stack.size > 1) { model.back() }
    Surface(Modifier.fillMaxSize()) {
        when {
            !model.signedIn -> SignIn(model)
            screen != null -> {
                LaunchedEffect(screen.url) { model.load(screen) }
                ScreenView(model, screen)
            }
        }
    }
}

@Composable
private fun SignIn(model: AppModel) {
    val context = LocalContext.current
    Column(
        Modifier.fillMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("Edges", style = MaterialTheme.typography.displaySmall, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(8.dp))
        Text("Chat with ChatGPT, stored on this device", color = MaterialTheme.colorScheme.onSurfaceVariant)
        Spacer(Modifier.height(40.dp))
        val code = model.deviceCode
        if (code != null) {
            Text("Enter this code at ChatGPT", color = MaterialTheme.colorScheme.onSurfaceVariant)
            Text(code.userCode, style = MaterialTheme.typography.headlineMedium, fontFamily = FontFamily.Monospace)
            TextButton(onClick = {
                context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(code.verificationUrl)))
            }) { Text("Open ChatGPT") }
            CircularProgressIndicator()
        } else {
            Button(
                onClick = { model.signIn() },
                shape = RoundedCornerShape(12.dp),
                modifier = Modifier.fillMaxWidth().height(52.dp),
            ) { Text("Sign in with ChatGPT", style = MaterialTheme.typography.titleMedium) }
        }
        model.message?.let {
            Spacer(Modifier.height(16.dp))
            Text(it, color = MaterialTheme.colorScheme.error)
        }
    }
}

@OptIn(ExperimentalMaterial3Api::class)
@Composable
private fun ScreenView(model: AppModel, screen: Screen) {
    val title = when (screen.route) {
        "conversations" -> model.list?.title ?: "Conversations"
        "new" -> "New conversation"
        else -> model.conversations[screen.url]?.title ?: ""
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = { Text(title) },
                navigationIcon = {
                    if (model.stack.size > 1) TextButton(onClick = { model.back() }) { Text("‹ Back") }
                },
                actions = {
                    if (screen.route == "conversations") {
                        TextButton(onClick = model::signOut) { Text("Sign out") }
                        TextButton(onClick = { model.open("edges://conversations/new") }) { Text("New") }
                    }
                },
            )
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding).imePadding()) {
            if (screen.route == "conversations") Conversations(model) else Chat(model, screen)
            model.message?.let { Text(it, Modifier.align(Alignment.Center).padding(24.dp)) }
        }
    }
}

@Composable
private fun Conversations(model: AppModel) {
    val rows = model.list?.rows.orEmpty()
    if (model.list != null && rows.isEmpty()) {
        Text("No conversations yet", Modifier.fillMaxWidth().padding(32.dp))
    }
    LazyColumn(Modifier.fillMaxSize()) {
        items(rows, key = { it.id }) { row ->
            // A tap opens the deep link the core built for the row, like any other link.
            ListItem(
                headlineContent = { Text(row.title, fontWeight = FontWeight.Medium) },
                supportingContent = { Text(row.subtitle, maxLines = 2) },
                modifier = Modifier.clickable { model.open(row.url) },
            )
            HorizontalDivider()
        }
    }
}

@Composable
private fun Chat(model: AppModel, screen: Screen) {
    var draft by rememberSaveable(screen.url) { mutableStateOf("") }
    val messages = model.conversations[screen.url]?.messages.orEmpty()
    Column(Modifier.fillMaxSize()) {
        LazyColumn(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 16.dp), reverseLayout = true) {
            model.sending?.let { text ->
                item { CircularProgressIndicator(Modifier.padding(12.dp)) }
                item { Bubble(ChatMessage("user", text)) }
            }
            items(messages.reversed()) { Bubble(it) }
        }
        Row(Modifier.padding(12.dp), verticalAlignment = Alignment.CenterVertically) {
            OutlinedTextField(draft, { draft = it }, Modifier.weight(1f), placeholder = { Text("Message") })
            TextButton(
                onClick = {
                    model.send(draft, screen)
                    draft = ""
                },
                enabled = model.sending == null && draft.isNotBlank(),
            ) { Text("Send") }
        }
    }
}

@Composable
private fun Bubble(message: ChatMessage) {
    when (message.role) {
        "user" -> Box(Modifier.fillMaxWidth().padding(vertical = 6.dp), contentAlignment = Alignment.CenterEnd) {
            Text(
                message.text,
                color = Color.White,
                modifier = Modifier.background(Purple, RoundedCornerShape(16.dp)).padding(10.dp),
            )
        }
        "tool" -> Text(
            "🔧 ${message.text}",
            style = MaterialTheme.typography.labelMedium,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
            modifier = Modifier.padding(vertical = 4.dp),
        )
        else -> Text(message.text, Modifier.padding(vertical = 6.dp))
    }
}
