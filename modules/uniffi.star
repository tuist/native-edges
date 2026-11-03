# UniFFI bindings as a graph target. Once has no UniFFI rule, and its
# apple_application and android_binary kinds only compile sources they glob,
# so generated files can't flow into them. This kind generates the bindings
# from the compiled core and compiles them into a library the app links:
#
#   language = "swift"  -> a static Swift module, EdgesCore, plus the C module
#                          EdgesCoreFFI it calls (apple_linkable)
#   language = "kotlin" -> a jar with dev.tuist.edges.core (android_library)
#
# deps: the uniffi-bindgen binary, the core built for the platform, and, for
# Kotlin, the libraries that provide JNA.

def _find(deps, field):
    for dep in deps:
        if dep.get(field):
            return dep[field]
    fail("uniffi_bindings needs a dependency that provides `" + field + "`")

def _generate(ctx, language, library, files):
    outputs = [declare_output("generated/" + name) for name in files]
    out_dir = outputs[0][:-len(files[0]) - 1]
    run_action(
        display_name = "Generate " + language.capitalize() + " bindings",
        argv = [_find(ctx["deps"], "binary"), "generate", "--library", library, "--config", "bindgen/uniffi.toml", "--language", language, "--out-dir", out_dir, "--no-format"],
        inputs = [_find(ctx["deps"], "binary"), library, "bindgen/uniffi.toml"],
        outputs = outputs,
        identifier = "uniffi_generate:" + ctx["label"]["id"],
    )
    return outputs

def _swift(ctx):
    swift, header, modulemap = _generate(ctx, "swift", _find(ctx["deps"], "staticlib"), ["EdgesCore.swift", "EdgesCoreFFI.h", "EdgesCoreFFI.modulemap"])
    sdk = host_command([host_which("xcrun"), "--sdk", "iphonesimulator", "--show-sdk-path"]).strip()
    swiftc = host_command([host_which("xcrun"), "--sdk", "iphonesimulator", "--find", "swiftc"]).strip()
    module = declare_output("EdgesCore.swiftmodule")
    archive = declare_output("libEdgesCore.a")
    run_action(
        display_name = "Compile Swift bindings",
        argv = [swiftc, "-parse-as-library", "-module-name", "EdgesCore", "-swift-version", "5",
                "-target", "arm64-apple-ios" + ctx["attr"]["minimum_os"] + "-simulator", "-sdk", sdk,
                "-Xcc", "-fmodule-map-file=" + modulemap, "-emit-module", "-emit-module-path", module,
                "-emit-library", "-static", "-o", archive, swift],
        inputs = [swift, header, modulemap],
        outputs = [module, archive],
        toolchain_identity = host_command([swiftc, "--version"]) + "\x00" + sdk,
        identifier = "uniffi_swiftc:" + ctx["label"]["id"],
    )
    return {
        "label_id": ctx["label"]["id"],
        "module_name": "EdgesCore",
        "swiftmodule_dir": ctx["build_dir"],
        "archive": archive,
        "modulemap": modulemap,
        "transitive_swiftmodule_dirs": [ctx["build_dir"]],
        "transitive_swiftmodule_inputs": [module],
        "transitive_exported_headers": [header],
        "transitive_exported_header_dirs": [header.rsplit("/", 1)[0]],
        "transitive_modulemaps": [modulemap],
        "transitive_archives": [archive],
    }

def _kotlin(ctx):
    sources = _generate(ctx, "kotlin", _find(ctx["deps"], "dylib"), ["dev/tuist/edges/core/edges_core.kt"])
    classpath = []
    for dep in ctx["deps"]:
        classpath.extend(dep.get("transitive_compile_jars") or [])
    kotlinc = host_which("kotlinc")
    jar = declare_output("edges-core-bindings.jar")
    run_action(
        display_name = "Compile Kotlin bindings",
        argv = [kotlinc, "-jvm-target", "17", "-nowarn", "-classpath", ":".join(classpath), "-d", jar] + sources,
        inputs = sources + classpath,
        outputs = [jar],
        env = {"PATH": host_env("PATH"), "JAVA_HOME": host_env("JAVA_HOME")},
        toolchain_identity = host_command([kotlinc, "-version"]),
        identifier = "uniffi_kotlinc:" + ctx["label"]["id"],
    )
    return {
        "label_id": ctx["label"]["id"],
        "compile_jar": jar,
        "transitive_compile_jars": [jar],
        "transitive_runtime_jars": [jar],
    }

def _uniffi_bindings_impl(ctx):
    return _swift(ctx) if ctx["attr"]["language"] == "swift" else _kotlin(ctx)

uniffi_bindings = target_kind(
    docs = "Generate UniFFI bindings for the core and compile them into a Swift module or a Kotlin jar.",
    attrs = [
        attr("language", "string", required = True, configurable = False, allowed_values = ["swift", "kotlin"]),
        attr("minimum_os", "string"),
    ],
    providers = ["apple_linkable", "android_library"],
    capabilities = [capability("build", ["default"])],
    impl = _uniffi_bindings_impl,
)
