"""Calibrate/freeze, then evaluate labeled production embeddings. Standard library only.

Manifest rows: {samples_file, speaker_id, person, session, day, split, language,
condition}. split is enroll/calibrate/test. An unknown person's person is still
a unique label, never null. Same-session rows must belong to one split.
"""
import argparse
import hashlib
import itertools
import json
import math
from pathlib import Path


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def normalize(v):
    if len(v) != 512 or not all(math.isfinite(x) for x in v):
        raise ValueError("invalid embedding")
    norm = math.sqrt(sum(x*x for x in v))
    if norm <= 1e-12:
        raise ValueError("zero embedding")
    return [x/norm for x in v]


def load(path, splits):
    manifest = json.loads(path.read_text(encoding="utf-8-sig"))
    sessions, seen, rows, hashes = {}, set(), [], {}
    for row in manifest:
        session = row["session"]
        group = (row["split"], row["day"])
        if sessions.setdefault(session, group) != group:
            raise ValueError("session overlaps splits or days")
        if row["split"] not in {"enroll", "calibrate", "test"}:
            raise ValueError("unknown split")
        expected = row["samples_sha256"]
        if len(expected) != 64 or any(c not in "0123456789abcdef" for c in expected):
            raise ValueError("Each manifest row needs its frozen samples_sha256")
        if row["split"] not in splits:
            continue  # calibration never reads reserved embedding files
        file = (path.parent / row["samples_file"]).resolve()
        key = (str(file), row["speaker_id"])
        if key in seen:
            raise ValueError("sample reused across rows")
        seen.add(key)
        data = json.loads(file.read_text(encoding="utf-8-sig"))
        hashes[str(file)] = digest(file)
        if hashes[str(file)] != expected:
            raise ValueError("Embedding file changed from its manifest hash")
        sample = next((s for s in data["samples"] if s["speaker_id"] == row["speaker_id"]), None)
        rows.append({**row, "vector": normalize(sample["embedding"]) if sample else None,
                     "seconds": sample["seconds"] if sample else 0,
                     "model": data["model_id"], "format": data["format"]})
    identities = {(r["model"], r["format"]) for r in rows}
    if len(identities) != 1:
        raise ValueError("mixed or missing model identities")
    if any(not math.isfinite(r["seconds"]) or r["seconds"] < 0 for r in rows):
        raise ValueError("invalid speech duration")
    return rows, hashes


def score(rows, policy, split):
    profiles = {}
    for row in rows:
        if row["split"] == "enroll":
            if row["person"] in profiles:
                raise ValueError("choose exactly one explicit reference per person")
            if row["vector"] is None or row["seconds"] < policy["min_seconds"]:
                raise ValueError("enrollment sample is too short")
            profiles[row["person"]] = row["vector"]
    result = dict(correct=0, wrong=0, unknown_accepted=0, eligible_returning=0,
                  excluded=0, total=0, unknown_total=0, returning_total=0)
    details=[]
    for row in rows:
        if row["split"] != split:
            continue
        result["total"] += 1
        known = row["person"] in profiles
        result["returning_total" if known else "unknown_total"] += 1
        predicted = None
        eligible = row["vector"] is not None and row["seconds"] >= policy["min_seconds"]
        if not eligible:
            result["excluded"] += 1
        else:
            if known:
                result["eligible_returning"] += 1
            scores = sorted(((sum(a*b for a,b in zip(row["vector"],v)),p) for p,v in profiles.items()), reverse=True)
            if scores and scores[0][0] >= policy["threshold"] and scores[0][0] - (scores[1][0] if len(scores)>1 else -1) >= policy["margin"]:
                predicted=scores[0][1]
            if predicted == row["person"]:
                result["correct"] += 1
            elif predicted is not None:
                result["wrong" if known else "unknown_accepted"] += 1
        details.append({"session":row["session"],"speaker":row["speaker_id"],"person":row["person"],"predicted":predicted,"eligible":eligible})
    result["recall"] = result["correct"]/result["eligible_returning"] if result["eligible_returning"] else 0
    result["passed"] = result["wrong"] == result["unknown_accepted"] == 0 and result["recall"] >= .8 and result["unknown_total"] >= 3
    result["details"] = details
    return result


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument("phase",choices=["calibrate","evaluate"])
    p.add_argument("manifest",type=Path)
    p.add_argument("policy",type=Path)
    p.add_argument("--report",type=Path)
    a=p.parse_args()
    if a.phase == "calibrate":
        rows,hashes=load(a.manifest,{"enroll","calibrate"})
        candidates=[]
        for threshold,margin,seconds in itertools.product([.8,.85,.9,.92,.95],[.05,.1,.15],[3.,5.,10.]):
            policy=dict(threshold=threshold,margin=margin,min_seconds=seconds)
            try:
                report=score(rows,policy,"calibrate")
            except ValueError:
                continue
            if report["wrong"] == report["unknown_accepted"] == 0:
                candidates.append((report["correct"],-report["excluded"],threshold,margin,-seconds,policy,report))
        if not candidates:
            raise ValueError("No safe calibration candidate; collect new development recordings")
        *_,policy,report=max(candidates,key=lambda c:c[:5])
        frozen=dict(policy=policy,manifest_sha256=digest(a.manifest),inputs=hashes,model=rows[0]["model"],format=rows[0]["format"],calibration=report)
        with a.policy.open("x",encoding="utf-8") as f:
            json.dump(frozen,f,indent=2)
        print("Frozen policy written; no held-out embeddings were read.")
    else:
        frozen=json.loads(a.policy.read_text(encoding="utf-8-sig"))
        if digest(a.manifest) != frozen["manifest_sha256"]:
            raise ValueError("Manifest changed after calibration")
        rows,hashes=load(a.manifest,{"enroll","calibrate","test"})
        if any(hashes.get(path) != value for path,value in frozen["inputs"].items()):
            raise ValueError("Calibration inputs changed")
        enrolled={r["person"] for r in rows if r["split"]=="enroll"}
        if len(enrolled)<3:
            raise ValueError("Need at least three enrolled people")
        for person in enrolled:
            person_rows=[r for r in rows if r["person"]==person]
            days=[{r["day"] for r in person_rows if r["split"]==split} for split in ("enroll","calibrate","test")]
            if not all(days) or any(x & y for x,y in itertools.combinations(days,2)):
                raise ValueError("Each returning person needs separate days in all three splits")
        unknown={r["person"] for r in rows if r["split"]=="test" and r["person"] not in enrolled}
        if len(unknown)<3:
            raise ValueError("Need at least three held-out unenrolled people")
        report=score(rows,frozen["policy"],"test")
        report.update(policy_sha256=digest(a.policy),inputs=hashes,
                      release_approved=False, note="Measured dataset result only. Live microphone/room and packaged-app validation still required.")
        if not a.report:
            raise ValueError("--report is required")
        with a.report.open("x",encoding="utf-8") as f:
            json.dump(report,f,indent=2)
        print(json.dumps({k:v for k,v in report.items() if k not in {"details","inputs"}},indent=2))


if __name__ == "__main__":
    main()
