import EdgesCore
import Foundation
import Observation

/// Edge state only: which screens are on the stack and what the core returned
/// for them. Routing, sign-in, ChatGPT, and the database all live in the core.
@MainActor @Observable
final class AppModel {
    static let root = "edges://conversations"

    private(set) var signedIn = false
    private(set) var deviceCode: DeviceCode?
    var root: Screen?
    var path: [Screen] = []
    var list: ConversationList?
    var conversations: [String: ConversationScreen] = [:]
    var sending: String?
    var message: String?

    /// Opens the database, runs pending migrations, and hands the core the Keychain.
    init() {
        let directory = URL.applicationSupportDirectory
        try? FileManager.default.createDirectory(at: directory, withIntermediateDirectories: true)
        perform { _ = try launch(dataDir: directory.path, storage: Keychain()) }
        signedIn = isSignedIn()
    }

    /// Every navigation, from a tap or from a deep link, goes through a URL.
    func open(_ link: String) {
        perform {
            let stack = try EdgesCore.open(url: link)
            root = stack.first
            path = Array(stack.dropFirst())
        }
    }

    /// Shows a code, then polls until the person enters it at ChatGPT.
    func signIn() async {
        await perform {
            let code = try await background { try EdgesCore.signIn() }
            deviceCode = code
            defer { deviceCode = nil }
            while try await !background({ try pollSignIn(code: code) }) {
                try await Task.sleep(for: .seconds(code.interval))
            }
            signedIn = true
        }
    }

    func load(_ screen: Screen) async {
        await perform {
            switch screen.route {
            case "conversations": list = try EdgesCore.conversations()
            case "conversation": conversations[screen.url] = try conversation(id: screen.params["id"]!)
            default: break
            }
        }
    }

    /// Sends a message; a new conversation then replaces the "new" screen with its own URL.
    func send(_ text: String, from screen: Screen) async {
        sending = text
        defer { sending = nil }
        await perform {
            let id = screen.params["id"]
            let reply = try await background { try EdgesCore.send(id: id, text: text) }
            conversations[reply.url] = reply
            if screen.route == "new" { open(reply.url) }
        }
    }

    func signOut() {
        EdgesCore.signOut()
        signedIn = false
    }

    private func perform(_ work: () throws -> Void) {
        do {
            message = nil
            try work()
        } catch {
            handle(error)
        }
    }

    private func perform(_ work: () async throws -> Void) async {
        do {
            message = nil
            try await work()
        } catch {
            handle(error)
        }
    }

    private func handle(_ error: Error) {
        switch error {
        case CoreError.Unauthorized: signedIn = false
        case let CoreError.Failed(message): self.message = message
        case is CancellationError: break
        default: message = error.localizedDescription
        }
    }
}

/// Signing in and talking to ChatGPT block, so they run off the main thread.
private func background<T: Sendable>(_ work: @escaping @Sendable () throws -> T) async throws -> T {
    try await Task.detached(operation: work).value
}
