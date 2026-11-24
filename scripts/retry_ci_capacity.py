"""Retry CI jobs whose GitHub-hosted runner was never acquired."""

import json
import os
import subprocess
import sys
from datetime import datetime, timedelta, timezone
from pathlib import Path
from urllib.parse import quote


CAPACITY_ERROR = "The job was not acquired by Runner of type hosted even after multiple attempts"
FAILED = {"failure", "cancelled"}


def api(path, *, method="GET", paginate=False):
    command = ["gh", "api", "--method", method, f"repos/{os.environ['GITHUB_REPOSITORY']}/{path}"]
    if paginate:
        command.extend(["--paginate", "--slurp"])
    response = subprocess.run(command, check=True, capture_output=True, text=True)
    return json.loads(response.stdout) if response.stdout else None


def retry_path(run):
    if (
        run["status"] != "completed"
        or run["conclusion"] not in FAILED
        or run["run_attempt"] != 1
        or run["path"].split("@", 1)[0] != ".github/workflows/ci.yml"
    ):
        return None

    run_path = f"actions/runs/{run['id']}"
    pages = api(f"{run_path}/attempts/1/jobs?per_page=100", paginate=True)
    failed_jobs = [
        job for page in pages for job in page["jobs"]
        if job["conclusion"] not in {"success", "skipped", "neutral"}
    ]
    capacity_jobs = []
    for job in failed_jobs:
        if job["conclusion"] not in FAILED or job["runner_id"] or job["steps"]:
            continue
        check_id = int(job["check_run_url"].rsplit("/", 1)[1])
        annotations = api(f"check-runs/{check_id}/annotations?per_page=100", paginate=True)
        if any(
            item["annotation_level"] == "failure" and item["message"].strip() == CAPACITY_ERROR
            for page in annotations for item in page
        ):
            capacity_jobs.append(job)

    if len(capacity_jobs) == 1:
        return f"actions/jobs/{capacity_jobs[0]['id']}/rerun"
    if capacity_jobs and len(capacity_jobs) == len(failed_jobs):
        return f"{run_path}/rerun-failed-jobs"
    return None


def still_current(run):
    if run["event"] == "push":
        branch = quote(run["head_branch"], safe="/")
        return api(f"git/ref/heads/{branch}")["object"]["sha"] == run["head_sha"]
    if run["event"] == "pull_request":
        for entry in run["pull_requests"]:
            pull = api(f"pulls/{entry['number']}")
            if pull["state"] == "open" and pull["head"]["sha"] == run["head_sha"]:
                return True
    return False


def main(mode):
    event = json.loads(Path(os.environ["GITHUB_EVENT_PATH"]).read_text(encoding="utf-8"))
    run_id = int(event["workflow_run"]["id"])
    run = api(f"actions/runs/{run_id}")
    path = retry_path(run)
    if mode == "inspect":
        with Path(os.environ["GITHUB_OUTPUT"]).open("a", encoding="utf-8") as output:
            output.write(f"retry={str(path is not None).lower()}\n")
        print("Hosted runner capacity failure confirmed." if path else "No eligible capacity failure.")
        return
    if not path or not still_current(run):
        print("The CI run is no longer eligible for a retry.")
        return
    completed_at = datetime.fromisoformat(run["updated_at"].replace("Z", "+00:00"))
    if datetime.now(timezone.utc) < completed_at + timedelta(hours=1):
        raise SystemExit("Retry is too early. Set the ci-capacity-retry environment wait timer to 60 minutes.")
    latest = api(f"actions/runs/{run_id}")
    if latest["status"] != "completed" or latest["run_attempt"] != 1:
        print("The CI run was already retried.")
        return
    api(path, method="POST")
    print(f"Requested a capacity retry for CI run {run_id}.")


if __name__ == "__main__":
    if len(sys.argv) != 2 or sys.argv[1] not in {"inspect", "retry"}:
        raise SystemExit("Usage: retry_ci_capacity.py {inspect|retry}")
    try:
        main(sys.argv[1])
    except subprocess.CalledProcessError as error:
        raise SystemExit(error.stderr.strip() or "GitHub API request failed.") from error
