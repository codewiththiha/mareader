#!/usr/bin/env python3
"""CI mode (docs/verification.md): let GitHub Actions build, test and render.

  python3 tools/ci.py mode                 # local, cache or ci for this machine, and why
  python3 tools/ci.py watch                # the quick gate on the branch head, to a verdict
  python3 tools/ci.py final                # the full pool on the branch head; waits for all
  python3 tools/ci.py watch --once         # one snapshot, no waiting
  python3 tools/ci.py watch --detach       # in the background; --tail, --stop
  python3 tools/ci.py shots "grid texture=grid gap=false" "dark theme=dark"
  python3 tools/ci.py web                  # the browser preview, built + screenshot
  python3 tools/ci.py memory --ref feat/x  # docs/memory.md's workloads and budgets
  python3 tools/ci.py kokoro --ref feat/x  # the Kokoro port against its reference
  python3 tools/ci.py piper --ref feat/x   # the Piper voices against onnxruntime
  python3 tools/ci.py kitten --ref feat/x  # the Kitten builds against onnxruntime
  python3 tools/ci.py phonon --ref feat/x  # the Phonon port against ptts
  python3 tools/ci.py ipa --ref feat/x     # our other languages against espeak
  python3 tools/ci.py hanzi --ref feat/x   # our Chinese reader against misaki
  python3 tools/ci.py system --ref feat/x  # system voices on macOS and Windows
  python3 tools/ci.py installers --ref ci/x # every platform's installers, checked
  python3 tools/ci.py cache build          # refresh the sandbox cache for HEAD
                                           # (CI does this after every green push)
  python3 tools/ci.py cache restore        # unpack the closest one here (idempotent)
  python3 tools/ci.py cache status         # is this sandbox warm, stale or wiped?
  eval "$(python3 tools/ci.py cache env)"  # the cache's cargo env, in this shell

Exit code: 0 green, 1 red, 2 not finished yet.

`watch` reads the `CI` workflow; a red job is the whole answer, so the wait
stops there and digests the failed job's log while its siblings still run. A
run a newer push cancelled is superseded, not red.

`shots`, `web` and `cache` dispatch the `Remote` workflow on a commit GitHub
already has (push its <type>/<slug> branch first), wait, and download what it made:
shots and web land in target/remote/<task>/, ready for read_file.

Reads of this public repository need no token; dispatches and downloads do.
A token comes from --token-file, $GITHUB_TOKEN or $GH_TOKEN and is never
written anywhere. Standard library only.
"""

from __future__ import annotations

import argparse
import glob
import hashlib
import io
import json
import os
import re
import secrets
import shutil
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
import zipfile
from dataclasses import dataclass, field

API = "https://api.github.com"
REPO_DEFAULT = "codewiththiha/mareader2"
GATE = "CI"
# ci.yml sets `run-name`, so a run's "name" is its display title;
# the workflow it belongs to is the only stable handle.
GATE_WORKFLOW = "ci.yml"
REMOTE = "remote.yml"
INSTALLERS = "installers.yml"
CACHE_ROOT = "/home/user/.cache"
SANDBOX_REPO = "/home/user/mareader2"
ENV_SH = os.path.join(CACHE_ROOT, "env.sh")
# Below either, a cold build takes half an hour; CI takes minutes.
LOCAL_MIN_GIB = 8
LOCAL_MIN_CORES = 4


def log(msg: str) -> None:
    print(msg, flush=True)


