"""The sandbox cache (docs/verification.md): CI packs a build, a sandbox restores it.

tools/ci.py loads this module; run that, not this file.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import shutil
import subprocess
import time
import zipfile

from ci import (  # noqa: E402 - ci.py registers itself before importing this
    CACHE_ROOT,
    ENV_SH,
    SANDBOX_REPO,
    api,
    artifact,
    dispatch,
    download,
    follow,
    git,
    log,
    pushed_sha,
    token_from,
)


def cache_build(args) -> int:
    token = token_from(args)
    sha = pushed_sha(args.repo, token, args.rev)
    cold = "true" if args.cold else "false"
    full = "true" if args.full else "false"
    run = follow(args.repo, token,
                 dispatch(args.repo, token, "cache", sha, cold=cold, full=full),
                 args.timeout, args.poll)
    return 0 if run.conclusion == "success" else 1


CACHE_DIRS = ("rustup", "cargo", "target-mr")


LOCAL_META = os.path.join(CACHE_ROOT, "cache-meta.json")


# The cache job's env (remote.yml); cargo hashes it into every fingerprint.
CACHE_ENV = {
    "RUSTUP_HOME": f"{CACHE_ROOT}/rustup",
    "CARGO_HOME": f"{CACHE_ROOT}/cargo",
    "CARGO_TARGET_DIR": f"{CACHE_ROOT}/target-mr",
    "CARGO_INCREMENTAL": "1",
    "CARGO_PROFILE_DEV_DEBUG": "0",
    "CARGO_PROFILE_TEST_DEBUG": "0",
}


def cache_env(meta: dict | None) -> dict:
    return {**CACHE_ENV, **((meta or {}).get("env") or {})}


def write_env(meta: dict | None) -> str:
    """env.sh: what every shell must source before cargo touches the cache."""
    lines = ["# tools/ci.py cache restore wrote this; source it before any cargo command."]
    lines += [f'export {k}="{v}"' for k, v in sorted(cache_env(meta).items())]
    lines.append('export PATH="$CARGO_HOME/bin:$PATH"')
    os.makedirs(CACHE_ROOT, exist_ok=True)
    with open(ENV_SH, "w") as handle:
        handle.write("\n".join(lines) + "\n")
    return ENV_SH


def env_mismatch(meta: dict | None) -> list[str]:
    return [f"{k}={os.environ.get(k, '<unset>')} (cache wants {v})"
            for k, v in cache_env(meta).items() if os.environ.get(k) != v]


def local_cache() -> tuple[dict | None, list[str]]:
    """The restored cache's metadata, and which of its dirs survive here."""
    meta = None
    try:
        with open(LOCAL_META) as handle:
            meta = json.load(handle)
    except (OSError, ValueError):
        pass
    present = [d for d in CACHE_DIRS if os.path.isdir(os.path.join(CACHE_ROOT, d))]
    return meta, present


def behind(sha: str) -> int | None:
    """Commits from `sha` to HEAD, or None when `sha` is not an ancestor."""
    if not sha:
        return None
    if subprocess.run(["git", "cat-file", "-e", f"{sha}^{{commit}}"],
                      capture_output=True).returncode != 0:
        subprocess.run(["git", "fetch", "-q", "origin", sha], capture_output=True)
    if subprocess.run(["git", "merge-base", "--is-ancestor", sha, "HEAD"],
                      capture_output=True).returncode != 0:
        return None
    try:
        return int(git("rev-list", "--count", f"{sha}..HEAD"))
    except (subprocess.CalledProcessError, ValueError):
        return None


def cache_candidates(repo: str, token: str) -> list[dict]:
    """Live sandbox caches, each with the commit it was built for."""
    payload = api(token, f"/repos/{repo}/actions/artifacts?name=sandbox-cache&per_page=20")
    live = [a for a in (payload or {}).get("artifacts", []) if not a["expired"]]
    for art in live:
        run = art.get("workflow_run") or {}
        art["built_sha"] = run.get("head_sha", "")
        # A Remote run's title is "cache <sha> <id>"; the sha is what it built.
        info = api(token, f"/repos/{repo}/actions/runs/{run['id']}") if run.get("id") else None
        parts = (info or {}).get("display_title", "").split()
        if len(parts) >= 2 and re.fullmatch(r"[0-9a-f]{40}", parts[1]):
            art["built_sha"] = parts[1]
    return live


