#!/usr/bin/env python3
"""Poll GitHub Actions for a pushed revision and report failures.

Built for a sandbox that cannot compile Rust or install toolchains: the only way
to know whether a change is good is to push it and read the result back.

  python3 tools/ci_watch.py                 # watch CI on the branch head
  python3 tools/ci_watch.py --final <sha>    # CI + Deep CI on the gated SHA
  python3 tools/ci_watch.py --once           # one snapshot, no waiting
  python3 tools/ci_watch.py --detach --final # watch in the background, return
  python3 tools/ci_watch.py --tail           # read the background log
  python3 tools/ci_watch.py --stop           # stop a detached watcher

Exit code: 0 green, 1 red, 2 not finished yet (so `--once` is a poll and a
detached watcher's log is the only place to look).

The loop policy this encodes (AGENTS.md, "The CI loop"):

  * `CI` gates every push and is the only workflow watched by default; `--final`
    adds `Deep CI`, which gates the round's last code-bearing commit alone.
  * A red job inside a run is the whole answer, so the wait stops there
    (`--no-fast-fail` waits for the run anyway) and digests the failed jobs while
    their siblings are still running.
  * A run a newer push cancelled is not a verdict, so it reports as superseded
    rather than red.
  * Reads on a public repository need no token; only log downloads do, and those
    degrade to a message instead of an exception.

No third-party dependencies. A token comes from --token, --token-file,
$GITHUB_TOKEN or $GH_TOKEN, and is never written anywhere.
"""

from __future__ import annotations

import argparse
import io
import json
import os
import re
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
REPO_DEFAULT = "codewiththiha/mareader"
DEFAULT_WORKFLOWS = ("CI",)
FINAL_WORKFLOWS = ("CI", "Deep CI")


def log(msg: str) -> None:
    print(msg, flush=True)


class _HopAuth(urllib.request.HTTPRedirectHandler):
    """Azure 401s a SAS-signed log URL that arrives with `Authorization`; the
    GitHub-side log receiver needs it. urllib forwards the header either way."""

    def redirect_request(self, req, fp, code, msg, headers, newurl):  # noqa: D102
        new = super().redirect_request(req, fp, code, msg, headers, newurl)
        if new is None or not req.has_header("Authorization"):
            return new
        if "sig=" in urllib.parse.urlparse(newurl).query:
            if new.has_header("Authorization"):
                new.remove_header("Authorization")
        else:
            new.add_header("Authorization", req.get_header("Authorization"))
        return new


_OPENER = urllib.request.build_opener(_HopAuth())


def fetch_raw(url: str, token: str, retries: int = 4) -> bytes:
    """Fetch an Actions log; a 403 or 429 here is a secondary rate limit."""
    left = retries
    while True:
        left -= 1
        req = urllib.request.Request(url)
        req.add_header("User-Agent", "mareader-ci-watch")
        if token:
            req.add_header("Authorization", f"Bearer {token}")
        try:
            with _OPENER.open(req, timeout=120) as resp:
                return resp.read()
        except urllib.error.HTTPError as exc:
            if exc.code not in (403, 429) or left <= 0:
                raise
            wait = int(exc.headers.get("Retry-After") or 15)
            log(f"  log refused ({exc.code}); waiting {wait}s")
            time.sleep(wait)


def read_error(code: int, url: str, body: str) -> str:
    """A refusal one line long, with the fix in it."""
    if code in (401, 403) and "rate limit" in body.lower():
        return (f"{url}: HTTP {code} rate limit — an unauthenticated read allows 60 "
                "requests an hour; pass --token or --token-file")
    return f"{url}: HTTP {code}: {body[:200].strip()}"


def api(token: str, path: str, *, raw: bool = False, retries: int = 4):
    url = path if path.startswith("http") else f"{API}{path}"
    last = None
    for attempt in range(retries):
        req = urllib.request.Request(url)
        if token:
            req.add_header("Authorization", f"Bearer {token}")
        req.add_header("Accept", "application/vnd.github+json")
        req.add_header("X-GitHub-Api-Version", "2022-11-28")
        req.add_header("User-Agent", "mareader-ci-watch")
        try:
            with _OPENER.open(req, timeout=60) as resp:
                data = resp.read()
                if raw:
                    return data
                return json.loads(data.decode("utf-8"))
        except urllib.error.HTTPError as exc:
            body = exc.read().decode("utf-8", "replace")
            if exc.code == 404:
                return None
            wait = exc.headers.get("Retry-After")
            if wait and exc.code in (403, 429) and attempt < retries - 1:
                log(f"  secondary rate limit ({exc.code}); waiting {int(wait)}s")
                time.sleep(int(wait))
                last = exc
                continue
            raise SystemExit(read_error(exc.code, url, body)) from exc
        except (urllib.error.URLError, TimeoutError) as exc:
            last = exc
            if attempt < retries - 1:
                time.sleep(3 * (attempt + 1))
                continue
            raise SystemExit(f"{url}: network error: {exc}") from exc
    raise RuntimeError(f"giving up on {url}: {last}")


