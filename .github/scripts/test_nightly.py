import datetime
import unittest

from nightly import nightly_tag, next_version, stale_nightlies

NOW = datetime.datetime(2026, 11, 27, 3, 5, tzinfo=datetime.timezone.utc)


class VersionTests(unittest.TestCase):
    def test_next_minor_of_the_newest_release(self):
        tags = ["v0.5.0", "v0.6.1", "v0.6.0", "v0.3.0-joe.9", "v0.7.0-nightly.202611260300"]
        self.assertEqual(next_version(tags), "0.7.0")
        self.assertEqual(nightly_tag(tags, NOW), "v0.7.0-nightly.202611270305")

    def test_minor_sorts_as_a_number(self):
        self.assertEqual(next_version(["v0.9.0", "v0.10.0"]), "0.11.0")

    def test_a_release_candidate_is_not_a_release(self):
        self.assertEqual(next_version(["v0.6.1", "v0.7.0-rc.1"]), "0.7.0")


class PruneTests(unittest.TestCase):
    def test_keeps_the_newest_nightlies_and_nothing_else_is_touched(self):
        nightlies = [f"v0.7.0-nightly.2026112{day}0300" for day in range(10)]
        tags = ["v0.6.1", "v0.7.0-rc.1", "v0.8.0-nightly.202611300300", *nightlies]
        stale = stale_nightlies(tags, keep=3)
        self.assertEqual(stale, nightlies[:8])


if __name__ == "__main__":
    unittest.main()