def newest_cache(repo: str, token: str) -> dict | None:
    """The live cache closest behind HEAD; else the newest one."""
    live = cache_candidates(repo, token)
    if not live:
        return None
    near = [(n, a) for a in live if (n := behind(a["built_sha"])) is not None]
    if near:
        n, art = min(near, key=lambda x: (x[0], x[1]["created_at"]))
        log(f"  closest cache: {art['built_sha'][:9]}, {n} commits behind HEAD")
        return art
    log("  no cache is an ancestor of HEAD: taking the newest")
    return max(live, key=lambda a: a["created_at"])


def ensure_zstd() -> None:
    if shutil.which("zstd"):
        return
    log("  installing zstd")
    for cmd in (["sudo", "-n", "apt-get", "install", "-y", "-q", "zstd"],
                ["sudo", "-n", "sh", "-c", "apt-get update -q && apt-get install -y -q zstd"]):
        if subprocess.run(cmd, capture_output=True).returncode == 0 and shutil.which("zstd"):
            return
    raise SystemExit("zstd is missing and could not be installed; install it and retry")


# Commits past which a restored cache recompiles most workspace crates.
STALE = 30


def cache_restore(args) -> int:
    token = token_from(args)
    if not token:
        raise SystemExit("downloading needs a token: --token-file or $GITHUB_TOKEN")
    started = time.time()
    local, present = local_cache()
    # Safe at every session start: a warm sandbox downloads nothing.
    if len(present) == len(CACHE_DIRS) and local and not args.force and not args.run:
        write_env(local)
        n = behind(local.get("sha", ""))
        log(f"already restored: cache at {local.get('sha', '?')[:9]}, "
            f"{'?' if n is None else n} commits behind HEAD; nothing downloaded")
        log(f"  next: source {ENV_SH}")
        if n is None or n > STALE:
            log("  stale here; if a newer cache exists, take it with: cache restore --force")
        return 0
    if present:
        log(f"  partial or unknown cache here ({', '.join(present)}): replacing it")
    if args.run:
        art = artifact(args.repo, token, args.run, "sandbox-cache")
    else:
        art = newest_cache(args.repo, token)
        if art is None:
            raise SystemExit("no live sandbox cache; make one: python3 tools/ci.py cache build")
    meta = fetch_cache(token, art, CACHE_ROOT, True)
    with open(LOCAL_META, "w") as handle:
        json.dump({k: v for k, v in meta.items() if k != "mtimes"}, handle, indent=2)
    log(f"  wrote {write_env(meta)}; next: source {ENV_SH}")
    if meta["repo_dir"] != git("rev-parse", "--show-toplevel"):
        log(f"  warning: the cache was built at {meta['repo_dir']}, this clone is "
            "elsewhere: every workspace crate rebuilds. Clone there instead.")
    log(f"restored in {time.time() - started:.0f}s; workspace crates edited since "
        f"{meta['sha'][:9]} recompile, dependencies do not")
    try:
        lag = int(git("rev-list", "--count", f"{meta['sha']}..HEAD"))
    except (subprocess.CalledProcessError, ValueError):
        lag = -1
    # Past this, most crates recompile: a 12-minute first lint here.
    if lag > STALE or lag < 0:
        log(f"  stale: {lag if lag >= 0 else '?'} commits behind HEAD. Lint in "
            "CI (about 1 min) rather than here; a green push refreshes the cache")
    return 0


def fetch_cache(token: str, art: dict, root: str, force: bool) -> dict:
    """Download a sandbox cache, unpack it, and redate the sources."""
    log(f"== sandbox cache {art['id']} from {art['created_at']}, "
        f"{art['size_in_bytes'] >> 20} MB")
    os.makedirs(os.path.join(root, "tmp"), exist_ok=True)
    zip_path = os.path.join(root, "tmp", "sandbox-cache.zip")
    need = art["size_in_bytes"] * 4
    if shutil.disk_usage(root).free < need:
        raise SystemExit(f"not enough disk: need ~{need >> 30} GB free under {root}")
    ensure_zstd()
    download(token, art, zip_path)
    with zipfile.ZipFile(zip_path) as zf:
        meta = json.loads(zf.read("meta.json"))
        root = meta["root"]
        present = [d for d in meta["dirs"] if os.path.exists(os.path.join(root, d))]
        if present and not force:
            raise SystemExit(f"{', '.join(present)} already exist under {root}: this "
                             "machine is warm already; --force replaces them")
        for d in present:
            shutil.rmtree(os.path.join(root, d))
        log(f"  unpacking into {root} ({meta['rustc']})")
        tar = subprocess.Popen(["tar", "--zstd", "-x", "-C", root, "-f", "-"],
                               stdin=subprocess.PIPE)
        with zf.open("sandbox-cache.tar.zst") as member:
            shutil.copyfileobj(member, tar.stdin, 1 << 20)
        tar.stdin.close()
        if tar.wait() != 0:
            raise SystemExit("tar failed; the cache is incomplete, delete it and retry")
    os.unlink(zip_path)
    log(f"  {redate(meta)} unchanged source files dated as at {meta['sha'][:9]}")
    return meta


