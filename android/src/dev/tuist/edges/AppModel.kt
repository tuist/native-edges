package dev.tuist.edges

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import org.json.JSONObject

data class Screen(val route: String, val params: Map<String, String>, val url: String)

sealed interface Content {
    data object Loading : Content
    data class Loaded(val screen: JSONObject) : Content
    data class Failed(val message: String) : Content
}

/**
 * Edge state only: which screens are on the stack and what the core returned
 * for them. Routing, sign-in, and Tuist access all happen in the core.
 */
class AppModel(
    private val store: SecureStore,
    private val scope: CoroutineScope,
    private val openBrowser: (String) -> Unit,
) {
    var session by mutableStateOf(store.read()?.let(::JSONObject))
        private set
    var stack by mutableStateOf(emptyList<Screen>())
        private set
    var message by mutableStateOf<String?>(null)
    var signingIn by mutableStateOf(false)
        private set
    val content = mutableStateMapOf<String, Content>()

    private var pending: JSONObject? = null
    private var pendingLink: String? = null

    fun handle(link: String) {
        if (link.startsWith(CALLBACK)) completeSignIn(link) else open(link)
    }

    /** Every navigation, from a tap or from a deep link, goes through a URL. */
    fun open(link: String) {
        scope.launch {
            try {
                val stack = Core.run(Core.command("open", "url" to link)).getJSONArray("stack")
                this@AppModel.stack = (0 until stack.length()).map { index ->
                    val screen = stack.getJSONObject(index)
                    val params = screen.getJSONObject("params")
                    Screen(
                        route = screen.getString("route"),
                        params = params.keys().asSequence().associateWith { params.getString(it) },
                        url = screen.getString("url"),
                    )
                }
                if (session == null) pendingLink = link
            } catch (error: CoreException) {
                message = error.message
            }
        }
    }

    fun back(): Boolean {
        if (stack.size <= 1) return false
        stack = stack.dropLast(1)
        return true
    }

    fun signIn() {
        scope.launch {
            signingIn = true
            try {
                val result = Core.run(Core.command("sign_in", "redirect_uri" to CALLBACK))
                pending = result.getJSONObject("pending")
                openBrowser(result.getString("authorize_url"))
            } catch (error: CoreException) {
                message = error.message
                signingIn = false
            }
        }
    }

    private fun completeSignIn(callback: String) {
        val pending = pending ?: return
        scope.launch {
            try {
                val result = Core.run(Core.command("complete_sign_in", "pending" to pending, "callback_url" to callback))
                persist(result.getJSONObject("session"))
                this@AppModel.pending = null
                open(pendingLink ?: ROOT)
                pendingLink = null
            } catch (error: CoreException) {
                message = error.message
            } finally {
                signingIn = false
            }
        }
    }

    fun load(screen: Screen, refresh: Boolean = false) {
        val session = session ?: return
        if (!refresh && content[screen.url] is Content.Loaded) return
        content[screen.url] = Content.Loading
        scope.launch {
            val command = when (screen.route) {
                "projects" -> Core.command("projects", "session" to session)
                else -> Core.command(
                    "project",
                    "session" to session,
                    "account" to screen.params["account"],
                    "project" to screen.params["project"],
                )
            }
            content[screen.url] = try {
                val result = Core.run(command)
                result.optJSONObject("session")?.let(::persist)
                Content.Loaded(result.getJSONObject("screen"))
            } catch (error: CoreException) {
                if (error.kind == "unauthorized") signOut()
                Content.Failed(error.message ?: "Something went wrong")
            }
        }
    }

    fun signOut() {
        store.clear()
        session = null
        content.clear()
    }

    private fun persist(session: JSONObject) {
        store.write(session.toString())
        this.session = session
    }

    companion object {
        const val ROOT = "edges://projects"
        const val CALLBACK = "edges://oauth-callback"
    }
}