@dataclass
class Job:
    name: str
    status: str
    conclusion: str | None
    id: int


@dataclass
class Run:
    name: str
    status: str
    conclusion: str | None
    id: int
    sha: str
    url: str
    created: str
    jobs: list[Job] = field(default_factory=list)

    @property
    def finished(self) -> bool:
        return self.status == "completed"


def runs_for(repo: str, token: str, sha: str) -> list[Run]:
    payload = api(token, f"/repos/{repo}/actions/runs?head_sha={sha}&per_page=100")
    out = []
    for item in (payload or {}).get("workflow_runs", []):
        out.append(
            Run(
                name=item["name"],
                status=item["status"],
                conclusion=item.get("conclusion"),
                id=item["id"],
                sha=item["head_sha"],
                url=item["html_url"],
                created=item["created_at"],
            )
        )
    return out


def jobs_for(repo: str, token: str, run_id: int) -> list[Job]:
    payload = api(token, f"/repos/{repo}/actions/runs/{run_id}/jobs?per_page=100")
    return [
        Job(j["name"], j["status"], j.get("conclusion"), j["id"])
        for j in (payload or {}).get("jobs", [])
    ]


def head_sha(repo: str, token: str, branch: str) -> str:
    info = api(token, f"/repos/{repo}/commits/{branch}")
    if not info:
        raise SystemExit(f"cannot resolve {branch}")
    return info["sha"]


def branch_of_sha(repo: str, token: str, sha: str) -> str:
    info = api(token, f"/repos/{repo}/commits/{sha}/branches-where-head")
    if info:
        return info[0]["name"]
    return ""


ANSI = re.compile(r"\x1b\[[0-9;]*m")

# A run a newer push cancelled is no verdict; calling it red wastes a round.
SUPERSEDED = ("cancelled", "startup_cancelled")


def is_red(conclusion: str | None) -> bool:
    """Whether a conclusion means the code has to answer for it."""
    return conclusion not in (None, "success", "skipped", *SUPERSEDED)
STEP_FAIL = re.compile(r"##\[error\]|Error:|error(\[|:)|FAILED|panicked|Diff in")
# A rustfmt diff is only useful with its hunk, and a hunk follows the marker.
DIFF_CONTEXT = 14
STAMP = re.compile(r"^\d{4}-\d{2}-\d{2}T[\d:.]+Z ?")
SKIP_NOISE = re.compile(
    r"^\s*(Download|Extract|Cache|adding |Receiving object|Resolving deltas|Updating files)"
)


def job_log(repo: str, token: str, job_id: int) -> str:
    """One job's log, which — unlike the run archive — exists mid-run."""
    try:
        raw = fetch_raw(f"{API}/repos/{repo}/actions/jobs/{job_id}/logs", token)
    except Exception as exc:  # noqa: BLE001 - a missing log is not fatal
        return f"(job {job_id} log failed: {exc})"
    return ANSI.sub("", raw.decode("utf-8", "replace"))


def digest(text: str) -> str:
    """Error lines (with a rustfmt hunk kept whole) plus the log's tail."""
    lines = [STAMP.sub("", ln) for ln in text.splitlines()]
    marks = [i for i, ln in enumerate(lines) if STEP_FAIL.search(ln)]
    out = []
    if marks:
        out.append("--- error lines ---")
        shown: set[int] = set()
        for i in marks[:60]:
            span = range(i, i + DIFF_CONTEXT) if "Diff in" in lines[i] else (i,)
            for j in span:
                if j < len(lines) and j not in shown:
                    shown.add(j)
                    out.append(lines[j])
    tail = [ln for ln in lines[-250:] if not SKIP_NOISE.match(ln)]
    out.append("--- tail ---")
    out.extend(tail[-120:])
    return "\n".join(out)