class _HopAuth(urllib.request.HTTPRedirectHandler):
    """Blob storage refuses a signed URL that also carries `Authorization`,
    and urllib forwards the header across redirects; drop it there."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):  # noqa: D102
        new = super().redirect_request(req, fp, code, msg, headers, newurl)
        if new is not None and "sig=" in urllib.parse.urlparse(newurl).query:
            if new.has_header("Authorization"):
                new.remove_header("Authorization")
        return new


_OPENER = urllib.request.build_opener(_HopAuth())


def request(token: str, url: str, data: dict | None = None) -> urllib.request.Request:
    url = url if url.startswith("http") else f"{API}{url}"
    body = json.dumps(data).encode() if data is not None else None
    req = urllib.request.Request(url, data=body, method="POST" if body else "GET")
    if token:
        req.add_header("Authorization", f"Bearer {token}")
    req.add_header("Accept", "application/vnd.github+json")
    req.add_header("X-GitHub-Api-Version", "2022-11-28")
    req.add_header("User-Agent", "mareader2-ci")
    return req


def api(token: str, path: str, data: dict | None = None, *, raw: bool = False):
    """JSON from the API; None on 404 or 422 (an unknown commit)."""
    for attempt in range(4):
        try:
            with _OPENER.open(request(token, path, data), timeout=120) as resp:
                body = resp.read()
                if raw:
                    return body
                return json.loads(body) if body else {}
        except urllib.error.HTTPError as exc:
            text = exc.read().decode("utf-8", "replace")
            if exc.code in (404, 422) and data is None:
                return None
            wait = exc.headers.get("Retry-After")
            if wait and exc.code in (403, 429) and attempt < 3:
                log(f"  rate limited ({exc.code}); waiting {wait}s")
                time.sleep(int(wait))
                continue
            hint = ""
            if exc.code in (401, 403, 404) and not token:
                hint = " (no token: pass --token-file or set $GITHUB_TOKEN)"
            raise SystemExit(f"{path}: HTTP {exc.code}{hint}: {text[:300].strip()}")
        except (urllib.error.URLError, TimeoutError) as exc:
            if attempt < 3:
                time.sleep(3 * (attempt + 1))
                continue
            raise SystemExit(f"{path}: network error: {exc}") from exc
    raise SystemExit(f"{path}: gave up after retries")


@dataclass
class Job:
    name: str
    status: str
    conclusion: str | None
    id: int
    failed_steps: list[str] = field(default_factory=list)


# CI's jobs that run after the verdict: the sandbox cache refresh.
AFTER = "Cache /"


@dataclass
class Run:
    name: str
    status: str
    conclusion: str | None
    id: int
    sha: str
    url: str
    created: str
    title: str = ""
    jobs: list[Job] = field(default_factory=list)

    @property
    def gating(self) -> list[Job]:
        return [j for j in self.jobs if not j.name.startswith(AFTER)]

    @property
    def finished(self) -> bool:
        """Done, or every job the verdict reads is."""
        gate = self.gating
        return self.status == "completed" or (
            bool(gate) and all(j.status == "completed" for j in gate))

    @staticmethod
    def of(item: dict) -> "Run":
        return Run(item["name"], item["status"], item.get("conclusion"), item["id"],
                   item["head_sha"], item["html_url"], item["created_at"],
                   item.get("display_title", ""))


def jobs_for(repo: str, token: str, run_id: int) -> list[Job]:
    payload = api(token, f"/repos/{repo}/actions/runs/{run_id}/jobs?per_page=100")
    return [Job(j["name"], j["status"], j.get("conclusion"), j["id"],
                [st["name"] for st in j.get("steps") or []
                 if st.get("conclusion") == "failure"])
            for j in (payload or {}).get("jobs", [])]


def refresh(repo: str, token: str, run: Run) -> Run:
    fresh = Run.of(api(token, f"/repos/{repo}/actions/runs/{run.id}"))
    fresh.jobs = jobs_for(repo, token, run.id)
    return fresh


# A cancelled run is a newer push's doing, not this commit's verdict.
SUPERSEDED = ("cancelled", "startup_cancelled")


def is_red(conclusion: str | None) -> bool:
    return conclusion not in (None, "success", "skipped", "neutral", *SUPERSEDED)


ANSI = re.compile(r"\x1b\[[0-9;]*m")
STAMP = re.compile(r"^\d{4}-\d{2}-\d{2}T[\d:.]+Z ?")
STEP_FAIL = re.compile(r"##\[error\]|error(\[|:)|FAILED|panicked|Diff in|^---- .* stdout ----$")
NOISE = re.compile(r"^\s*(Download|Extract|Cache|Receiving|Resolving|Updating files"
                   r"|##\[(end)?group\]|##\[(start|end)-action)")
# A rustfmt diff needs its hunk, which follows the marker.
DIFF_CONTEXT = 14


def digest(text: str) -> str:
    """Error lines, rustfmt hunks kept whole, then the log's tail."""
    lines = [STAMP.sub("", ln) for ln in ANSI.sub("", text).splitlines()]
    out: list[str] = []
    marks = [i for i, ln in enumerate(lines) if STEP_FAIL.search(ln)]
    if marks:
        out.append("--- error lines ---")
        shown: set[int] = set()
        for i in marks[:80]:
            span = range(i, i + DIFF_CONTEXT) if "Diff in" in lines[i] else (i,)
            for j in span:
                if j < len(lines) and j not in shown:
                    shown.add(j)
                    out.append(lines[j])
    # Check runs past failures, so one log may hold several failing steps.
    errors = [i for i, ln in enumerate(lines) if ln.startswith("##[error]")]
    runs = [i for i, ln in enumerate(lines) if ln.startswith("##[group]Run ")]
    spans: list[tuple[int, int]] = []
    for e in errors:
        start = max((r for r in runs if r < e), default=max(0, e - 250))
        if spans and spans[-1][0] == start:
            continue
        spans.append((start, e + 1))
    for start, end in spans[:4]:
        head = lines[start].removeprefix("##[group]Run ")[:90]
        out.append(f"--- failing step: {head} ---")
        out.extend([ln for ln in lines[start:end] if not NOISE.match(ln)][-60:])
    if not spans:
        out.append("--- tail ---")
        out.extend([ln for ln in lines[-250:] if not NOISE.match(ln)][-80:])
    return "\n".join(out)


