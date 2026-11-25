"""Checks whether a pull request is ready for review (CONTRIBUTING.md, "When a PR is ready for review").

Run by .github/workflows/pr-gate.yml under pull_request_target: it reads the PR as data
through the API and never checks out or runs code from the PR.

Locally, before a PR leaves draft: `python3 .github/scripts/pr_gate.py --dry-run <number>`.
It prints what is missing and changes nothing on GitHub.
"""

import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from dataclasses import dataclass, field
from typing import Any

TITLE_PATTERN = re.compile(r"^(feat|fix|perf|refactor|docs|test|chore|ci|build)(\([a-z0-9-]+\))?!?: \S")
UI_DIRECTORIES = ("views", "settings", "components", "widgets")
UI_PATH_PATTERN = re.compile(rf"^crates/app/src/({'|'.join(UI_DIRECTORIES)})/|^crates/app/src/[^/]+_view\.rs$|/locales/")
IMAGE_PATTERN = re.compile(r"!\[[^\]]*\]\(|<img\s|<video\s|github\.com/user-attachments/|\.(png|jpe?g|gif|webp|mp4|webm)\b", re.IGNORECASE)
ISSUE_REFERENCE_PATTERN = re.compile(
    r"\b(?:close[sd]?|fix(?:e[sd])?|resolve[sd]?):?\s+(?:https://github\.com/danieldaeschle/rusty-teams/issues/|#)(\d+)", re.IGNORECASE
)
NO_VISUAL_CHECKBOX_PATTERN = re.compile(r"^\s*[-*]\s*\[[xX]\]\s*no visible change", re.IGNORECASE | re.MULTILINE)
PLANNING_FOLDERS = ("openspec", ".specify", ".kiro")
FORBIDDEN_PATH_PATTERN = re.compile(rf"^({'|'.join(re.escape(folder) for folder in PLANNING_FOLDERS)})/")
WINDOWS_CODE_PATTERN = re.compile(r"^\+.*(cfg\((all\()?windows|target_os\s*=\s*\"windows\")", re.MULTILINE)
WINDOWS_PATH_PATTERN = re.compile(r"(^|/)(win32|windows)(/|_[a-z]+\.rs$)|_win(dows)?\.rs$")
LOCKFILE_NAMES = ("Cargo.lock", "package-lock.json", "pnpm-lock.yaml", "yarn.lock")

MAX_CHANGED_LINES = 400
MIN_PROBLEM_CHARACTERS = 80
MIN_REVIEW_LINES = 2
REQUIRED_SECTIONS = ("Summary", "AI model", "Tests", "Review")
SECTION_ALIASES = {"tests": ("test plan", "testing", "test")}

LABEL_NEEDS_INFO = "status: needs info"
LABEL_NEEDS_REVIEW = "status: needs review"
STATUS_LABEL_PREFIX = "status: "
EXEMPT_ASSOCIATIONS = ("OWNER", "MEMBER", "COLLABORATOR")
LABEL_ACCEPTED = "status: accepted"
LABEL_NO_VISUAL = "gate: no visual"
LABEL_SKIP = "gate: skip"

COMMENT_MARKER = "<!-- pr-gate -->"
DEFAULT_REPOSITORY = "danieldaeschle/rusty-teams"


@dataclass
class ChangedFile:
    path: str
    additions: int = 0
    deletions: int = 0
    patch: str = ""


@dataclass
class PullRequest:
    title: str
    body: str
    labels: list[str] = field(default_factory=list)
    files: list[ChangedFile] = field(default_factory=list)
    linked_issue_labels: dict[int, list[str]] = field(default_factory=dict)


def strip_comments(text: str) -> str:
    return re.sub(r"<!--.*?-->", "", text or "", flags=re.DOTALL)


