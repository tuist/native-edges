package dev.tuist.edges

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import dev.tuist.edges.core.ConversationList
import dev.tuist.edges.core.Conversation
import dev.tuist.edges.core.CoreException
import dev.tuist.edges.core.DeviceCode
import dev.tuist.edges.core.Destination
import dev.tuist.edges.core.conversation
import dev.tuist.edges.core.conversations
import dev.tuist.edges.core.isSignedIn
import dev.tuist.edges.core.launch
import dev.tuist.edges.core.pollSignIn
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import dev.tuist.edges.core.open as openLink
import dev.tuist.edges.core.send as sendMessage
import dev.tuist.edges.core.signIn as startSignIn
import dev.tuist.edges.core.signOut as forgetSession

/**
 * Edge state only: which destinations are on the stack and what the core returned
 * for them. Routing, sign-in, ChatGPT, and the database all live in the core.
 */
class AppModel(private val scope: CoroutineScope, dataDir: String, storage: Keystore) {
    var signedIn by mutableStateOf(false)
        private set
    var deviceCode by mutableStateOf<DeviceCode?>(null)
        private set
    var stack by mutableStateOf(emptyList<Destination>())
        private set
    var list by mutableStateOf<ConversationList?>(null)
        private set
    val conversations = mutableStateMapOf<String, Conversation>()
    var sending by mutableStateOf<String?>(null)
        private set
    var message by mutableStateOf<String?>(null)

    /** Opens the database, runs pending migrations, and hands the core the Keystore. */
    init {
        launch(dataDir, storage)
        signedIn = isSignedIn()
    }

    /** Every navigation, from a tap or from a deep link, goes through a URL. */
    fun open(link: String) = perform { stack = openLink(link) }

    fun back(): Boolean {
        if (stack.size <= 1) return false
        stack = stack.dropLast(1)
        return true
    }

    /** Shows a code, then polls until the person enters it at ChatGPT. */
    fun signIn() = perform {
        val code = background { startSignIn() }
        deviceCode = code
        try {
            while (!background { pollSignIn(code) }) delay(code.interval.toLong() * 1000)
            signedIn = true
        } finally {
            deviceCode = null
        }
    }

    fun load(destination: Destination) = perform {
        when (destination.route) {
            "conversations" -> list = conversations()
            "conversation" -> conversations[destination.url] = conversation(destination.params.getValue("id"))
        }
    }

    /** Sends a message; a new conversation then replaces the "new" destination with its own URL. */
    fun send(text: String, from: Destination) = perform {
        sending = text
        try {
            val reply = background { sendMessage(from.params["id"], text) }
            conversations[reply.url] = reply
            if (from.route == "new") stack = openLink(reply.url)
        } finally {
            sending = null
        }
    }

    fun signOut() {
        forgetSession()
        signedIn = false
    }

    private fun perform(work: suspend () -> Unit) {
        scope.launch {
            try {
                message = null
                work()
            } catch (error: CoreException.Unauthorized) {
                signedIn = false
            } catch (error: CoreException) {
                message = error.message
            }
        }
    }

    companion object {
        const val ROOT = "edges://conversations"
    }
}

/** Signing in and talking to ChatGPT block, so they run off the main thread. */
private suspend fun <T> background(work: () -> T): T = withContext(Dispatchers.IO) { work() }