def failure_digest(repo: str, token: str, run: Run) -> str:
    """The failed jobs' logs, which a job serves even mid-run."""
    parts = []
    for job in run.gating:
        if not is_red(job.conclusion):
            continue
        try:
            raw = api(token, f"/repos/{repo}/actions/jobs/{job.id}/logs", raw=True)
            text = digest(raw.decode("utf-8", "replace")) if raw else "(no log)"
        except SystemExit as exc:
            text = f"(log unavailable: {exc})"
        parts.append(f"\n===== {job.name} ({job.conclusion}) =====\n{text}")
    return "\n".join(parts) or "(no failed job; see the run page)"


def summarize(run: Run) -> str:
    bad = [f"{j.name} ({j.conclusion})" for j in run.gating if is_red(j.conclusion)]
    state = run.conclusion or ("done" if run.finished else run.status)
    return f"{run.name}: {state}" + (" — failed: " + ", ".join(bad) if bad else "")


def follow(repo: str, token: str, run: Run, timeout: int, poll: int,
           whole: bool = False) -> Run:
    """Wait for one run; a red job ends the wait with its digest.

    With `whole`, siblings of a red job still run to the end.
    """
    deadline = time.time() + timeout
    noted = ""
    seen_steps: set[str] = set()
    while True:
        run = refresh(repo, token, run)
        # A Remote run is named by its title: "task sha id".
        task = run.name.split()[0]
        if task in ("shots", "web", "memory", "cache", "kokoro", "piper", "kitten", "ipa",
                    "hanzi", "system", "phonon"):
            run.name = f"Remote {task}"
        running = [j.name for j in run.jobs if j.status != "completed"]
        state = f"{run.status} {', '.join(running)}"
        if state != noted:
            noted = state
            log(f"  {run.name} {run.status}" + (f": {', '.join(running)}" if running else ""))
        for job in run.gating:
            for step in job.failed_steps:
                if f"{job.name}/{step}" not in seen_steps:
                    seen_steps.add(f"{job.name}/{step}")
                    tail_note = "" if job.status == "completed" else " (the job goes on)"
                    log(f"  step failed: {job.name} > {step}{tail_note}")
        red = any(is_red(j.conclusion) for j in run.gating)
        if (red and not whole) or run.finished:
            if whole:
                for job in run.jobs:
                    log(f"    {job.conclusion or job.status:<10} {job.name}")
            log("  " + summarize(run))
            if any(is_red(j.conclusion) for j in run.gating):
                log(f"\n########## failure digest ({run.url}) ##########")
                log(failure_digest(repo, token, run))
                log("########## end digest ##########\n")
            return run
        if time.time() > deadline:
            log(f"  timed out after {timeout}s: {run.url}")
            return run
        time.sleep(poll)


def verdict(sha: str, run: Run | None) -> int:
    if run is None:
        log(f"IN PROGRESS for {sha[:9]} — no {GATE} run has appeared yet")
        return 2
    # A run with no jobs failed to start, e.g. an invalid workflow file.
    if any(is_red(j.conclusion) for j in run.gating) or (
            is_red(run.conclusion) and not run.gating):
        log(f"VERDICT RED for {sha[:9]} — {run.url}")
        return 1
    if not run.finished:
        log(f"IN PROGRESS for {sha[:9]} — {run.url}")
        return 2
    if run.conclusion in SUPERSEDED or any(j.conclusion in SUPERSEDED
                                           for j in run.gating):
        log(f"VERDICT SUPERSEDED for {sha[:9]} — a newer push cancelled it; "
            "watch the branch head instead")
        return 2
    log(f"VERDICT GREEN for {sha[:9]}")
    if any(j.name.startswith(AFTER) for j in run.jobs) or run.status != "completed":
        log("  the sandbox cache now refreshes for this commit (Remote cache)")
    return 0


def git(*args: str) -> str:
    return subprocess.run(["git", *args], capture_output=True, text=True,
                          check=True).stdout.strip()


def git_dir() -> str:
    try:
        return git("rev-parse", "--absolute-git-dir")
    except (OSError, subprocess.CalledProcessError):
        return "."


def state_path(name: str) -> str:
    return os.path.join(git_dir(), f"ci-watch.{name}")