def parse_sections(body: str) -> dict[str, str]:
    sections: dict[str, str] = {}
    current_heading = None
    current_lines: list[str] = []
    for line in strip_comments(body).splitlines():
        heading = re.match(r"^##\s+(.+?)\s*$", line)
        if heading:
            if current_heading is not None:
                sections[current_heading] = "\n".join(current_lines).strip()
            current_heading = heading.group(1).strip().lower()
            current_lines = []
        elif current_heading is not None:
            current_lines.append(line)
    if current_heading is not None:
        sections[current_heading] = "\n".join(current_lines).strip()
    return sections


def find_section(sections: dict[str, str], name: str) -> str | None:
    names = (name.lower(), *SECTION_ALIASES.get(name.lower(), ()))
    for heading, content in sections.items():
        if any(heading == wanted or heading.startswith(wanted + " ") or heading.startswith(wanted + ":") for wanted in names):
            return content
    return None


def linked_issue_numbers(body: str) -> list[int]:
    return sorted({int(number) for number in ISSUE_REFERENCE_PATTERN.findall(strip_comments(body))})


def changed_lines(files: list[ChangedFile]) -> int:
    return sum(changed.additions + changed.deletions for changed in files if changed.path.rsplit("/", 1)[-1] not in LOCKFILE_NAMES)


def title_type(title: str) -> str:
    return title.split("(", 1)[0].split(":", 1)[0].split("!", 1)[0].strip()


def non_empty_lines(text: str) -> list[str]:
    return [line for line in text.splitlines() if line.strip()]


def check(pull_request: PullRequest) -> list[str]:
    """Returns one line per missing item, empty when the PR is ready for review."""
    missing: list[str] = []
    sections = parse_sections(pull_request.body)
    issue_numbers = linked_issue_numbers(pull_request.body)
    paths = [changed.path for changed in pull_request.files]

    if not TITLE_PATTERN.match(pull_request.title):
        missing.append("**Title**: use `type(scope): message` from the user's view, e.g. `fix(chat): a sent image no longer blinks`.")

    for name in REQUIRED_SECTIONS:
        if not find_section(sections, name):
            missing.append(f"**{name}**: the `## {name}` section is missing or empty.")

    if not issue_numbers:
        problem = find_section(sections, "Problem") or ""
        if len(problem) < MIN_PROBLEM_CHARACTERS:
            missing.append(
                "**Problem**: add a `## Problem` section: what is wrong or missing, when a user hits it "
                "(steps or when/then) and what should happen instead. A fix needs no issue; "
                "a linked issue (`Closes #123`) replaces this section."
            )

    size = changed_lines(pull_request.files)
    needs_accepted_issue = title_type(pull_request.title) == "feat" or size > MAX_CHANGED_LINES
    if needs_accepted_issue:
        accepted = any(LABEL_ACCEPTED in pull_request.linked_issue_labels.get(number, []) for number in issue_numbers)
        if not accepted:
            reason = "a feature" if title_type(pull_request.title) == "feat" else f"{size} changed lines (over {MAX_CHANGED_LINES})"
            missing.append(
                f"**Issue first**: this PR is {reason}. Link an issue that a maintainer marked `{LABEL_ACCEPTED}`, "
                "or split the change into smaller PRs."
            )

    if any(UI_PATH_PATTERN.search(path) for path in paths) and LABEL_NO_VISUAL not in pull_request.labels:
        screenshot = find_section(sections, "Screenshot") or ""
        has_image = bool(IMAGE_PATTERN.search(strip_comments(pull_request.body)))
        if not has_image and not NO_VISUAL_CHECKBOX_PATTERN.search(screenshot):
            missing.append(
                "**Screenshot**: the change touches UI code. Add a shot of the changed area taken with `--demo` data, "
                "or tick `No visible change` and say why."
            )

    tests = find_section(sections, "Tests") or ""
    touches_windows = any(WINDOWS_CODE_PATTERN.search(changed.patch) or WINDOWS_PATH_PATTERN.search(changed.path) for changed in pull_request.files)
    if tests and touches_windows and "windows" not in tests.lower():
        missing.append("**Tests**: the change touches Windows-only code. Say whether you built and ran it on Windows.")

    review = find_section(sections, "Review") or ""
    if review and len(non_empty_lines(review)) < MIN_REVIEW_LINES:
        missing.append("**Review**: list at least two independent reviews of the diff and what happened to each finding (AGENTS.md).")

    forbidden = sorted({path.split("/", 1)[0] for path in paths if FORBIDDEN_PATH_PATTERN.match(path)})
    if forbidden:
        missing.append(f"**Scope**: remove planning folders from the PR: {', '.join(f'`{name}/`' for name in forbidden)}.")

    if any(path.startswith("vendor/") for path in paths) and "vendor" not in strip_comments(pull_request.body).lower():
        missing.append("**Scope**: the PR changes `vendor/`. Say in the summary why the fix belongs in the fork.")

    return missing


