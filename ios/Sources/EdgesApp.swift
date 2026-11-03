import AuthenticationServices
import SwiftUI

@main
struct EdgesApp: App {
    @State private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            RootView(model: model)
                .tint(Color(red: 0.435, green: 0.173, blue: 1))
                .task { if model.root == nil { await model.open(AppModel.root) } }
                // Deep links into any screen: xcrun simctl openurl booted edges://projects/tuist/tuist
                .onOpenURL { url in Task { await model.open(url.absoluteString) } }
        }
    }
}

struct RootView: View {
    @Bindable var model: AppModel

    var body: some View {
        if model.session == nil {
            SignInView(model: model)
        } else if let root = model.root {
            NavigationStack(path: $model.path) {
                ScreenView(model: model, screen: root)
                    .navigationDestination(for: Screen.self) { ScreenView(model: model, screen: $0) }
            }
        }
    }
}

struct SignInView: View {
    let model: AppModel
    @Environment(\.webAuthenticationSession) private var webAuthentication

    var body: some View {
        VStack(spacing: 12) {
            Spacer()
            Text("Edges").font(.largeTitle.bold())
            Text("A headless Rust core with native edges").foregroundStyle(.secondary)
            Spacer()
            Button {
                Task { await model.signIn(with: webAuthentication) }
            } label: {
                Group {
                    if model.signingIn { ProgressView().tint(.white) } else { Text("Sign in with Tuist") }
                }
                .font(.headline)
                .frame(maxWidth: .infinity, minHeight: 34)
            }
            .buttonStyle(.borderedProminent)
            .disabled(model.signingIn)
            if let message = model.message {
                Text(message).font(.footnote).foregroundStyle(.red)
            }
        }
        .padding(32)
    }
}

struct ScreenView: View {
    let model: AppModel
    let screen: Screen

    var body: some View {
        Group {
            switch model.content[screen.url] {
            case let .loaded(content) where screen.route == "projects": ProjectsView(model: model, content: content)
            case let .loaded(content): ProjectView(content: content)
            case let .failed(message): ContentUnavailableView(message, systemImage: "exclamationmark.triangle")
            default: ProgressView()
            }
        }
        .navigationTitle(title)
        .toolbar {
            if screen.route == "projects" { Button("Sign out", action: model.signOut) }
        }
        .task(id: screen) { await model.load(screen) }
        .refreshable { await model.load(screen, refresh: true) }
    }

    private var title: String {
        if case let .loaded(content) = model.content[screen.url] { return content["title"] as? String ?? "" }
        return screen.route == "projects" ? "Projects" : screen.params["project"] ?? ""
    }
}

struct ProjectsView: View {
    let model: AppModel
    let content: [String: Any]

    var body: some View {
        List(content["rows"] as? [[String: String]] ?? [], id: \.["id"]) { row in
            // A tap opens the deep link the core built for the row, like any other link.
            Button {
                Task { await model.open(row["url"] ?? "") }
            } label: {
                LabeledContent {
                    Text(row["badge"] ?? "").font(.caption)
                } label: {
                    Text(row["title"] ?? "").foregroundStyle(.primary)
                    Text(row["subtitle"] ?? "")
                }
            }
        }
    }
}

struct ProjectView: View {
    let content: [String: Any]

    var body: some View {
        List {
            Section(content["subtitle"] as? String ?? "") {
                ForEach(content["fields"] as? [[String: String]] ?? [], id: \.["label"]) { field in
                    LabeledContent(field["label"] ?? "", value: field["value"] ?? "")
                }
            }
            if let url = (content["dashboard_url"] as? String).flatMap(URL.init(string:)) {
                Link("Open in Tuist", destination: url)
            }
        }
    }
}