def gate_run(repo: str, token: str, sha: str) -> Run | None:
    """The newest CI run on this commit; older ones are context."""
    payload = api(token, f"/repos/{repo}/actions/workflows/{GATE_WORKFLOW}"
                         f"/runs?head_sha={sha}&per_page=50")
    runs = [Run.of(r) for r in (payload or {}).get("workflow_runs", [])]
    return max(runs, key=lambda r: r.created) if runs else None


def watch(args) -> int:
    token = token_from(args)
    sha = full_sha(args.sha) if args.sha else resolve(args.repo, token, args.branch)
    log(f"== watching {args.repo}@{sha[:12]} for {GATE}")
    if args.once:
        run = gate_run(args.repo, token, sha)
        if run:
            run.jobs = jobs_for(args.repo, token, run.id)
            log("  " + summarize(run))
            if any(is_red(j.conclusion) for j in run.gating):
                log(failure_digest(args.repo, token, run))
        code = verdict(sha, run)
    else:
        deadline = time.time() + 300
        run = gate_run(args.repo, token, sha)
        while run is None and time.time() < deadline:
            log(f"  waiting for a {GATE} run on {sha[:9]} ...")
            time.sleep(10)
            run = gate_run(args.repo, token, sha)
        if run is not None:
            run = follow(args.repo, token, run, args.timeout, args.poll)
        code = verdict(sha, run)
    if args.json:
        print(json.dumps({"sha": sha, "code": code,
                          "url": run.url if run else None,
                          "failed": [j.name for j in run.gating if is_red(j.conclusion)]
                          if run else []}))
    return code


def final(args) -> int:
    """The full pool on the branch head, dispatched; waits for every job."""
    token = token_from(args)
    branch = args.branch
    if not branch:
        try:
            branch = git("rev-parse", "--abbrev-ref", "HEAD")
        except (OSError, subprocess.CalledProcessError):
            branch = "HEAD"
    if branch in ("HEAD", ""):
        raise SystemExit("final needs a branch: pass --branch <type>/<slug>")
    sha = resolve(args.repo, token, branch)
    try:
        if git("rev-parse", "HEAD") != sha and not args.branch:
            log(f"  note: local HEAD differs from {branch} on GitHub; final checks "
                f"GitHub's {sha[:9]}. Push first if that is not what you mean.")
    except (OSError, subprocess.CalledProcessError):
        pass
    run = dispatch(args.repo, token, "", sha, branch, GATE_WORKFLOW, profile="full")
    run = follow(args.repo, token, run, args.timeout, args.poll, whole=True)
    return verdict(sha, run)


def full_sha(rev: str) -> str:
    """The API matches whole commit ids only; expand a short one."""
    try:
        return git("rev-parse", f"{rev}^{{commit}}")
    except (OSError, subprocess.CalledProcessError):
        return rev


def resolve(repo: str, token: str, branch: str | None) -> str:
    """The branch head on GitHub: what CI actually ran."""
    if not branch:
        try:
            branch = git("rev-parse", "--abbrev-ref", "HEAD")
        except (OSError, subprocess.CalledProcessError):
            branch = "main"
        branch = "main" if branch == "HEAD" else branch
    info = api(token, f"/repos/{repo}/commits/{urllib.parse.quote(branch, safe='')}")
    if not info:
        raise SystemExit(f"{branch} is not on GitHub; push it first")
    return info["sha"]


def tail(lines: int) -> int:
    path = state_path("log")
    if not os.path.exists(path):
        raise SystemExit(f"no watcher log at {path}; start one with --detach")
    text = open(path, errors="replace").read().splitlines()
    for line in text[-lines:]:
        print(line)
    starts = [i for i, ln in enumerate(text) if ln.startswith("== watching")]
    for line in reversed(text[starts[-1] if starts else 0:]):
        if line.startswith("VERDICT GREEN"):
            return 0
        if line.startswith("VERDICT RED"):
            return 1
    return 2


def stop() -> int:
    path = state_path("pid")
    if not os.path.exists(path):
        print("no detached watcher is running")
        return 0
    pid = int(open(path).read().strip())
    try:
        os.kill(pid, signal.SIGTERM)
        print(f"stopped watcher {pid}")
    except ProcessLookupError:
        print(f"watcher {pid} had already exited")
    os.unlink(path)
    return 0


def detach(args) -> int:
    pid = os.fork()
    if pid > 0:
        with open(state_path("pid"), "w") as handle:
            handle.write(str(pid))
        print(f"watching in the background: pid {pid}, log {state_path('log')}")
        print("  read it with: python3 tools/ci.py watch --tail")
        return 0
    os.setsid()
    fd = os.open(state_path("log"), os.O_WRONLY | os.O_CREAT | os.O_APPEND)
    os.dup2(fd, 1)
    os.dup2(fd, 2)
    try:
        code = watch(args)
    except BaseException as exc:  # noqa: BLE001 - the log is the only output
        print(f"watcher failed: {type(exc).__name__}: {exc}")
        code = 1
    print(f"watcher exited with {code}")
    sys.stdout.flush()
    try:
        os.unlink(state_path("pid"))
    except OSError:
        pass
    os._exit(code)


