#!/usr/bin/env python3
"""Lock and materialize Jetpack Compose and JNA from Maven without Gradle.

Once builds the APK; it does not import Maven artifacts. This script resolves
the roots below through Gradle module metadata (falling back to POMs), pins the
whole closure with SHA-256 digests in maven.lock.json, and unpacks every AAR
into .once/vendor-maven for modules/maven.star.

  python3 android/maven.py lock      # online: re-resolve and rewrite the lock
  python3 android/maven.py prepare   # verify digests and materialize the lock

Adapted from Atlas's Firebase importer (tuist/tuist, atlas-native/android/push).
"""
import argparse
import hashlib
import io
import json
import re
import shutil
import sys
import urllib.error
import urllib.request
import xml.etree.ElementTree as ET
import zipfile
from pathlib import Path

ROOTS = {
    "androidx.activity:activity-compose": "1.12.4",
    "androidx.compose.material3:material3": "1.4.0",
    # Keep the Compose libraries on one release.
    "androidx.compose.foundation:foundation": "1.9.2",
    "androidx.compose.ui:ui": "1.9.2",
    "androidx.compose.material:material-icons-core": "1.7.8",
    # The UniFFI Kotlin bindings call the core through JNA.
    "net.java.dev.jna:jna": "5.19.1",
}
# Published as a JAR for the JVM and as an AAR that carries Android's libjnidispatch.so.
AARS = {"net.java.dev.jna:jna"}
ROOT = Path(__file__).resolve().parents[1]
LOCK = Path(__file__).with_name("maven.lock.json")
VENDOR = ROOT / ".once/vendor-maven"
REPOSITORIES = ["https://dl.google.com/dl/android/maven2", "https://repo.maven.apache.org/maven2"]
POM = {"m": "http://maven.apache.org/POM/4.0.0"}
# The Kotlin toolchain supplies its own standard library; a second copy makes d8 fail.
EXCLUDED = {"org.jetbrains.kotlin:kotlin-stdlib", "org.jetbrains.kotlin:kotlin-stdlib-common",
            "org.jetbrains.kotlin:kotlin-stdlib-jdk7", "org.jetbrains.kotlin:kotlin-stdlib-jdk8",
            "org.jetbrains:annotations"}


def fetch(url):
    with urllib.request.urlopen(url, timeout=60) as response:
        return response.read()


def remote(group, artifact, version, file):
    suffix = f"{group.replace('.', '/')}/{artifact}/{version}/{file}"
    for repository in REPOSITORIES:
        try:
            return f"{repository}/{suffix}", fetch(f"{repository}/{suffix}")
        except urllib.error.HTTPError as error:
            if error.code != 404:
                raise
    return None, None


def version_key(value):
    return tuple((0, int(part)) if part.isdigit() else (1, part) for part in re.split(r"[.-]", value))


def runtime_variant(module):
    """Picks the variant an Android app consumes at runtime."""
    best, best_score = None, -1
    for variant in module.get("variants", []):
        attrs = variant.get("attributes", {})
        if attrs.get("org.gradle.usage") != "java-runtime" or attrs.get("org.gradle.category", "library") != "library":
            continue
        platform = attrs.get("org.jetbrains.kotlin.platform.type")
        score = {"androidJvm": 3, "jvm": 2, None: 1}.get(platform, -1)
        if attrs.get("com.android.build.api.attributes.BuildTypeAttr", "release") != "release":
            score -= 1
        if score > best_score:
            best, best_score = variant, score
    return best


def describe(coordinate, version):
    """Returns (redirect, file name, dependencies, constraints) for one module version."""
    group, artifact = coordinate.split(":")
    _, data = remote(group, artifact, version, f"{artifact}-{version}.module")
    if data:
        module = json.loads(data)
        variant = runtime_variant(module)
        if variant is None:
            raise ValueError(f"No Android runtime variant: {coordinate}:{version}")
        if "available-at" in variant:
            target = variant["available-at"]
            return f"{target['group']}:{target['module']}", target["version"], None, [], []
        files = [f["url"] for f in variant.get("files", []) if f["name"].endswith((".aar", ".jar"))]
        def requirements(entries):
            result = []
            for entry in entries:
                if entry.get("attributes", {}).get("org.gradle.category") in ("platform", "enforced-platform"):
                    continue  # BOMs only carry version alignment.
                name = f"{entry['group']}:{entry['module']}"
                wanted = entry.get("version", {})
                value = wanted.get("strictly") or wanted.get("requires") or wanted.get("prefers")
                if value and name not in EXCLUDED:
                    result.append((name, value.strip("[]")))
            return result
        return (None, None, files[0] if files else None,
                requirements(variant.get("dependencies", [])),
                requirements(variant.get("dependencyConstraints", [])))
    _, pom = remote(group, artifact, version, f"{artifact}-{version}.pom")
    if pom is None:
        raise ValueError(f"Not found: {coordinate}:{version}")
    model = ET.fromstring(pom)
    packaging = model.findtext("m:packaging", default="jar", namespaces=POM)
    dependencies = []
    for dependency in model.findall("m:dependencies/m:dependency", POM):
        text = lambda name, default="": dependency.findtext(f"m:{name}", default=default, namespaces=POM)
        if text("scope", "compile") not in ("compile", "runtime") or text("optional") == "true":
            continue
        name = f"{text('groupId')}:{text('artifactId')}"
        if name not in EXCLUDED:
            dependencies.append((name, text("version").strip("[]")))
    if packaging == "pom":
        return None, None, None, dependencies, []
    extension = "aar" if packaging == "aar" or coordinate in AARS else "jar"
    return None, None, f"{artifact}-{version}.{extension}", dependencies, []


