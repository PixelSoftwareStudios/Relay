import unittest

from packaging_fixtures import release_directory  # noqa: F401  (sets sys.path)
import verify_release_run as run

SHA = "a" * 40


def good():
    return ({"status": "completed", "head_sha": SHA, "path": ".github/workflows/release.yml",
             "event": "push"},
            [{"name": n, "status": "completed", "conclusion": "success"} for n in run.REQUIRED_JOBS])


class RunTests(unittest.TestCase):
    def test_complete_run_is_accepted(self):
        run.verify(*good(), SHA)

    def test_android_job_is_required(self):
        info, jobs = good()
        self.assertIn("Build Android APK", run.REQUIRED_JOBS)
        jobs[-1]["conclusion"] = "failure"
        with self.assertRaises(ValueError):
            run.verify(info, jobs, SHA)

    def test_wrong_commit_workflow_or_event_is_refused(self):
        info, jobs = good()
        for change in ({"head_sha": "b" * 40}, {"path": ".github/workflows/ci.yml"},
                       {"event": "pull_request"}, {"status": "in_progress"}):
            with self.assertRaises(ValueError):
                run.verify({**info, **change}, jobs, SHA)


if __name__ == "__main__":
    unittest.main()
