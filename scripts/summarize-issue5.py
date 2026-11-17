#!/usr/bin/env python3
"""Generate the bounded issue-5 evidence summary exclusively from retained raw runs."""
import json
import pathlib
import statistics

root = pathlib.Path("bench/results/2026-11-17-issue5")
summary = {
    "schemaVersion": 1,
    "inputCommit": "430261138aa08539cd598b9269bfe8f82fd49b78",
    "reportedTwoMinuteBaselineReproduced": False,
    "scope": "warm-cache shared Mac; exact input commit and full-history object store",
    "criteria": {"maximumMedianCreateSlowdownVsGit": 2.0, "wrongCheckouts": 0, "sourceMutations": 0},
    "environments": {},
}
verified = 0
for environment, suffix in [("host", "host"), ("case-sensitive-image", "images")]:
    before = json.loads((root / f"before-{suffix}.json").read_text())
    after = json.loads((root / f"after-{suffix}.json").read_text())
    for run in [before, after]:
        assert run["dirty"] is False and not run["failures"]
        for cohort in run["cohorts"]:
            assert cohort["failed"] == 0 and cohort["sourceUnchanged"] and cohort["privateWriteTested"]
            assert all(op["exitCode"] == 0 and op["verified"] and op["error"] is None
                       and (cohort["backend"] == "git" or op["nativeCow"])
                       for op in cohort["operations"])
            verified += cohort["succeeded"]
    def median(run, backend, field):
        values = [c[field] for c in run["cohorts"] if c["backend"] == backend]
        assert len(values) == 3
        return statistics.median(values)
    git_ms = median(after, "git", "createWallMs")
    new_ms = median(after, "twig", "createWallMs")
    ratio = new_ms / git_ms
    assert ratio <= summary["criteria"]["maximumMedianCreateSlowdownVsGit"]
    result = {
        "samplesPerArm": 3,
        "stockGitMedianCreateMs": git_ms,
        "beforeTwigMedianCreateMs": median(before, "twig", "createWallMs"),
        "afterTwigMedianCreateMs": new_ms,
        "afterVsGitMedianCreateRatio": ratio,
        "criterionPassed": True,
    }
    if suffix == "images":
        result["stockGitMedianAllocationBytes"] = median(after, "git", "growthAfterStatusBytes")
        result["afterTwigMedianAllocationBytes"] = median(after, "twig", "growthAfterStatusBytes")
    summary["environments"][environment] = result
summary["verifiedMeasuredCreates"] = verified
assert verified == 18
(root / "summary.json").write_text(json.dumps(summary, indent=2) + "\n")
print(json.dumps(summary, indent=2))
