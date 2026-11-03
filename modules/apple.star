# Once's Swift link step stamps the deployment target as the SDK version, so
# the app would run in iOS 18 compatibility mode. Forward the real SDK to the
# linker. Copied from Atlas (tuist/tuist, atlas-native/modules/apple.star).
def _apple_sdk_impl(ctx):
    sdk = ctx["attr"]["sdk"]
    path = host_command([host_which("xcrun"), "--sdk", sdk, "--show-sdk-path"]).strip()
    return {
        "label_id": ctx["label"]["id"],
        # Joined spelling keeps the pair intact through linker-option deduplication.
        "transitive_linkopts": ["-Xclang-linker", "-isysroot" + path],
    }

apple_sdk = target_kind(
    docs = "Forward the selected iOS SDK to the linker without raising the deployment minimum.",
    attrs = [attr("sdk", "string", required = True, configurable = False, allowed_values = ["iphoneos", "iphonesimulator"])],
    providers = ["apple_linkable"],
    capabilities = [capability("build", ["default"])],
    impl = _apple_sdk_impl,
)
