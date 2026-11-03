import AuthenticationServices
import Foundation
import Observation
import SwiftUI

struct Screen: Hashable {
    let route: String
    let params: [String: String]
    let url: String
}

enum Content {
    case loading
    case loaded([String: Any])
    case failed(String)
}

/// Edge state only: which screens are on the stack and what the core returned
/// for them. Routing, sign-in, and Tuist access all happen in the core.
@MainActor @Observable
final class AppModel {
    static let root = "edges://projects"
    static let callback = "edges://oauth-callback"

    private(set) var session = SecureStore.read()
    var root: Screen?
    var path: [Screen] = []
    var content: [String: Content] = [:]
    var message: String?
    private(set) var signingIn = false
    private var pendingLink: String?

    /// Every navigation, from a tap or from a deep link, goes through a URL.
    func open(_ link: String) async {
        do {
            let stack = try await Core.run(["command": "open", "url": link])["stack"] as? [[String: Any]] ?? []
            let screens = stack.map {
                Screen(route: $0["route"] as! String, params: $0["params"] as! [String: String], url: $0["url"] as! String)
            }
            root = screens.first
            path = Array(screens.dropFirst())
            if session == nil { pendingLink = link }
        } catch {
            message = error.localizedDescription
        }
    }

    func signIn(with webAuthentication: WebAuthenticationSession) async {
        signingIn = true
        defer { signingIn = false }
        do {
            let started = try await Core.run(["command": "sign_in", "redirect_uri": Self.callback])
            let callback = try await webAuthentication.authenticate(
                using: URL(string: started["authorize_url"] as! String)!,
                callbackURLScheme: "edges",
                preferredBrowserSession: .ephemeral
            )
            let result = try await Core.run([
                "command": "complete_sign_in",
                "pending": started["pending"]!,
                "callback_url": callback.absoluteString,
            ])
            persist(result["session"] as! [String: Any])
            await open(pendingLink ?? Self.root)
            pendingLink = nil
        } catch ASWebAuthenticationSessionError.canceledLogin {
            // The person closed the sheet.
        } catch {
            message = error.localizedDescription
        }
    }

    func load(_ screen: Screen, refresh: Bool = false) async {
        guard let session else { return }
        if !refresh, case .loaded = content[screen.url] { return }
        content[screen.url] = .loading
        var command: [String: Any] = ["command": screen.route, "session": session]
        command.merge(screen.params) { $1 }
        do {
            let result = try await Core.run(command)
            if let renewed = result["session"] as? [String: Any] { persist(renewed) }
            content[screen.url] = .loaded(result["screen"] as! [String: Any])
        } catch let error as CoreError {
            if error.kind == "unauthorized" { signOut() }
            content[screen.url] = .failed(error.message)
        } catch {
            content[screen.url] = .failed(error.localizedDescription)
        }
    }

    func signOut() {
        SecureStore.clear()
        session = nil
        content = [:]
    }

    private func persist(_ session: [String: Any]) {
        SecureStore.write(session)
        self.session = session
    }
}
