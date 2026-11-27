"""Tags main as a nightly pre-release and starts build.yml for it (.github/workflows/nightly.yml).

Tag: v<next minor>.0-nightly.<UTC yyyymmddhhmm>. It sorts above the last release and below the next
one (release_feed.rs), so a nightly install moves on to the next release on its own. Only a build
that runs a pre-release is offered pre-releases, so stable installs never see nightlies.
"""

import datetime
import json
import re
import subprocess
import sys
import urllib.parse

STABLE_TAG = re.compile(r"^v(\d+)\.(\d+)\.(\d+)$")
NIGHTLY_TAG = re.compile(r"^v\d+\.\d+\.\d+-nightly\.\d{12}$")
KEEP_NIGHTLIES = 7
BUILD_WORKFLOW = "build.yml"


def next_version(tags: list[str]) -> str:
    stable = [tuple(map(int, match.groups())) for match in map(STABLE_TAG.match, tags) if match]
    major, minor, _ = max(stable, default=(0, 0, 0))
    return f"{major}.{minor + 1}.0"


def nightly_tag(tags: list[str], now: datetime.datetime) -> str:
    return f"v{next_version(tags)}-nightly.{now.strftime('%Y%m%d%H%M')}"


def stale_nightlies(tags: list[str], keep: int = KEEP_NIGHTLIES) -> list[str]:
    nightlies = sorted((tag for tag in tags if NIGHTLY_TAG.match(tag)), key=lambda tag: tag.rsplit(".", 1)[1])
    return nightlies[:-keep] if keep else nightlies


def run(*command: str) -> str:
    return subprocess.run(command, check=True, capture_output=True, text=True).stdout.strip()


def gh_api(method: str, path: str, *fields: str) -> str:
    return run("gh", "api", "-X", method, path, *fields)


def delete_nightly(repository: str, tag: str) -> None:
    try:
        release = json.loads(gh_api("GET", f"repos/{repository}/releases/tags/{urllib.parse.quote(tag)}"))
        gh_api("DELETE", f"repos/{repository}/releases/{release['id']}")
    except subprocess.CalledProcessError:
        pass
    run("git", "push", "--delete", "origin", tag)


def main() -> int:
    repository = run("gh", "repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner")
    if any(NIGHTLY_TAG.match(tag) for tag in run("git", "tag", "--points-at", "HEAD").split()):
        print("main has not moved since the last nightly")
        return 0
    tags = run("git", "tag", "--list", "v*").split()
    tag = nightly_tag(tags, datetime.datetime.now(datetime.timezone.utc))
    run("git", "tag", tag)
    run("git", "push", "origin", tag)
    # A tag pushed with the workflow token starts no workflow; a dispatch does.
    run("gh", "workflow", "run", BUILD_WORKFLOW, "--ref", tag)
    print(f"started {BUILD_WORKFLOW} for {tag}")
    for stale in stale_nightlies(tags + [tag]):
        delete_nightly(repository, stale)
        print(f"deleted {stale}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
