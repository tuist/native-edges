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
        VStack(spacing: 24) {
            Spacer()
            Image(systemName: "bubble.left.and.text.bubble.right.fill")
                .font(.system(size: 64))
                .foregroundStyle(.tint)
            VStack(spacing: 8) {
                Text("Edges").font(.largeTitle.bold())
                Text("Chat with ChatGPT. Conversations stay on this iPhone.")
                    .multilineTextAlignment(.center)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            if let code = model.deviceCode {
                VStack(spacing: 12) {
                    Text("Enter this code at ChatGPT").font(.subheadline).foregroundStyle(.secondary)
                    Text(code.userCode).font(.largeTitle.monospaced().weight(.semibold)).textSelection(.enabled)
                    Button("Copy Code", systemImage: "doc.on.doc") { UIPasteboard.general.string = code.userCode }
                        .font(.subheadline)
                }
                Link(destination: URL(string: code.verificationUrl)!) {
                    Text("Open ChatGPT").frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
                Label {
                    Text("Waiting for you to sign in")
                } icon: {
                    ProgressView()
                }
                .font(.footnote)
                .foregroundStyle(.secondary)
            } else {
                Button {
                    Task { await model.signIn() }
                } label: {
                    Text("Sign In with ChatGPT").frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .controlSize(.large)
            }
            if let message = model.message {
                Text(message).font(.footnote).foregroundStyle(.red)
            }
        }
        .padding(.horizontal, 32)
        .padding(.bottom)
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
        .task(id: screen) { await model.load(screen) }
    }
}

struct ConversationsView: View {
    let model: AppModel

    var body: some View {
        let rows = model.list?.rows ?? []
        List(rows, id: \.id) { row in
            // Each row pushes the screen its deep link resolves to, like any other link.
            NavigationLink(value: model.screen(for: row.url)) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(row.title).font(.headline).lineLimit(1)
                    Text(row.subtitle).font(.subheadline).foregroundStyle(.secondary).lineLimit(2)
                }
                .padding(.vertical, 4)
            }
        }
        .listStyle(.plain)
        .overlay {
            if model.list != nil, rows.isEmpty {
                ContentUnavailableView {
                    Label("No Conversations", systemImage: "bubble.left.and.bubble.right")
                } description: {
                    Text("Conversations you start are stored on this iPhone.")
                } actions: {
                    Button("Start a Conversation") { model.open("edges://conversations/new") }
                }
            }
        }
        .refreshable { await model.load(Screen(route: "conversations", params: [:], url: AppModel.root)) }
        .navigationTitle(model.list?.title ?? "Conversations")
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                Menu("More", systemImage: "ellipsis") {
                    Button("Sign Out", systemImage: "rectangle.portrait.and.arrow.right", role: .destructive, action: model.signOut)
                }
            }
            ToolbarItem(placement: .topBarTrailing) {
                Button("New Conversation", systemImage: "square.and.pencil") { model.open("edges://conversations/new") }
            }
        }
    }
}

struct ChatView: View {
    let model: AppModel
    let screen: Screen
    @State private var draft = ""

    var body: some View {
        let conversation = model.conversations[screen.url]
        ScrollView {
            LazyVStack(spacing: 10) {
                ForEach(Array((conversation?.messages ?? []).enumerated()), id: \.offset) { _, message in
                    Bubble(message: message)
                }
                if let sending = model.sending {
                    Bubble(message: ChatMessage(role: "user", text: sending))
                    HStack {
                        ProgressView().padding(12).background(Color(.secondarySystemFill), in: .rect(cornerRadius: 18))
                        Spacer()
                    }
                }
            }
            .padding()
        }
        .defaultScrollAnchor(.bottom)
        .scrollDismissesKeyboard(.interactively)
        .navigationTitle(screen.route == "new" ? "New Conversation" : conversation?.title ?? "")
        .navigationBarTitleDisplayMode(.inline)
        .safeAreaInset(edge: .bottom) {
            HStack(alignment: .bottom, spacing: 8) {
                TextField("Message", text: $draft, axis: .vertical)
                    .lineLimit(1...5)
                    .padding(.horizontal, 14)
                    .padding(.vertical, 8)
                    .overlay(Capsule().stroke(Color(.separator)))
                    .submitLabel(.send)
                    .onSubmit(send)
                Button("Send", systemImage: "arrow.up.circle.fill", action: send)
                    .labelStyle(.iconOnly)
                    .font(.system(size: 32))
                    .disabled(model.sending != nil || draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty)
            }
            .padding(.horizontal)
            .padding(.vertical, 8)
            .background(.bar)
        }
    }

    private func send() {
        let text = draft.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !text.isEmpty, model.sending == nil else { return }
        draft = ""
        Task { await model.send(text, from: screen) }
    }
}

struct Bubble: View {
    let message: ChatMessage

    var body: some View {
        switch message.role {
        case "user":
            Text(message.text)
                .padding(.horizontal, 14).padding(.vertical, 9)
                .foregroundStyle(.white)
                .background(.tint, in: .rect(cornerRadius: 18))
                .frame(maxWidth: 300, alignment: .trailing)
                .frame(maxWidth: .infinity, alignment: .trailing)
        case "tool":
            Label("Used \(message.text.replacingOccurrences(of: "_", with: " "))", systemImage: "wrench.and.screwdriver")
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(maxWidth: .infinity)
        default:
            Text((try? AttributedString(markdown: message.text, options: .init(interpretedSyntax: .inlineOnlyPreservingWhitespace))) ?? AttributedString(message.text))
                .padding(.horizontal, 14).padding(.vertical, 9)
                .background(Color(.secondarySystemFill), in: .rect(cornerRadius: 18))
                .frame(maxWidth: 300, alignment: .leading)
                .frame(maxWidth: .infinity, alignment: .leading)
        }
    }
}