def cache_seed(args) -> int:
    """In the cache job: start from the newest cache, not from nothing.
    Never fails the job; a cold build is the fallback."""
    token = token_from(args)
    try:
        art = newest_cache(args.repo, token)
        if art is None:
            log("no live sandbox cache: building cold")
            return 0
        meta = fetch_cache(token, art, "/home/user/.cache", force=True)
        # Nothing deletes a stale artifact: a lock or build-set change
        # would carry the old files along, so target-mr starts afresh.
        lock = hashlib.sha256(open("Cargo.lock", "rb").read()).hexdigest()
        builds = os.environ.get("CACHE_BUILDS", "lint")
        if meta.get("lock") != lock or meta.get("builds") != builds:
            log(f"Cargo.lock or the build set ({builds}) changed since that "
                "cache: target-mr builds from nothing; toolchain and registry stay")
            shutil.rmtree("/home/user/.cache/target-mr", ignore_errors=True)
    except (SystemExit, OSError, zipfile.BadZipFile) as exc:
        log(f"seeding failed ({exc}): building cold")
        for d in ("rustup", "cargo", "target-mr"):
            shutil.rmtree(os.path.join("/home/user/.cache", d), ignore_errors=True)
    return 0


def cache_status(args) -> int:
    """What a reset may have wiped, and the one next step. 0 ready, 1 not."""
    local, present = local_cache()
    log(f"== sandbox cache under {CACHE_ROOT}")
    ready = True
    if not present:
        log("  none here: a fresh or wiped sandbox")
        ready = False
    elif len(present) < len(CACHE_DIRS):
        missing = [d for d in CACHE_DIRS if d not in present]
        log(f"  partial: {', '.join(missing)} missing (wiped?)")
        ready = False
    n = behind((local or {}).get("sha", "")) if local else None
    if local:
        log(f"  restored cache: {local.get('sha', '?')[:9]} ({local.get('builds', '?')} builds, "
            f"{local.get('rustc', '?')}), {'?' if n is None else n} commits behind HEAD")
    wrong = env_mismatch(local)
    if wrong:
        log(f"  this shell lacks the cache's env: {'; '.join(wrong[:3])}"
            f"{' ...' if len(wrong) > 3 else ''}")
    try:
        top = git("rev-parse", "--show-toplevel")
    except (OSError, subprocess.CalledProcessError):
        top = os.getcwd()
    if top != SANDBOX_REPO:
        log(f"  this clone is at {top}; the cache was built at {SANDBOX_REPO}")
    root = CACHE_ROOT if os.path.isdir(CACHE_ROOT) else os.path.dirname(CACHE_ROOT)
    if os.path.isdir(root):
        log(f"  free disk: {shutil.disk_usage(root).free / (1 << 30):.1f} GiB")
    if not ready:
        log("NEXT: python3 tools/ci.py cache restore, then source " + ENV_SH)
        return 1
    if wrong:
        log(f"NEXT: source {ENV_SH} (or eval \"$(python3 tools/ci.py cache env)\")")
        return 1
    if n is None or n > STALE:
        log("NEXT: stale; lint in CI, or cache restore --force once a newer cache exists")
        return 1
    log("READY: scoped local clippy is fine, e.g. timeout 900 cargo clippy -p <crate> "
        "-j 2 --all-targets -- -D warnings")
    return 0


def print_env(args) -> int:
    local, _ = local_cache()
    for k, v in sorted(cache_env(local).items()):
        print(f'export {k}="{v}"')
    print('export PATH="$CARGO_HOME/bin:$PATH"')
    return 0


def redate(meta: dict) -> int:
    """Give unchanged sources the times CI built them with, so cargo
    rebuilds only what differs from the cached commit."""
    sha = meta["sha"]
    if subprocess.run(["git", "cat-file", "-e", f"{sha}^{{commit}}"],
                      capture_output=True).returncode != 0:
        subprocess.run(["git", "fetch", "-q", "origin", sha], capture_output=True)
    try:
        changed = set(git("diff", "--name-only", sha).splitlines())
    except subprocess.CalledProcessError:
        log("  cached commit unknown here: every crate recompiles")
        return 0
    # Per-file times since seeding; older caches dated all to the commit.
    times = meta.get("mtimes", {})
    count = 0
    for path in git("ls-files").splitlines():
        if path not in changed and os.path.isfile(path) and not os.path.islink(path):
            t = times.get(path, meta["time"])
            os.utime(path, (t, t))
            count += 1
    return count
