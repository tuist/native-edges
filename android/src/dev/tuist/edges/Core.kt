package dev.tuist.edges

import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import org.json.JSONObject

class CoreException(val kind: String, message: String) : Exception(message)

/** The one bridge into the Rust core: a JSON command in, a JSON result out. */
object Core {
    init {
        System.loadLibrary("edges_core")
    }

    @JvmStatic
    private external fun call(input: String): String

    /** Runs a command off the main thread and returns its `ok` payload. */
    suspend fun run(command: JSONObject): JSONObject = withContext(Dispatchers.IO) {
        val result = JSONObject(call(command.toString()))
        result.optJSONObject("error")?.let {
            throw CoreException(it.optString("kind"), it.optString("message"))
        }
        result.getJSONObject("ok")
    }

    fun command(name: String, vararg fields: Pair<String, Any?>): JSONObject =
        JSONObject().put("command", name).apply { fields.forEach { (key, value) -> put(key, value) } }
}
