import EdgesCoreFFI
import Foundation

struct CoreError: LocalizedError {
    let kind: String
    let message: String
    var errorDescription: String? { message }
}

/// The one bridge into the Rust core: a JSON command in, a JSON result out.
enum Core {
    static func run(_ command: [String: Any]) async throws -> [String: Any] {
        let input = String(decoding: try JSONSerialization.data(withJSONObject: command), as: UTF8.self)
        // Commands that reach Tuist block, so they run off the main thread.
        let output = await Task.detached {
            input.withCString { pointer in
                let result = edges_call(pointer)!
                defer { edges_free(result) }
                return String(cString: result)
            }
        }.value
        let object = try JSONSerialization.jsonObject(with: Data(output.utf8)) as? [String: Any] ?? [:]
        if let error = object["error"] as? [String: Any] {
            throw CoreError(kind: error["kind"] as? String ?? "internal", message: error["message"] as? String ?? "")
        }
        return object["ok"] as? [String: Any] ?? [:]
    }
}
