#!/usr/bin/env bash
# Compare affected workspace packages and their locked dependencies, without a build.
set -euo pipefail
python3 - "${1:-}" <<'PY'
import json
import pathlib
import subprocess
import sys
import tomllib

base = sys.argv[1]

def read(ref, path):
    if ref:
        result = subprocess.run(["git", "show", f"{ref}:{path}"], capture_output=True)
        return tomllib.loads(result.stdout.decode()) if result.returncode == 0 else {}
    file = pathlib.Path(path)
    return tomllib.loads(file.read_text()) if file.exists() else {}

def workspace(ref):
    return {
        f"{member}/Cargo.toml": read(ref, f"{member}/Cargo.toml")
        for member in read(ref, "Cargo.toml").get("workspace", {}).get("members", [])
    }

def dependency_options(document, kinds):
    tables = [("", document), *document.get("target", {}).items()]
    return {(target, kind, alias): spec for target, table in tables
            for kind in kinds for alias, spec in table.get(kind, {}).items()}

def canonical(alias, spec):
    return spec.get("package", alias) if isinstance(spec, dict) else alias

def resolve(packages, dependency):
    parts = dependency.split()
    return [p for p in packages if p["name"] == parts[0]
            and (len(parts) == 1 or p["version"] == parts[1])]

def closure(lock, name, production=None):
    packages = lock.get("package", [])
    pending = [p for p in packages if p["name"] == name]
    found = {}
    while pending:
        package = pending.pop()
        key = (package["name"], package["version"], package.get("source", ""))
        if key in found:
            continue
        dependencies = package.get("dependencies", [])
        if production is not None and package["name"] in production:
            options = dependency_options(production[package["name"]], ("dependencies", "build-dependencies"))
            names = {canonical(alias, spec) for (_, _, alias), spec in options.items()}
            dependencies = [d for d in dependencies if d.split()[0] in names]
            package = {**package, "dependencies": dependencies}
        found[key] = package
        for dependency in dependencies:
            pending.extend(resolve(packages, dependency))
    return found

application = not base
configuration = False
affected = set()
if base:
    paths = subprocess.check_output(["git", "diff", "--name-only", "-z", base]).decode().split("\0")
    before, after = workspace(base), workspace("")
    old_lock, new_lock = read(base, "Cargo.lock"), read("", "Cargo.lock")
    old_production = {m["package"]["name"]: m for m in before.values() if "package" in m}
    new_production = {m["package"]["name"]: m for m in after.values() if "package" in m}
    old_runtime = closure(old_lock, "fastmash", old_production)
    new_runtime = closure(new_lock, "fastmash", new_production)
    runtime = old_runtime | new_runtime
    runtime_names = {key[0] for key in runtime}
    application = old_runtime != new_runtime
    for manifest in before.keys() | after.keys():
        old, new = before.get(manifest, {}), after.get(manifest, {})
        name = new.get("package", old.get("package", {})).get("name", "")
        directory = str(pathlib.PurePosixPath(manifest).parent) + "/"
        changed = any(path.startswith(directory) for path in paths)
        changed |= closure(old_lock, name) != closure(new_lock, name)
        if changed:
            affected.add(name)
            application |= name in runtime_names and any(
                p.startswith(directory) and p != manifest and not p.endswith(".md") for p in paths)
        if manifest in paths and name in runtime_names:
            def production_manifest(document):
                return {**document, "dev-dependencies": {}, "target": {
                    target: {**table, "dev-dependencies": {}}
                    for target, table in document.get("target", {}).items()}}
            application |= production_manifest(old) != production_manifest(new)
        if manifest in paths and name not in runtime_names:
            # Features and shared dependency options can change without a lock delta.
            def shared_names(lock, runtime):
                packages = lock.get("package", [])
                return {p["name"] for root in packages if root["name"] == name
                        for dependency in root.get("dependencies", [])
                        for p in resolve(packages, dependency)
                        if (p["name"], p["version"], p.get("source", "")) in runtime}
            shared = shared_names(old_lock, old_runtime) | shared_names(new_lock, new_runtime)
            def shared_options(document):
                return {key: spec for key, spec in dependency_options(document, ("dependencies", "dev-dependencies", "build-dependencies")).items()
                        if canonical(key[2], spec) in shared}
            application |= shared_options(old) != shared_options(new)
            # A local feature matters to the application only if it enables a shared dependency.
            def shared_features(document):
                return {feature: [value for value in values if value.split("/")[0].removesuffix("?").removeprefix("dep:") in {
                    key[2] for key in shared_options(document)}]
                        for feature, values in document.get("features", {}).items()}
            application |= {k: v for k, v in shared_features(old).items() if v} != {
                k: v for k, v in shared_features(new).items() if v}
    for path in filter(None, paths):
        if path.endswith(".md") or path.startswith("docs/") or path == "Cargo.lock":
            continue
        if path.startswith((".github/workflows/", ".github/scripts/")):
            configuration = True
        elif any(path.startswith(str(pathlib.PurePosixPath(m).parent) + "/") for m in after):
            continue
        elif path.startswith("tools/icu-exemplars/"):
            continue  # Its separate reference workflow covers this standalone tool.
        else:
            application = True

print(f"application={str(bool(application)).lower()}")
print(f"configuration={str(configuration).lower()}")
print("packages=" + " ".join(sorted(affected - {""})))
PY