def fetch_failed_logs(repo: str, token: str, run: Run) -> str:
    """An extract of the logs of the jobs that failed.

    A job's own log endpoint is the primary source rather than the run archive
    for two reasons: it answers while the run is still going, which is what a
    fast-fail round reads, and it needs no scope beyond Actions read — this PAT's
    archive download has come back 403 more than once. The archive is only
    reached when no job reported a conclusion at all (a run cancelled outright),
    where there is nothing to select and the whole bundle is the question.
    """
    failed = [j for j in run.jobs if is_red(j.conclusion)]
    if failed:
        return "\n".join(
            f"\n===== {j.name} ({j.conclusion}) =====\n{digest(job_log(repo, token, j.id))}"
            for j in failed
        )
    try:
        raw = fetch_raw(f"{API}/repos/{repo}/actions/runs/{run.id}/logs", token)
    except RuntimeError as exc:
        return f"(log download failed: {exc})"
    if not raw:
        return "(no log archive available; logs may have expired)"
    buf = io.BytesIO(raw)
    out: list[str] = []
    try:
        zf = zipfile.ZipFile(buf)
    except zipfile.BadZipFile:
        return "(log archive was not a zip; logs may have expired)"
    # Archive entries replace "/" in a job name: "Rust / format" -> "Rust _ format".
    for item in sorted(zf.namelist()):
        if item.endswith("/"):
            continue
        out.append(f"\n===== {item} =====")
        out.append(digest(zf.read(item).decode("utf-8", "replace")))
    if out:
        return "\n".join(out)
    return "(the archive holds no readable entry: " + \
        ", ".join(sorted(zf.namelist())[:6]) + ")"


def summarize(run: Run) -> str:
    bad = [j for j in run.jobs if is_red(j.conclusion)]
    if not bad:
        return f"{run.name}: {run.conclusion or run.status}"
    state = run.conclusion or run.status
    return f"{run.name}: {state} — failed jobs: " + ", ".join(
        f"{j.name} ({j.conclusion})" for j in bad
    )


def wait_for(repo: str, token: str, sha: str, workflows: tuple[str, ...],
             timeout: int, poll: int, show_logs: bool,
             fast_fail: bool = True) -> dict[str, Run]:
    """Wait until every wanted workflow on this SHA is completed.

    With `fast_fail` a red job inside a run ends the wait: the run cannot get
    better, and the remaining jobs cannot change what to fix.
    """
    deadline = time.time() + timeout
    seen_done: dict[str, Run] = {}
    noted: set[str] = set()
    while True:
        runs = latest_per_workflow(
            [r for r in runs_for(repo, token, sha) if r.name in workflows])
        if not runs:
            if time.time() > deadline:
                raise SystemExit(f"no workflow runs appeared for {sha[:9]} within {timeout}s")
            log(f"  waiting for a run to appear for {sha[:9]} ...")
            time.sleep(min(poll, 15))
            continue
        for run in runs:
            run.jobs = jobs_for(repo, token, run.id)
            state = f"{run.name}={run.status}"
            if run.finished and run.conclusion not in (None, "success"):
                state += f"/{run.conclusion}"
            if state not in noted:
                noted.add(state)
                detail = ""
                if not run.finished and run.jobs:
                    running = [j.name for j in run.jobs if j.status != "completed"]
                    detail = " running: " + ", ".join(running[:6])
                log(f"  {sha[:9]} {state}{detail}")
        red = [r for r in runs if any(is_red(j.conclusion) for j in r.jobs)]
        if fast_fail and red:
            for run in red:
                log("  " + summarize(run) + " — stopped early: a red job is the answer")
                if show_logs:
                    log(f"\n########## {run.name} failure digest ({run.url}) ##########")
                    log(fetch_failed_logs(repo, token, run))
                    log("########## end digest ##########\n")
            return {r.name: r for r in runs}
        if all(r.finished for r in runs) and len(runs) >= len(workflows):
            for run in runs:
                seen_done[run.name] = run
            for run in runs:
                log("  " + summarize(run))
            if show_logs:
                for run in runs:
                    if is_red(run.conclusion):
                        log(f"\n########## {run.name} failure digest ({run.url}) ##########")
                        log(fetch_failed_logs(repo, token, run))
                        log("########## end digest ##########\n")
            return seen_done
        if time.time() > deadline:
            log(f"  timed out after {timeout}s waiting on {sha[:9]}")
            for run in runs:
                log("  " + summarize(run))
                if show_logs and is_red(run.conclusion):
                    log(fetch_failed_logs(repo, token, run))
            return seen_done
        time.sleep(poll)


def token_from(args) -> str:
    """Token for the API. Empty is legal: a public repository answers reads
    without one (60 requests an hour), and only log downloads need more."""
    if args.token:
        return args.token.strip()
    if args.token_file:
        path = os.path.expanduser(args.token_file)
        try:
            return open(path, encoding="utf-8").read().strip()
        except OSError as exc:
            raise SystemExit(f"--token-file {path}: {exc.strerror}") from exc
    return os.environ.get("GITHUB_TOKEN") or os.environ.get("GH_TOKEN") or ""


