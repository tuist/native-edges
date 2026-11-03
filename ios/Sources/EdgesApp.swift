import EdgesCore
import SwiftUI

@main
struct EdgesApp: App {
    @State private var model = AppModel()

    var body: some Scene {
        WindowGroup {
            RootView(model: model)
                .tint(Color(red: 0.435, green: 0.173, blue: 1))
                .task { if model.root == nil { model.open(AppModel.root) } }
                // Deep links into any screen: xcrun simctl openurl booted edges://conversations/<id>
                .onOpenURL { url in model.open(url.absoluteString) }
        }
    }
}

struct RootView: View {
    @Bindable var model: AppModel

    var body: some View {
        if !model.signedIn {
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

    var body: some View {
        VStack(spacing: 16) {
            Spacer()
            Text("Edges").font(.largeTitle.bold())
            Text("Chat with ChatGPT, stored on this device").foregroundStyle(.secondary)
            Spacer()
            if let code = model.deviceCode {
                Text("Enter this code at ChatGPT").foregroundStyle(.secondary)
                Text(code.userCode).font(.title.monospaced().bold()).textSelection(.enabled)
                Link("Open ChatGPT", destination: URL(string: code.verificationUrl)!).font(.headline)
                ProgressView()
            } else {
                Button {
                    Task { await model.signIn() }
                } label: {
                    Text("Sign in with ChatGPT").font(.headline).frame(maxWidth: .infinity, minHeight: 34)
                }
                .buttonStyle(.borderedProminent)
            }
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
            if screen.route == "conversations" {
                ConversationsView(model: model)
            } else {
                ChatView(model: model, screen: screen)
            }
        }
        .navigationTitle(title)
        .task(id: screen) { await model.load(screen) }
    }

    private var title: String {
        switch screen.route {
        case "conversations": model.list?.title ?? "Conversations"
        case "new": "New conversation"
        default: model.conversations[screen.url]?.title ?? ""
        }
    }
}

struct ConversationsView: View {
    let model: AppModel

    var body: some View {
        let rows = model.list?.rows ?? []
        List(rows, id: \.id) { row in
            // A tap opens the deep link the core built for the row, like any other link.
            Button {
                model.open(row.url)
            } label: {
                VStack(alignment: .leading, spacing: 4) {
                    Text(row.title).foregroundStyle(.primary)
                    Text(row.subtitle).font(.subheadline).foregroundStyle(.secondary).lineLimit(2)
                }
            }
        }
        .overlay {
            if model.list != nil, rows.isEmpty {
                ContentUnavailableView("No conversations yet", systemImage: "bubble.left.and.bubble.right")
            }
        }
        .toolbar {
            Button("Sign out", action: model.signOut)
            Button("New", systemImage: "square.and.pencil") { model.open("edges://conversations/new") }
        }
    }
}

struct ChatView: View {
    let model: AppModel
    let screen: Screen
    @State private var draft = ""

    var body: some View {
        let messages = model.conversations[screen.url]?.messages ?? []
        ScrollView {
            LazyVStack(alignment: .leading, spacing: 12) {
                ForEach(messages.indices, id: \.self) { index in
                    Bubble(message: messages[index])
                }
                if let sending = model.sending {
                    Bubble(message: ChatMessage(role: "user", text: sending))
                    ProgressView().padding()
                }
            }
            .padding()
        }
        .defaultScrollAnchor(.bottom)
        .safeAreaInset(edge: .bottom) {
            HStack {
                TextField("Message", text: $draft, axis: .vertical).textFieldStyle(.roundedBorder)
                Button("Send", systemImage: "arrow.up.circle.fill") {
                    let text = draft
                    draft = ""
                    Task { await model.send(text, from: screen) }
                }
                .labelStyle(.iconOnly).font(.title)
                .disabled(model.sending != nil || draft.trimmingCharacters(in: .whitespaces).isEmpty)
            }
            .padding()
            .background(.bar)
        }
    }
}

struct Bubble: View {
    let message: ChatMessage

    var body: some View {
        switch message.role {
        case "user":
            Text(message.text).padding(10).background(.tint, in: .rect(cornerRadius: 16)).foregroundStyle(.white)
                .frame(maxWidth: .infinity, alignment: .trailing)
        case "tool":
            Label(message.text, systemImage: "wrench.and.screwdriver").font(.caption).foregroundStyle(.secondary)
        default:
            Text(LocalizedStringKey(message.text)).frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
