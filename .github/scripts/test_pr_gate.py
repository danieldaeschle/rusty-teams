import unittest

from pr_gate import LABEL_ACCEPTED, LABEL_NO_VISUAL, ChangedFile, PullRequest, check, linked_issue_numbers, parse_sections

GOOD_BODY = """## Summary

A sent image no longer blinks after sending.

## Problem

<!-- template hint -->
After sending an image the bubble shows empty for 200 ms and then the image again.
Expected: the image stays visible the whole time.

## AI model

Claude Code, Opus 5.5

## Tests

Ran: `cargo test -p app image_cache`. Not run: Windows.

## Review

- `/code-review medium`: 1 finding, fixed
- fresh agent session: no findings
"""

BACKEND_FILE = ChangedFile("crates/chatsvc/src/window.rs", 20, 5)
UI_FILE = ChangedFile("crates/app/src/views/conversation.rs", 10, 2)


def make(title="fix(chat): a sent image no longer blinks", body=GOOD_BODY, files=None, labels=None, issue_labels=None):
    return PullRequest(title, body, labels or [], files if files is not None else [BACKEND_FILE], issue_labels or {})


def missing_keys(pull_request):
    return [item.split("**")[1] for item in check(pull_request)]


class SectionTests(unittest.TestCase):
    def test_comments_do_not_count_as_content(self):
        sections = parse_sections("## Summary\n<!-- what and why -->\n## AI model\nNone")
        self.assertEqual(sections["summary"], "")
        self.assertEqual(sections["ai model"], "None")

    def test_test_plan_counts_as_tests(self):
        self.assertEqual(check(make(body=GOOD_BODY.replace("## Tests", "## Test plan"))), [])

    def test_issue_reference_only_with_closing_keyword(self):
        self.assertEqual(linked_issue_numbers("Closes #12, fixes #3, see #99"), [3, 12])
        self.assertEqual(linked_issue_numbers("<!-- Closes #123 -->"), [])
        self.assertEqual(linked_issue_numbers("Fixes: https://github.com/danieldaeschle/rusty-teams/issues/42"), [42])


class GateTests(unittest.TestCase):
    def test_good_pr_passes(self):
        self.assertEqual(check(make()), [])

    def test_bad_title(self):
        self.assertIn("Title", missing_keys(make(title="Bound audio capture lifetime")))

    def test_untouched_template_fails_every_section(self):
        template = "## Summary\n<!-- x -->\n## AI model\n<!-- x -->\n## Review\n<!-- x -->"
        keys = missing_keys(make(body=template))
        for name in ("Summary", "AI model", "Tests", "Review", "Problem"):
            self.assertIn(name, keys)

    def test_issue_link_replaces_problem(self):
        body = GOOD_BODY.replace("## Problem", "Closes #5\n\n## Notes")
        self.assertEqual(check(make(body=body)), [])

    def test_feature_needs_accepted_issue(self):
        title = "feat(chat): pin a message"
        self.assertIn("Issue first", missing_keys(make(title=title)))
        body = GOOD_BODY + "\nCloses #7\n"
        self.assertIn("Issue first", missing_keys(make(title=title, body=body, issue_labels={7: ["enhancement"]})))
        self.assertEqual(check(make(title=title, body=body, issue_labels={7: [LABEL_ACCEPTED]})), [])

    def test_large_fix_needs_accepted_issue_but_lockfile_does_not_count(self):
        large = [ChangedFile("crates/chatsvc/src/window.rs", 350, 100)]
        self.assertIn("Issue first", missing_keys(make(files=large)))
        lock = [BACKEND_FILE, ChangedFile("Cargo.lock", 900, 900)]
        self.assertEqual(check(make(files=lock)), [])

    def test_ui_change_needs_screenshot(self):
        self.assertIn("Screenshot", missing_keys(make(files=[UI_FILE])))
        with_image = GOOD_BODY + "\n## Screenshot\n\n![chat](https://github.com/user-attachments/assets/abc)\n"
        self.assertEqual(check(make(body=with_image, files=[UI_FILE])), [])

    def test_ui_change_without_visible_change(self):
        ticked = GOOD_BODY + "\n## Screenshot\n\n- [x] No visible change: only the cache key moved\n"
        self.assertEqual(check(make(body=ticked, files=[UI_FILE])), [])
        self.assertEqual(check(make(files=[UI_FILE], labels=[LABEL_NO_VISUAL])), [])

    def test_windows_code_needs_windows_note(self):
        windows_file = ChangedFile("crates/calling/src/device.rs", 5, 1, "@@\n+#[cfg(windows)]\n+fn open() {}")
        body = GOOD_BODY.replace("Not run: Windows.", "")
        self.assertIn("Tests", missing_keys(make(body=body, files=[windows_file])))
        self.assertEqual(check(make(files=[windows_file])), [])

    def test_gpui_window_file_is_not_windows_code(self):
        body = GOOD_BODY.replace("Not run: Windows.", "")
        self.assertEqual(check(make(body=body, files=[ChangedFile("crates/chatsvc/src/window.rs", 1, 1)])), [])

    def test_single_review_line_is_not_enough(self):
        body = GOOD_BODY.replace("- fresh agent session: no findings\n", "")
        self.assertIn("Review", missing_keys(make(body=body)))

    def test_planning_folder_and_vendor(self):
        files = [BACKEND_FILE, ChangedFile("openspec/changes/x.md", 10, 0), ChangedFile("vendor/gpui/src/a.rs", 1, 1)]
        self.assertEqual(missing_keys(make(files=files)), ["Scope", "Scope"])


if __name__ == "__main__":
    unittest.main()
