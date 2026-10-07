import path from "node:path";

import { BUILD_TARGETS } from "./runtime-artifacts.mjs";

export const RELEASE_FINGERPRINT_SCHEMA = 1;

export const RELEASE_TARGET_MANIFESTS = {
  shell: "Cargo.toml",
  library: "crates/library-runtime/Cargo.toml",
  reader: "crates/reader-runtime/Cargo.toml",
  pdf: "crates/reader-runtime/Cargo.toml",
  reflow: "crates/reader-runtime/Cargo.toml",
};

export function cargoDependencyRoots(target, packages, root) {
  const manifest = RELEASE_TARGET_MANIFESTS[target];
  if (!Array.isArray(packages) || !manifest) return ["crates"];
  const wantedManifest = path.resolve(root, manifest);
  const entry = packages.find((pkg) => path.resolve(pkg.manifest_path) === wantedManifest);
  if (!entry) return ["crates"];

  const byName = new Map();
  for (const pkg of packages) {
    const members = byName.get(pkg.name) ?? [];
    members.push(pkg);
    byName.set(pkg.name, members);
  }
  const pending = [entry];
  const seen = new Map();
  while (pending.length) {
    const pkg = pending.pop();
    const id = pkg.id ?? pkg.manifest_path;
    if (seen.has(id)) continue;
    seen.set(id, pkg);
    for (const dependency of pkg.dependencies ?? []) {
      for (const name of new Set([dependency.name, dependency.rename].filter(Boolean))) {
        pending.push(...(byName.get(name) ?? []));
      }
    }
  }

  const roots = [];
  for (const pkg of seen.values()) {
    const directory = path.dirname(path.resolve(pkg.manifest_path));
    const relative = path.relative(root, directory);
    if (relative === "") {
      roots.push("src", "build.rs");
    } else if (relative !== ".." && !relative.startsWith(`..${path.sep}`) && !path.isAbsolute(relative)) {
      roots.push(relative);
    }
  }
  return roots.length ? roots : ["crates"];
}

export function hasReleaseTargetManifest(manifest) {
  return manifest?.profile === "release" &&
    manifest?.schema === RELEASE_FINGERPRINT_SCHEMA &&
    manifest?.targets && typeof manifest.targets === "object" &&
    !Array.isArray(manifest.targets);
}

export function staleReleaseTargets({ manifest, targetFingerprints, outputsPresent, force = false }) {
  if (force || !hasReleaseTargetManifest(manifest) ||
      !BUILD_TARGETS.every((target) => typeof targetFingerprints?.[target] === "string")) {
    return [...BUILD_TARGETS];
  }
  return BUILD_TARGETS.filter((target) =>
    manifest.targets[target] !== targetFingerprints[target] || outputsPresent?.[target] !== true,
  );
}

export function mergedReleaseTargets(previous, builtTargets, targetFingerprints) {
  const targets = hasReleaseTargetManifest(previous) ? { ...previous.targets } : {};
  for (const target of builtTargets) {
    if (BUILD_TARGETS.includes(target) && typeof targetFingerprints?.[target] === "string") {
      targets[target] = targetFingerprints[target];
    }
  }
  return targets;
}
