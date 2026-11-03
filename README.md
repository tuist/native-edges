# Native Edges

The smallest app we could build that follows the **native edges** architecture:
a headless core with no UI and no platform frameworks, rendered by native edges.

| Layer | Technology | Lives in |
| --- | --- | --- |
| Headless core: navigation, sign-in, Tuist API | Rust | [`core/`](core/src) |
| Terminal edge | Rust CLI | [`cli/`](cli/src/main.rs) |
| iOS edge | SwiftUI | [`ios/`](ios/Sources) |
| Android edge | Kotlin and Jetpack Compose | [`android/`](android/src/dev/tuist/edges) |
| Build, test, and cache | [Once](https://github.com/tuist/once), cached in [tuist/native-edges](https://tuist.dev/tuist/native-edges) | [`once.toml`](once.toml) |

The app signs in with Tuist, lists the projects you can access, and opens one.

## The principles, and where to find them

- **Only what needs the platform lives in the edge.** Edges render screens and
  provide platform services. Routing, OAuth, PKCE, token refresh, and every Tuist
  request live in the core ([`tuist.rs`](core/src/tuist.rs)).
- **One narrow bridge.** The core exports one C function that takes a JSON
  command and returns JSON, plus a JNI export that calls the same code
  ([`bridge.rs`](core/src/bridge.rs), [`edges_core.h`](core/include/edges_core.h)).
  The adapters are [`Core.swift`](ios/Sources/Core.swift) and
  [`Core.kt`](android/src/dev/tuist/edges/Core.kt).
- **Platform services stay in the edge.** The core returns the session; iOS keeps
  it in the Keychain ([`SecureStore.swift`](ios/Sources/SecureStore.swift)),
  Android encrypts it with an Android Keystore key
  ([`SecureStore.kt`](android/src/dev/tuist/edges/SecureStore.kt)), and the CLI
  writes a file only you can read.
- **Declarative, addressable navigation.** Routes are declared once in
  [`routes.rs`](core/src/routes.rs). Every screen has a URL, and a tap on a row
  opens that row's URL, so a deep link and a tap take the same path.
- **One build graph.** [`once.toml`](once.toml) models the core, the CLI, and both
  apps. Each edge depends only on the core build it needs, and the cache is
  shared through Tuist.

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

Jetpack Compose comes from Google Maven, and Once doesn't import Maven
artifacts. [`android/maven.py`](android/maven.py) pins the full closure with
SHA-256 digests in [`maven.lock.json`](android/maven.lock.json), and
[`modules/maven.star`](modules/maven.star) hands the unpacked libraries to Once.
Run `python3 android/maven.py lock` to upgrade them.

## Open any screen from the command line

```sh
edges routes                                     # every screen and its URL pattern
edges url project account=tuist project=tuist    # edges://projects/tuist/tuist

xcrun simctl install booted .once/out/IOS/Edges.app
xcrun simctl openurl booted edges://projects/tuist/tuist

adb install -r .once/out/Android/Android.apk
adb shell am start -a android.intent.action.VIEW -d edges://projects/tuist/tuist

edges login
edges open edges://projects/tuist/tuist
```

A link opens on top of its parent screens, so Back works as if you had tapped
your way there. A link that arrives before sign-in opens after it.