def git_dir() -> str:
    """This checkout's git directory, empty when not inside a repository."""
    try:
        out = subprocess.run(["git", "rev-parse", "--absolute-git-dir"],
                             capture_output=True, text=True, check=True)
    except Exception:  # noqa: BLE001 - not a repository, or git is missing
        return ""
    return out.stdout.strip()


def state_path(name: str) -> str:
    root = git_dir() or "."
    return os.path.join(root, f"ci-watch.{name}")


def current_branch() -> str:
    try:
        out = subprocess.run(["git", "rev-parse", "--abbrev-ref", "HEAD"],
                             capture_output=True, text=True, check=True)
    except Exception:  # noqa: BLE001 - not a repository, or detached HEAD
        return ""
    name = out.stdout.strip()
    return "" if name == "HEAD" else name


def latest_per_workflow(runs: list[Run]) -> list[Run]:
    """One run per workflow: the newest decides, and the older ones are context.

    A SHA can hold several runs of one workflow — a push the `[skip deep]` marker
    emptied, then a dispatch that really ran the lane. The gate is the newest.
    """
    by_name: dict[str, Run] = {}
    for run in runs:
        current = by_name.get(run.name)
        if current is None or run.created > current.created:
            by_name[run.name] = run
    return list(by_name.values())


def snapshot(repo: str, token: str, sha: str,
             workflows: tuple[str, ...]) -> list[Run]:
    """One read of every wanted workflow's run on this SHA, jobs included."""
    runs = latest_per_workflow([r for r in runs_for(repo, token, sha) if r.name in workflows])
    for run in runs:
        run.jobs = jobs_for(repo, token, run.id)
    return runs


def print_state(runs: list[Run]) -> None:
    for run in runs:
        log("  " + summarize(run))


def digests(repo: str, token: str, runs: list[Run]) -> None:
    """Failure detail for every run with a red job, finished or not."""
    for run in runs:
        if not (is_red(run.conclusion) or any(is_red(j.conclusion) for j in run.jobs)):
            continue
        log(f"\n########## {run.name} failure digest ({run.url}) ##########")
        log(fetch_failed_logs(repo, token, run))
        log("########## end digest ##########\n")


def verdict_of(sha: str, runs: list[Run], workflows: tuple[str, ...]) -> int:
    """Print this SHA's verdict and return its exit code."""
    unfinished = [r for r in runs if not r.finished]
    red = [r for r in runs if is_red(r.conclusion)]
    superseded = [r.name for r in runs if r.conclusion in SUPERSEDED]
    if unfinished or not runs:
        waiting = ", ".join(r.name for r in unfinished) or "no run has appeared yet"
        log(f"IN PROGRESS for {sha[:9]} — waiting on: {waiting}")
        return 2
    if all(r.conclusion == "success" for r in runs) and len(runs) >= len(workflows):
        log(f"VERDICT GREEN for {sha[:9]}")
        return 0
    if not red and superseded:
        log(f"VERDICT SUPERSEDED for {sha[:9]} — {'/'.join(superseded)} was cancelled by "
            "a newer push, so this SHA never got a verdict; watch the branch head instead "
            "(drop --sha, or use --loop)")
        return 2
    log(f"VERDICT RED for {sha[:9]}")
    return 1


def tail(lines: int) -> int:
    path = state_path("log")
    if not os.path.exists(path):
        raise SystemExit(f"no watcher log at {path} — start one with --detach")
    text = open(path, errors="replace").read().splitlines()
    for line in text[-lines:]:
        print(line)
    # Only the newest run's verdict counts: the log accumulates across watchers.
    start = max(i for i in range(len(text)) if text[i].startswith("== watching"))
    for line in reversed(text[start:]):
        if line.startswith("VERDICT GREEN"):
            return 0
        if line.startswith("VERDICT RED"):
            return 1
    return 2


def stop() -> int:
    path = state_path("pid")
    if not os.path.exists(path):
        print("no detached watcher is running (no pid file)")
        return 0
    pid = int(open(path).read().strip())
    try:
        os.kill(pid, signal.SIGTERM)
        print(f"stopped detached watcher pid {pid}")
    except ProcessLookupError:
        print(f"pid {pid} is gone already")
    os.unlink(path)
    return 0