def render_comment(missing: list[str]) -> str:
    if not missing:
        return f"{COMMENT_MARKER}\n**PR gate: ready for review.** Thanks, a maintainer will take a look."
    items = "\n".join(f"- [ ] {item}" for item in missing)
    return (
        f"{COMMENT_MARKER}\n**PR gate: not ready for review yet.** Please add:\n\n{items}\n\n"
        "Edit the description or push, and this comment updates itself. When everything is there, mark the PR "
        "ready for review. The rules are in CONTRIBUTING.md under \"When a PR is ready for review\". "
        f"A PR that stays `{LABEL_NEEDS_INFO}` for 14 days is closed; it can be reopened any time."
    )


class GitHub:
    def __init__(self, repository: str, token: str):
        self.base_url = f"https://api.github.com/repos/{repository}"
        self.token = token

    def request(self, method: str, url: str, payload=None) -> Any:
        full_url = url if url.startswith("https://") else self.base_url + url
        data = json.dumps(payload).encode() if payload is not None else None
        request = urllib.request.Request(full_url, data=data, method=method)
        if self.token:
            request.add_header("Authorization", f"Bearer {self.token}")
        request.add_header("Accept", "application/vnd.github+json")
        request.add_header("X-GitHub-Api-Version", "2022-11-28")
        for attempt in range(3):
            try:
                with urllib.request.urlopen(request) as response:
                    content = response.read()
                    return json.loads(content) if content else None
            except urllib.error.HTTPError as error:
                if error.code < 500 or attempt == 2:
                    raise
            time.sleep(2 * (attempt + 1))

    def paged(self, url: str) -> list:
        results, page = [], 1
        while True:
            separator = "&" if "?" in url else "?"
            batch = self.request("GET", f"{url}{separator}per_page=100&page={page}")
            results.extend(batch)
            if len(batch) < 100:
                return results
            page += 1


def load_pull_request(github: GitHub, number: int) -> tuple[dict, PullRequest]:
    raw = github.request("GET", f"/pulls/{number}")
    files = [
        ChangedFile(item["filename"], item.get("additions", 0), item.get("deletions", 0), item.get("patch", ""))
        for item in github.paged(f"/pulls/{number}/files")
    ]
    body = raw.get("body") or ""
    issue_labels: dict[int, list[str]] = {}
    for issue_number in linked_issue_numbers(body):
        try:
            issue = github.request("GET", f"/issues/{issue_number}")
            issue_labels[issue_number] = [label["name"] for label in issue.get("labels", [])]
        except urllib.error.HTTPError:
            issue_labels[issue_number] = []
    labels = [label["name"] for label in raw.get("labels", [])]
    return raw, PullRequest(raw["title"], body, labels, files, issue_labels)


def is_exempt(raw: dict, labels: list[str]) -> bool:
    return LABEL_SKIP in labels or raw["user"]["type"] == "Bot" or raw.get("author_association") in EXEMPT_ASSOCIATIONS


def upsert_comment(github: GitHub, number: int, text: str) -> None:
    for comment in github.paged(f"/issues/{number}/comments"):
        if COMMENT_MARKER in (comment.get("body") or "") and comment["user"]["type"] == "Bot":
            if comment["body"] != text:
                github.request("PATCH", f"/issues/comments/{comment['id']}", {"body": text})
            return
    github.request("POST", f"/issues/{number}/comments", {"body": text})