def pushed_sha(repo: str, token: str, rev: str) -> str:
    """A local commit that GitHub already has."""
    try:
        sha = git("rev-parse", f"{rev}^{{commit}}")
    except subprocess.CalledProcessError as exc:
        raise SystemExit(f"unknown revision {rev}") from exc
    if not api(token, f"/repos/{repo}/commits/{sha}"):
        raise SystemExit(f"{sha[:9]} is not on GitHub: push it first, e.g. "
                         "git push origin HEAD:refs/heads/<type>/<slug>")
    if rev == "HEAD" and git("status", "--porcelain", "--untracked-files=no"):
        log("  note: uncommitted changes are not part of this run")
    return sha


def dispatch(repo: str, token: str, task: str, sha: str, ref: str = "main",
             workflow: str = REMOTE, **inputs: str) -> Run:
    """Start a run of `ref`'s `workflow`, found by its title's id.

    Remote runs take a `task`; other workflows get none (task "").
    """
    if not token:
        raise SystemExit("dispatching needs a token: --token-file or $GITHUB_TOKEN")
    rid = secrets.token_hex(4)
    fields = {"sha": sha, "id": rid, **inputs}
    if task:
        fields["task"] = task
    api(token, f"/repos/{repo}/actions/workflows/{workflow}/dispatches",
        {"ref": ref, "inputs": fields})
    log(f"== dispatched {task or workflow} on {sha[:9]} (id {rid})")
    for _ in range(40):
        time.sleep(3)
        payload = api(token, f"/repos/{repo}/actions/workflows/{workflow}/runs"
                             "?event=workflow_dispatch&per_page=20")
        for item in (payload or {}).get("workflow_runs", []):
            if item.get("display_title", "").endswith(rid):
                log(f"  {item['html_url']}")
                return Run.of(item)
    raise SystemExit("the dispatched run never appeared; check the Actions page")


def artifact(repo: str, token: str, run_id: int, name: str) -> dict:
    payload = api(token, f"/repos/{repo}/actions/runs/{run_id}/artifacts")
    for item in (payload or {}).get("artifacts", []):
        if item["name"] == name and not item["expired"]:
            return item
    raise SystemExit(f"run {run_id} has no live artifact named {name}")


def download(token: str, item: dict, dest: str) -> str:
    """Stream an artifact's zip to disk, reporting progress."""
    os.makedirs(os.path.dirname(dest), exist_ok=True)
    size = item["size_in_bytes"]
    started, done, next_note = time.time(), 0, 0
    with _OPENER.open(request(token, item["archive_download_url"]), timeout=300) as resp, \
            open(dest, "wb") as out:
        while chunk := resp.read(1 << 20):
            out.write(chunk)
            done += len(chunk)
            if done >= next_note:
                log(f"  {done >> 20} / {size >> 20} MB")
                next_note += 200 << 20
    secs = time.time() - started
    log(f"  downloaded {done >> 20} MB in {secs:.0f}s ({done / max(secs, 0.1) / 1e6:.1f} MB/s)")
    return dest


def remote_task(args, task: str, name: str, **inputs: str) -> tuple[int, str]:
    """Dispatch, follow, and unpack a small artifact into the out dir."""
    token = token_from(args)
    sha = pushed_sha(args.repo, token, args.rev)
    run = dispatch(args.repo, token, task, sha, getattr(args, "ref", "main"), **inputs)
    # A failed platform still leaves the report job to wait for.
    whole = task in ("system", "memory")
    run = follow(args.repo, token, run, args.timeout, args.poll, whole=whole)
    # A broken budget still leaves its tables to read.
    if run.conclusion != "success" and task not in ("memory", "kokoro", "piper", "kitten",
                                                    "ipa", "hanzi", "system", "phonon"):
        return 1, ""
    item = artifact(args.repo, token, run.id, name)
    raw = api(token, item["archive_download_url"], raw=True)
    out = os.path.abspath(args.out or os.path.join("target", "remote", task))
    shutil.rmtree(out, ignore_errors=True)
    zipfile.ZipFile(io.BytesIO(raw)).extractall(out)
    return 0, out


def shots(args) -> int:
    specs = [s.strip() for s in args.spec if s.strip()]
    for spec in specs:
        if not re.fullmatch(r"[A-Za-z0-9._-]+", spec.split()[0]):
            raise SystemExit(f"shot name must be a plain file name: {spec.split()[0]}")
    if not specs:
        raise SystemExit('give at least one shot: "NAME key=value ..."')
    code, out = remote_task(args, "shots", "shots", shots="\n".join(specs))
    if code == 0:
        for path in sorted(glob.glob(os.path.join(out, "*.png"))):
            log(f"  {path}")
    return code


