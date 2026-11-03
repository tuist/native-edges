# Once's Android rules consume library providers but don't import Maven AARs.
# `android/maven.py prepare` verifies and unpacks the locked Jetpack Compose
# closure into .once/vendor-maven; this kind exposes it as one android_library
# provider: class jars, compiled AAR resources, the arm64 native libraries, and
# a jar of the libraries' R classes generated against the app's resources.
# Adapted from Atlas's Firebase importer (tuist/tuist, atlas-native/modules).

def _maven_android_libraries_impl(ctx):
    vendor = [path for path in walk_files(".once/vendor-maven") if not path.startswith(".once/vendor-maven/downloads/")]
    if not vendor:
        fail("Run `mise run prepare:android` before building the Android app")
    jars = sorted([path for path in vendor if path.endswith("/classes.jar")])
    manifests = sorted([path for path in vendor if path.endswith("/AndroidManifest.xml")])
    natives = sorted([path for path in vendor if "/jni/arm64-v8a/" in path and path.endswith(".so")])
    app = glob(["android/AndroidManifest.xml", "android/res/**"])
    sdk = host_env("ANDROID_HOME") or host_env("ANDROID_SDK_ROOT")
    if not sdk:
        fail("Set ANDROID_HOME to the installed Android SDK")
    aapt2 = sdk + "/build-tools/36.0.0/aapt2"
    android_jar = sdk + "/platforms/android-36/android.jar"
    r_jar = declare_output("compose-r.jar")
    r_txt = declare_output("R.txt")
    linked = declare_output("resources.apk")
    resources = []
    arguments = []
    for manifest in manifests:
        name = manifest.split("/")[-2]
        compiled = declare_output("resources/" + name + ".zip")
        resources.append(compiled)
        arguments.extend([manifest, compiled])
    run_action(
        display_name = "Link Jetpack Compose resource classes",
        argv = [host_which("python3"), "android/resources.py", "build", aapt2, android_jar, host_which("javac"), host_which("jar"), r_jar, r_txt, linked] + arguments,
        inputs = glob(ctx["srcs"]) + app + [path for path in vendor if "/res/" in path or path.endswith("/AndroidManifest.xml")] + ["android/resources.py"],
        outputs = [r_jar, r_txt, linked] + resources,
        env = {"PATH": host_env("PATH"), "JAVA_HOME": host_env("JAVA_HOME")},
        toolchain_identity = host_file_sha256(aapt2) + "\x00" + host_file_sha256(android_jar) + "\x00" + host_command([host_which("javac"), "-version"]),
        identifier = "maven_android_resources:" + ctx["label"]["id"],
    )
    return {
        "label_id": ctx["label"]["id"],
        "transitive_compile_jars": jars + [r_jar],
        "transitive_runtime_jars": jars + [r_jar],
        "transitive_compiled_resource_zips": resources,
        "transitive_android_native_libraries": [{"abi": "arm64-v8a", "path": path} for path in natives],
    }

maven_android_libraries = target_kind(
    docs = "Import the checksum-pinned Jetpack Compose AARs, including their R classes and native libraries.",
    attrs = [],
    providers = ["android_library"],
    capabilities = [capability("build", ["default"])],
    impl = _maven_android_libraries_impl,
)