def add_label(github: GitHub, number: int, labels: list[str], name: str) -> None:
    if name not in labels:
        github.request("POST", f"/issues/{number}/labels", {"labels": [name]})


def remove_label(github: GitHub, number: int, labels: list[str], name: str) -> None:
    if name not in labels:
        return
    try:
        github.request("DELETE", f"/issues/{number}/labels/{urllib.parse.quote(name)}")
    except urllib.error.HTTPError as error:
        if error.code != 404:
            raise


def update_labels(github: GitHub, number: int, labels: list[str], ready: bool) -> None:
    """Sets only the gate's own labels (needs info, needs review); the maintainers' status labels stay as they are."""
    if not ready:
        remove_label(github, number, labels, LABEL_NEEDS_REVIEW)
        add_label(github, number, labels, LABEL_NEEDS_INFO)
        return
    remove_label(github, number, labels, LABEL_NEEDS_INFO)
    if not any(label.startswith(STATUS_LABEL_PREFIX) and label != LABEL_NEEDS_INFO for label in labels):
        add_label(github, number, labels, LABEL_NEEDS_REVIEW)


def convert_to_draft(github: GitHub, node_id: str) -> None:
    query = "mutation($id: ID!) { convertPullRequestToDraft(input: {pullRequestId: $id}) { clientMutationId } }"
    try:
        github.request("POST", "https://api.github.com/graphql", {"query": query, "variables": {"id": node_id}})
    except urllib.error.HTTPError as error:
        print(f"::warning::could not convert to draft: {error}")


def run_gate(github: GitHub, number: int, enforce: bool) -> bool:
    raw, pull_request = load_pull_request(github, number)
    if raw["state"] != "open":
        return True
    if is_exempt(raw, pull_request.labels):
        print(f"#{number}: exempt")
        return True
    is_draft = bool(raw.get("draft"))
    if is_draft and enforce and LABEL_NEEDS_INFO not in pull_request.labels:
        print(f"#{number}: draft, gate runs when it is marked ready for review")
        return True
    missing = check(pull_request)
    upsert_comment(github, number, render_comment(missing))
    update_labels(github, number, pull_request.labels, ready=not missing)
    if missing and enforce and not is_draft:
        convert_to_draft(github, raw["node_id"])
    print(f"#{number}: {'ready' if not missing else 'missing ' + str(len(missing))}")
    for item in missing:
        print(f"  - {item}")
    return is_draft or not missing


def dry_run(github: GitHub, number: int) -> int:
    _, pull_request = load_pull_request(github, number)
    missing = check(pull_request)
    if not missing:
        print(f"#{number}: ready for review")
        return 0
    print(f"#{number}: not ready, missing:")
    for item in missing:
        print(f"- {item.replace('**', '')}")
    return 1


def main() -> int:
    repository = os.environ.get("GITHUB_REPOSITORY") or DEFAULT_REPOSITORY
    github = GitHub(repository, os.environ.get("GITHUB_TOKEN", ""))
    if len(sys.argv) > 1:
        number_argument = re.search(r"(\d+)$", sys.argv[2]) if len(sys.argv) == 3 and sys.argv[1] == "--dry-run" else None
        if not number_argument:
            print("usage: python3 .github/scripts/pr_gate.py --dry-run <PR number or URL>", file=sys.stderr)
            return 2
        return dry_run(github, int(number_argument.group(1)))
    if os.environ.get("GITHUB_ACTIONS") != "true":
        print("Writes to GitHub only run in CI. Locally use --dry-run <PR number>.", file=sys.stderr)
        return 2
    number = os.environ.get("PR_NUMBER", "").strip()
    if number:
        return 0 if run_gate(github, int(number), enforce=True) else 1
    failed = []
    for raw in github.paged("/pulls?state=open"):
        try:
            run_gate(github, raw["number"], enforce=False)
        except urllib.error.HTTPError as error:
            print(f"::warning::#{raw['number']}: {error}")
            failed.append(raw["number"])
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