def lock():
    selected = dict(ROOTS)
    constraints = {}
    described = {}
    while True:
        before = (dict(selected), dict(constraints))
        for coordinate, version in list(selected.items()):
            key = (coordinate, version)
            if key not in described:
                described[key] = describe(coordinate, version)
            redirect, redirect_version, _, dependencies, module_constraints = described[key]
            edges = [(redirect, redirect_version)] if redirect else dependencies
            for name, wanted in edges:
                if name not in selected or version_key(wanted) > version_key(selected[name]):
                    selected[name] = wanted
            for name, wanted in module_constraints:
                if name not in constraints or version_key(wanted) > version_key(constraints[name]):
                    constraints[name] = wanted
        # Constraints align versions of modules that are already in the graph (like a BOM).
        for name, wanted in constraints.items():
            if name in selected and version_key(wanted) > version_key(selected[name]):
                selected[name] = wanted
        if (selected, constraints) == before:
            break
    artifacts = []
    for coordinate, version in sorted(selected.items()):
        redirect, _, file, dependencies, _ = described[(coordinate, version)]
        if redirect or file is None:
            continue  # Metadata-only modules point at the artifact that carries the code.
        group, artifact = coordinate.split(":")
        url, data = remote(group, artifact, version, file)
        if data is None:
            raise ValueError(f"Missing {file} for {coordinate}:{version}")
        file = url.rsplit("/", 1)[1]
        artifacts.append({"coordinate": coordinate, "version": version, "url": url,
                          "packaging": file.rsplit(".", 1)[1], "sha256": hashlib.sha256(data).hexdigest()})
        download = VENDOR / "downloads" / file
        download.parent.mkdir(parents=True, exist_ok=True)
        download.write_bytes(data)
    LOCK.write_text(json.dumps({"roots": ROOTS, "artifacts": artifacts}, indent=2) + "\n")
    print(f"Locked {len(artifacts)} artifacts in {LOCK.relative_to(ROOT)}")


def prepare():
    artifacts = json.loads(LOCK.read_text())["artifacts"]
    if VENDOR.exists():
        for path in VENDOR.iterdir():
            if path.is_dir() and path.name != "downloads":
                shutil.rmtree(path)
    for artifact in artifacts:
        name = artifact["url"].rsplit("/", 1)[1]
        download = VENDOR / "downloads" / name
        if not download.exists():
            download.parent.mkdir(parents=True, exist_ok=True)
            download.write_bytes(fetch(artifact["url"]))
        data = download.read_bytes()
        if hashlib.sha256(data).hexdigest() != artifact["sha256"]:
            raise ValueError(f"Integrity check failed for {name}; delete {download} and retry")
        target = VENDOR / artifact["coordinate"].replace(":", "__")
        (target / "res").mkdir(parents=True, exist_ok=True)
        if artifact["packaging"] == "jar":
            (target / "classes.jar").write_bytes(data)
            continue
        with zipfile.ZipFile(io.BytesIO(data)) as archive:
            for entry in archive.infolist():
                path = Path(entry.filename)
                if path.is_absolute() or ".." in path.parts:
                    raise ValueError(f"Unsafe path in {name}")
                if entry.is_dir():
                    continue
                if entry.filename.startswith("jni/") and not entry.filename.startswith("jni/arm64-v8a/"):
                    continue  # The app ships arm64 only.
                if entry.filename.startswith(("res/", "jni/")) or entry.filename == "AndroidManifest.xml":
                    output = target / path
                    output.parent.mkdir(parents=True, exist_ok=True)
                    output.write_bytes(archive.read(entry))
            jars = [e for e in archive.namelist() if e == "classes.jar" or (e.startswith("libs/") and e.endswith(".jar"))]
            with zipfile.ZipFile(target / "classes.jar", "w", zipfile.ZIP_DEFLATED) as merged:
                seen = set()
                for jar in jars:
                    with zipfile.ZipFile(io.BytesIO(archive.read(jar))) as source:
                        for entry in source.infolist():
                            # Kotlin needs .kotlin_module files to find top-level functions.
                            metadata = entry.filename.startswith("META-INF/") and not entry.filename.endswith(".kotlin_module")
                            if entry.is_dir() or metadata or entry.filename in seen:
                                continue
                            seen.add(entry.filename)
                            merged.writestr(entry.filename, source.read(entry))
    print(f"Verified and materialized {len(artifacts)} Maven artifacts")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("action", choices=["lock", "prepare"])
    if parser.parse_args().action == "lock":
        lock()
    prepare()