def detach(args) -> int:
    """Re-run this command as a background watcher writing to the log file."""
    path = state_path("log")
    pid = os.fork()
    if pid > 0:
        with open(state_path("pid"), "w") as handle:
            handle.write(str(pid))
        print(f"watching in the background: pid {pid}, log {path}")
        print("  read it with: python3 tools/ci_watch.py --tail")
        return 0
    os.setsid()
    with open(state_path("pid"), "w") as handle:
        handle.write(str(os.getpid()))
    fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_APPEND)
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


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("sha", nargs="?",
                    help="commit SHA to gate (default: the branch head)")
    ap.add_argument("--branch", default=None,
                    help="branch whose head to watch (default: the checked-out branch)")
    ap.add_argument("--repo", default=REPO_DEFAULT)
    ap.add_argument("--token", help="API token")
    ap.add_argument("--token-file",
                    help="read the token from this file (mode 600 is enough)")
    ap.add_argument("--workflows", default=None,
                    help="comma separated workflow names to require "
                         f"(default: {','.join(DEFAULT_WORKFLOWS)}; --final adds Deep CI)")
    ap.add_argument("--final", action="store_true",
                    help="this SHA is the round's last code-bearing commit: also wait "
                         "for Deep CI, the lane that gates it")
    ap.add_argument("--once", action="store_true",
                    help="read the runs and leave: exit 2 while anything is running")
    ap.add_argument("--detach", action="store_true",
                    help="run the watch in the background and return at once")
    ap.add_argument("--tail", action="store_true", help="print the background log")
    ap.add_argument("--stop", action="store_true", help="stop the detached watcher")
    ap.add_argument("--lines", type=int, default=40, help="lines for --tail")
    ap.add_argument("--timeout", type=int, default=3600, help="seconds to wait for one SHA")
    ap.add_argument("--poll", type=int, default=20, help="seconds between polls")
    ap.add_argument("--logs", action="store_true", default=True,
                    help="download failing job logs (default on)")
    ap.add_argument("--no-logs", dest="logs", action="store_false")
    ap.add_argument("--loop", action="store_true",
                    help="keep watching the branch head; report each new SHA once")
    ap.add_argument("--interval", type=int, default=45,
                    help="seconds between branch-head checks in --loop mode")
    ap.add_argument("--json", action="store_true", help="print a JSON verdict at the end")
    ap.add_argument("--no-fast-fail", dest="fast_fail", action="store_false",
                    help="keep waiting after a job turns red, until the run finishes")
    args = ap.parse_args()
    if args.tail:
        return tail(args.lines)
    if args.stop:
        return stop()
    if args.detach:
        if args.once:
            raise SystemExit("--detach and --once contradict: one returns at once already")
        return detach(args)
    return watch(args)


def watch(args) -> int:
    token = token_from(args)
    branch = args.branch or current_branch() or "main"
    wanted = args.workflows or (",".join(FINAL_WORKFLOWS if args.final else DEFAULT_WORKFLOWS))
    workflows = tuple(w.strip() for w in wanted.split(",") if w.strip())
    handled: set[str] = set()
    verdicts: list[dict] = []
    code = 1
    while True:
        sha = args.sha or head_sha(args.repo, token, branch)
        if sha in handled:
            if not args.loop:
                break
            time.sleep(args.interval)
            continue
        handled.add(sha)
        log(f"== watching {args.repo}@{sha[:12]} for {', '.join(workflows)}")
        if "Deep CI" not in workflows:
            log("   Deep CI not watched: it gates the round's last code-bearing "
                "commit (rerun with --final)")
        where = branch_of_sha(args.repo, token, sha) or branch
        if where:
            log(f"   head of {where}")
        if args.once:
            runs = snapshot(args.repo, token, sha, workflows)
            print_state(runs)
            if args.logs:
                digests(args.repo, token, runs)
            code = verdict_of(sha, runs, workflows)
            verdicts.append({"sha": sha, "code": code,
                             "runs": {r.name: {"status": r.status, "conclusion": r.conclusion,
                                               "url": r.url} for r in runs}})
            break
        runs_by_name = wait_for(args.repo, token, sha, workflows, args.timeout,
                                args.poll, args.logs, args.fast_fail)
        code = verdict_of(sha, list(runs_by_name.values()), workflows)
        verdicts.append({"sha": sha, "code": code,
                         "runs": {name: {"status": r.status, "conclusion": r.conclusion,
                                         "url": r.url,
                                         "failed_jobs": [j.name for j in r.jobs
                                                         if is_red(j.conclusion)]}
                                  for name, r in runs_by_name.items()}})
        if not args.loop:
            break
        time.sleep(args.interval)

    if args.json:
        print(json.dumps(verdicts, indent=2))
    return code


args = None


if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        sys.exit(130)


