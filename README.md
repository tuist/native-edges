# Native Edges

The smallest app we could build that follows the **native edges** architecture:
a headless core with no UI and no platform frameworks, rendered by native edges.

| Layer | Technology | Lives in |
| --- | --- | --- |
| Headless core: navigation, sign-in, ChatGPT, SQLite | Rust | [`core/`](core/src) |
| Terminal edge | Rust CLI | [`cli/`](cli/src/main.rs) |
| iOS edge | SwiftUI | [`ios/`](ios/Sources) |
| Android edge | Kotlin and Jetpack Compose | [`android/`](android/src/dev/tuist/edges) |
| Build, test, and cache | [Once](https://github.com/tuist/once), cached in [tuist/native-edges](https://tuist.dev/tuist/native-edges) | [`once.toml`](once.toml) |

You sign in with ChatGPT and chat with it. Conversations are stored on the
device, and ChatGPT can call tools that read your earlier conversations.

## The principles, and where to find them

- **Only what needs the platform lives in the edge.** Edges render screens and
  provide platform services. Routing, the device-code sign-in, token refresh,
  the agent loop ([`conversations.rs`](core/src/conversations.rs)), and the
  SQLite database with its migrations ([`store.rs`](core/src/store.rs)) live in
  the core.
- **One narrow bridge.** [`bridge.rs`](core/src/bridge.rs) is the core's whole
  public surface, annotated for [UniFFI](https://github.com/mozilla/uniffi-rs).
  The graph generates Swift and Kotlin bindings from it, so the apps call typed
  functions such as `open(url:)` and `send(id:text:)` and get records back. The
  CLI calls the same functions in Rust.
- **Platform services stay in the edge.** The core declares a `SecureStorage`
  trait that each edge implements: the Keychain on iOS
  ([`Keychain.swift`](ios/Sources/Keychain.swift)), an Android Keystore key on
  Android ([`Keystore.kt`](android/src/dev/tuist/edges/Keystore.kt)), and a
  file only you can read in the terminal. The core decides what to keep; the
  edge decides how to keep it safe.
- **Declarative, addressable navigation.** Routes are declared once in
  [`routes.rs`](core/src/routes.rs). Every screen has a URL, and a tap on a row
  opens that row's URL, so a deep link and a tap take the same path.
- **One build graph.** [`once.toml`](once.toml) models the core, the binding
  generator, the CLI, and both apps. Each edge depends only on the core build it
  needs, and the cache is shared through Tuist.

## Develop

Install the tools and Rust targets with [mise](https://mise.jdx.dev):

```sh
mise install
mise run test           # validate the graph and test the core
mise run build:cli      # .once/out/CLI/edges
mise run build:ios      # .once/out/IOS/Edges.app
mise run build:android  # .once/out/Android/Android.apk
```

The iOS app needs Xcode and an Apple silicon Mac. The Android app needs the
Android SDK (platform 36, build tools 36.0.0) and NDK 27.3; `mise.toml` defaults
`ANDROID_HOME` and `ANDROID_NDK_HOME` to the standard macOS locations. Builds
read the shared cache after `once auth login --provider tuist`; without a
session, set `ONCE_CACHE_PROVIDER=local`.

Once has no UniFFI rule, so [`modules/uniffi.star`](modules/uniffi.star) adds
one: it runs `uniffi-bindgen` (built from [`bindgen/`](bindgen)) on the compiled
core and compiles the result into a Swift module or a Kotlin jar the app links.
Nothing generated is checked in.

Jetpack Compose and JNA come from Maven, and Once doesn't import Maven
artifacts. [`android/maven.py`](android/maven.py) pins the full closure with
SHA-256 digests in [`maven.lock.json`](android/maven.lock.json), and
[`modules/maven.star`](modules/maven.star) hands the unpacked libraries to Once.
Run `python3 android/maven.py lock` to upgrade them.

## Open any screen from the command line

```sh
edges login                                   # sign in with ChatGPT
edges send "My favorite city is Lisbon"       # start a conversation
edges routes                                  # every screen and its URL pattern
edges url conversation id=a5083177c38a0dfd    # edges://conversations/a5083177c38a0dfd

xcrun simctl install booted .once/out/IOS/Edges.app
xcrun simctl openurl booted edges://conversations/new

adb install -r .once/out/Android/Android.apk
adb shell am start -a android.intent.action.VIEW -d edges://conversations/new
```

A link opens on top of its parent screens, so Back works as if you had tapped
your way there. Each edge has its own database, so a conversation ID from the
CLI opens in the CLI; open the app's own rows by their URL.