def web(args) -> int:
    code, out = remote_task(args, "web", "web")
    if code == 0:
        preview = os.path.join(out, "preview.png")
        log(f"  dist: {out}")
        log(f"  screenshot: {preview if os.path.exists(preview) else 'none (best effort)'}")
        log(f"  serve it: python3 -m http.server -d {out} 8000")
    return code


def mem_gib() -> float:
    """RAM, or the cgroup's limit when that is lower."""
    total = 0.0
    try:
        for line in open("/proc/meminfo"):
            if line.startswith("MemTotal:"):
                total = int(line.split()[1]) / (1 << 20)
    except OSError:
        try:
            out = subprocess.run(["sysctl", "-n", "hw.memsize"], capture_output=True,
                                 text=True).stdout.strip()
            total = int(out) / (1 << 30) if out else 0.0
        except OSError:
            pass
    try:
        limit = open("/sys/fs/cgroup/memory.max").read().strip()
        if limit.isdigit():
            total = min(total, int(limit) / (1 << 30)) if total else int(limit) / (1 << 30)
    except OSError:
        pass
    return total


def facts() -> dict:
    cores = len(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") \
        else (os.cpu_count() or 1)
    # target/ may hold only downloaded shots; the sandbox builds elsewhere.
    targets = [os.environ.get("CARGO_TARGET_DIR") or "", "target",
               "/home/user/.cache/target-mr"]
    warm = any(glob.glob(os.path.join(t, "debug", ".fingerprint", "mareader-ui-egui-*"))
               for t in targets if t)
    cargo = bool(shutil.which("cargo")) or os.path.exists("/home/user/.cache/env.sh")
    free = shutil.disk_usage(".").free / (1 << 30)
    root = CACHE_ROOT
    while root and not os.path.isdir(root):
        root = os.path.dirname(root)
    cache_free = shutil.disk_usage(root or "/").free / (1 << 30)
    try:
        top = git("rev-parse", "--show-toplevel")
    except (OSError, subprocess.CalledProcessError):
        top = os.getcwd()
    return {"cores": cores, "ram_gib": round(mem_gib(), 1), "warm": warm,
            "toolchain": cargo, "disk_free_gib": round(free, 1),
            "cache_disk_free_gib": round(cache_free, 1),
            "linux_x86_64": sys.platform.startswith("linux") and os.uname().machine == "x86_64",
            "token": bool(os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN")),
            "repo_dir": top}


# Restore needs about 4x the 1.2 GB archive while unpacking.
CACHE_MIN_FREE_GIB = 6
NEXT = {
    "local": [
        "run the AGENTS.md gates here, scoped with -p <crate> -j N and a timeout",
        "push; do not poll the quick gate, it repeats what passed",
        "before merging: python3 tools/ci.py final (macOS, Windows, wasm, each commit)",
    ],
    "cache": [
        "python3 tools/ci.py cache status   # every session start and after any reset",
        "python3 tools/ci.py cache restore  # idempotent; downloads only when wiped",
        f"source {ENV_SH}                    # in every new shell, before cargo",
        "timeout 900 cargo clippy -p <crate> -j 2 --all-targets -- -D warnings",
        "push; python3 tools/ci.py watch    # tests and runtime errors come from CI",
        "before merging: python3 tools/ci.py final",
    ],
    "ci": [
        "compile nothing here; run comment_check, commit_check, cargo fmt --check",
        "push a <type>/<slug> branch; python3 tools/ci.py watch (quick gate, one job)",
        "before merging: python3 tools/ci.py final (the full pool, waits for all)",
    ],
}


def mode(args) -> int:
    f = facts()
    forced = os.environ.get("MAREADER_VERIFY", "").strip().lower()
    reasons = []
    if forced in ("local", "cache", "ci"):
        choice = forced
        reasons.append(f"MAREADER_VERIFY={forced} overrides detection")
    else:
        capable = f["ram_gib"] >= LOCAL_MIN_GIB and f["cores"] >= LOCAL_MIN_CORES
        reasons.append(f"{f['ram_gib']} GiB RAM, {f['cores']} cores, "
                       f"{f['disk_free_gib']} GiB free (local wants >= "
                       f"{LOCAL_MIN_GIB} GiB, >= {LOCAL_MIN_CORES} cores, >= 10 GiB)")
        blockers = []
        if not f["linux_x86_64"]:
            blockers.append("not x86_64 Linux")
        if not f["token"]:
            blockers.append("no $GITHUB_TOKEN to download it")
        if f["cache_disk_free_gib"] < CACHE_MIN_FREE_GIB:
            blockers.append(f"{f['cache_disk_free_gib']} GiB free under {CACHE_ROOT}, "
                            f"wants {CACHE_MIN_FREE_GIB}")
        if capable and f["disk_free_gib"] >= 10:
            choice = "local"
        elif not blockers:
            choice = "cache"
            reasons.append("low-end, but CI's sandbox cache fits: lint here, test in CI")
            if f["repo_dir"] != SANDBOX_REPO:
                reasons.append(f"clone at {SANDBOX_REPO} first, or every crate rebuilds")
        else:
            choice = "ci"
            reasons.append("cache mode is out: " + "; ".join(blockers))
    if args.json:
        print(json.dumps({"mode": choice, "facts": f, "reasons": reasons,
                          "next": NEXT[choice]}))
    else:
        print(f"mode: {choice}")
        for r in reasons:
            print(f"  - {r}")
        print("next:")
        for step in NEXT[choice]:
            print(f"  {step}")
        print("  an explicit request from the user overrides this (docs/verification.md)")
    return 0


def token_from(args) -> str:
    if getattr(args, "token_file", None):
        path = os.path.expanduser(args.token_file)
        try:
            return open(path, encoding="utf-8").read().strip()
        except OSError as exc:
            raise SystemExit(f"--token-file {path}: {exc.strerror}") from exc
    return os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN") or ""


def memory(args) -> int:
    code, out = remote_task(args, "memory", "memory")
    report = os.path.join(out, "memory.md")
    if os.path.exists(report):
        log(f"  tables and budgets: {report}")
        held = "**no**" not in open(report).read()
        return code if held else 1
    return 1


def kokoro(args, task: str = "kokoro") -> int:
    code, out = remote_task(args, task, task)
    report = os.path.join(out, "summary.md")
    if not os.path.exists(report):
        return 1
    log(f"  stages, audio and speed: {report}")
    log(open(report, encoding="utf-8").read())
    return code if "**All checks pass.**" in open(report).read() else 1


def installers(args) -> int:
    """Every platform's installers and their checks, from installers.yml."""
    token = token_from(args)
    sha = pushed_sha(args.repo, token, args.rev)
    run = dispatch(args.repo, token, "", sha, args.ref, INSTALLERS)
    # Each platform's report is worth reading, whichever failed.
    run = follow(args.repo, token, run, args.timeout, args.poll, whole=True)
    out = os.path.abspath(args.out or os.path.join("target", "remote", "installers"))
    shutil.rmtree(out, ignore_errors=True)
    payload = api(token, f"/repos/{args.repo}/actions/runs/{run.id}/artifacts?per_page=50")
    # A failed check still leaves its report to read.
    for item in (payload or {}).get("artifacts", []):
        name = item["name"]
        wanted = name.startswith("checks-") or (args.download and
                                                name.startswith("installers-"))
        if not wanted or item["expired"]:
            continue
        log(f"  {name}: {item['size_in_bytes'] >> 20} MB")
        raw = api(token, item["archive_download_url"], raw=True)
        zipfile.ZipFile(io.BytesIO(raw)).extractall(os.path.join(out, name))
    for path in sorted(glob.glob(os.path.join(out, "checks-*", "checks.txt"))):
        log(f"-- {os.path.basename(os.path.dirname(path))}")
        log(open(path, encoding="utf-8").read().rstrip())
    log(f"  reports: {out}")
    return 0 if run.conclusion == "success" else 1


def cache_module():
    """tools/ci_cache.py, bound to this module rather than a second copy."""
    sys.modules.setdefault("ci", sys.modules[__name__])
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import ci_cache
    return ci_cache


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--repo", default=REPO_DEFAULT)
    ap.add_argument("--token-file", help="read the API token from this file")
    sub = ap.add_subparsers(dest="cmd", required=True)

    m = sub.add_parser("mode", help="recommend local or ci for this machine")
    m.add_argument("--json", action="store_true")

    w = sub.add_parser("watch", help="follow CI on a commit to its verdict")
    w.add_argument("sha", nargs="?", help="commit to gate (default: the branch head)")
    w.add_argument("--branch", help="branch whose head to watch (default: the current one)")
    w.add_argument("--once", action="store_true", help="one snapshot; exit 2 while running")
    w.add_argument("--detach", action="store_true", help="watch in the background")
    w.add_argument("--tail", action="store_true", help="print the background log")
    w.add_argument("--stop", action="store_true", help="stop the background watcher")
    w.add_argument("--lines", type=int, default=40, help="lines for --tail")
    w.add_argument("--json", action="store_true")

    fi = sub.add_parser("final", help="dispatch the full pool on the branch head; wait for all")
    fi.add_argument("--branch", help="branch to check (default: the current one)")

    s = sub.add_parser("shots", help="render shots in CI and download them")
    s.add_argument("spec", nargs="+", help='"NAME key=value ...", the shot example\'s keys')
    s.add_argument("--ref", default="main",
                   help="branch whose workflow runs (e.g. one adding fonts)")
    wb = sub.add_parser("web", help="build the browser preview in CI and download it")
    me = sub.add_parser("memory", help="measure memory and its budgets in CI")
    me.add_argument("--ref", default="main",
                    help="branch whose workflow runs (one with the memory task)")
    ko = sub.add_parser("kokoro", help="check the Kokoro port against its reference in CI")
    ko.add_argument("--ref", default="main",
                    help="branch whose workflow runs (one with the kokoro task)")
    pi = sub.add_parser("piper", help="check the Piper voices against onnxruntime in CI")
    pi.add_argument("--ref", default="main",
                    help="branch whose workflow runs (one with the piper task)")
    ki = sub.add_parser("kitten", help="check the Kitten builds against onnxruntime in CI")
    ki.add_argument("--ref", default="main",
                    help="branch whose workflow runs (one with the kitten task)")
    ph = sub.add_parser("phonon", help="check the Phonon port against ptts in CI")
    ph.add_argument("--ref", default="main",
                    help="branch whose workflow runs (one with the phonon task)")
    ip = sub.add_parser("ipa", help="check our other languages against espeak in CI")
    ip.add_argument("--ref", default="main",
                    help="branch whose workflow runs (one with the ipa task)")
    hz = sub.add_parser("hanzi", help="check our Chinese reader against misaki in CI")
    hz.add_argument("--ref", default="main",
                    help="branch whose workflow runs (one with the hanzi task)")
    sy = sub.add_parser("system", help="open system voices on macOS and Windows in CI")
    sy.add_argument("--ref", default="main",
                    help="branch whose workflow runs (one with the system task)")
    ins = sub.add_parser("installers", help="build and check every platform's installers")
    ins.add_argument("--ref", default="main",
                     help="branch whose workflow runs (one with installers.yml)")
    ins.add_argument("--download", action="store_true",
                     help="also download the installers (tens of MB each)")
    c = sub.add_parser("cache", help="a sandbox build cache made by CI")
    c.add_argument("action", choices=["build", "restore", "status", "env", "seed"],
                   help="seed is the cache job's own first step")
    c.add_argument("--full", action="store_true",
                   help="also test binaries and the shot example (local mode)")
    c.add_argument("--cold", action="store_true",
                   help="build from nothing, e.g. for a new toolchain")
    c.add_argument("--run", type=int, help="restore this run's cache, not the newest")
    c.add_argument("--force", action="store_true", help="replace an existing cache")
    for p in (w, fi, s, wb, me, ko, pi, ki, ph, ip, hz, sy, ins, c):
        p.add_argument("--timeout", type=int, default=3600, help="seconds to wait")
        p.add_argument("--poll", type=int, default=15, help="seconds between polls")
    for p in (s, wb, me, ko, pi, ki, ph, ip, hz, sy, ins, c):
        p.add_argument("--rev", default="HEAD", help="commit to use (must be pushed)")
    for p in (s, wb, me, ko, pi, ki, ph, ip, hz, sy, ins):
        p.add_argument("--out", help="where to unpack (default target/remote/<task>)")

    args = ap.parse_args()
    if args.cmd == "mode":
        return mode(args)
    if args.cmd == "watch":
        if args.tail:
            return tail(args.lines)
        if args.stop:
            return stop()
        return detach(args) if args.detach else watch(args)
    if args.cmd == "final":
        return final(args)
    if args.cmd == "shots":
        return shots(args)
    if args.cmd == "web":
        return web(args)
    if args.cmd == "memory":
        return memory(args)
    if args.cmd == "kokoro":
        return kokoro(args)
    if args.cmd == "piper":
        return kokoro(args, "piper")
    if args.cmd == "kitten":
        return kokoro(args, "kitten")
    if args.cmd == "phonon":
        return kokoro(args, "phonon")
    if args.cmd == "ipa":
        return kokoro(args, "ipa")
    if args.cmd == "hanzi":
        return kokoro(args, "hanzi")
    if args.cmd == "system":
        return kokoro(args, "system")
    if args.cmd == "installers":
        return installers(args)
    cache = cache_module()
    if args.action == "seed":
        return cache.cache_seed(args)
    if args.action == "status":
        return cache.cache_status(args)
    if args.action == "env":
        return cache.print_env(args)
    return cache.cache_build(args) if args.action == "build" else cache.cache_restore(args)


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(130)
