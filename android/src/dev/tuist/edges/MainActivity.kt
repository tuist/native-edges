package dev.tuist.edges

import android.content.Intent
import android.net.Uri
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.BackHandler
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
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
import androidx.compose.material3.OutlinedButton
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.TopAppBar
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import org.json.JSONObject

private val TuistPurple = Color(0xFF6F2CFF)

class MainActivity : ComponentActivity() {
    private var model: AppModel? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        val link = intent?.data?.toString()
        setContent {
            val scope = rememberCoroutineScope()
            val model = remember {
                AppModel(SecureStore(this), scope) { url ->
                    startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(url)))
                }.also {
                    this.model = it
                    it.handle(link ?: AppModel.ROOT)
                }
            }
            MaterialTheme(colorScheme = lightColorScheme(primary = TuistPurple)) {
                App(model)
            }
        }
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        intent.data?.toString()?.let { model?.handle(it) }
    }
}

@Composable
private fun App(model: AppModel) {
    val screen = model.stack.lastOrNull()
    BackHandler(enabled = model.stack.size > 1) { model.back() }
    Surface(Modifier.fillMaxSize()) {
        when {
            model.session == null -> SignIn(model)
            screen != null -> {
                LaunchedEffect(screen.url, model.session != null) { model.load(screen) }
                ScreenView(model, screen)
            }
        }
    }
}

@Composable
private fun SignIn(model: AppModel) {
    Column(
        Modifier.fillMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Text("Edges", style = MaterialTheme.typography.displaySmall, fontWeight = FontWeight.Bold)
        Spacer(Modifier.height(8.dp))
        Text(
            "A headless Rust core with native edges",
            style = MaterialTheme.typography.bodyLarge,
            color = MaterialTheme.colorScheme.onSurfaceVariant,
        )
        Spacer(Modifier.height(40.dp))
        Button(
            onClick = model::signIn,
            enabled = !model.signingIn,
            shape = RoundedCornerShape(12.dp),
            modifier = Modifier.fillMaxWidth().height(52.dp),
        ) {
            if (model.signingIn) {
                CircularProgressIndicator(Modifier.height(20.dp), color = Color.White, strokeWidth = 2.dp)
            } else {
                Text("Sign in with Tuist", style = MaterialTheme.typography.titleMedium)
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
private fun ScreenView(model: AppModel, screen: Screen) {
    val content = model.content[screen.url]
    val loaded = (content as? Content.Loaded)?.screen
    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    Text(loaded?.optString("title") ?: if (screen.route == "projects") "Projects" else "")
                },
                navigationIcon = {
                    if (model.stack.size > 1) TextButton(onClick = { model.back() }) { Text("‹ Back") }
                },
                actions = {
                    if (screen.route == "projects") TextButton(onClick = model::signOut) { Text("Sign out") }
                },
            )
        },
    ) { padding ->
        Box(Modifier.fillMaxSize().padding(padding)) {
            when (content) {
                is Content.Loaded -> when (screen.route) {
                    "projects" -> Projects(content.screen, model::open)
                    else -> Project(content.screen)
                }
                is Content.Failed -> Text(content.message, Modifier.align(Alignment.Center).padding(24.dp))
                else -> CircularProgressIndicator(Modifier.align(Alignment.Center))
            }
        }
    }
}

@Composable
private fun Projects(screen: JSONObject, open: (String) -> Unit) {
    val rows = screen.getJSONArray("rows").let { array -> (0 until array.length()).map(array::getJSONObject) }
    LazyColumn(Modifier.fillMaxSize()) {
        items(rows, key = { it.getString("id") }) { row ->
            ListItem(
                headlineContent = { Text(row.getString("title"), fontWeight = FontWeight.Medium) },
                supportingContent = { Text(row.getString("subtitle")) },
                trailingContent = {
                    Text(row.getString("badge"), style = MaterialTheme.typography.labelMedium, color = TuistPurple)
                },
                modifier = Modifier.clickable { open(row.getString("url")) },
            )
            HorizontalDivider()
        }
    }
}

@Composable
private fun Project(screen: JSONObject) {
    val fields = screen.getJSONArray("fields").let { array -> (0 until array.length()).map(array::getJSONObject) }
    val context = androidx.compose.ui.platform.LocalContext.current
    LazyColumn(Modifier.fillMaxSize()) {
        item {
            Text(
                screen.getString("subtitle"),
                style = MaterialTheme.typography.bodyLarge,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
                modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
            )
        }
        items(fields) { field ->
            ListItem(
                headlineContent = { Text(field.getString("value")) },
                overlineContent = { Text(field.getString("label")) },
            )
            HorizontalDivider()
        }
        item {
            OutlinedButton(
                onClick = {
                    context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(screen.getString("dashboard_url"))))
                },
                modifier = Modifier.fillMaxWidth().padding(16.dp),
            ) { Text("Open in Tuist") }
        }
    }
}
