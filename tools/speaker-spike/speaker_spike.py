"""Throwaway spike for KNEG-319: can a voice be recognised across recordings?

Route 1: per-speaker CAM++ embeddings (the same model the diarisation pass
clusters with), averaged over a speaker's saved segments, compared by cosine
similarity across notes.

Route 2: prepend a reference clip of a known voice to another recording and
run the shipped diarisation CLI once; does the clip's cluster coincide with
that voice's cluster in the recording?

Read-only against the application's database and recordings; every derived
file lands in this directory. Not part of the product.
"""

import json
import os
import subprocess
import sys
import time
import wave
from pathlib import Path

import numpy as np
import sherpa_onnx

HERE = Path(__file__).resolve().parent
APPDATA = Path(os.environ["APPDATA"]) / "KnegisWhisper"
DB = APPDATA / "knegiswhisper.db"
MEETINGS = APPDATA / "meetings"
EMBEDDING_MODEL = APPDATA / "models" / "diarization" / "3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx"
SEGMENTATION_MODEL = APPDATA / "models" / "diarization" / "sherpa-onnx-pyannote-segmentation-3-0" / "model.onnx"
DIARIZE_EXE = APPDATA / "sidecars" / "sherpa" / "sherpa-onnx-offline-speaker-diarization.exe"
RATE = 16000
MIN_SEGMENT_SECONDS = 1.0
REFERENCE_SECONDS = 8.0
GAP_SECONDS = 0.5
# The application's own Tuning defaults (crates/meeting-core/src/diarization.rs).
CLUSTER_THRESHOLD = 0.5
MIN_ON = 0.3
MIN_OFF = 0.5


def log(*a):
    print(*a, flush=True)


def load_notes():
    import sqlite3
    con = sqlite3.connect(f"file:{DB}?mode=ro", uri=True)
    rows = con.execute(
        "select n.id, n.title, n.participants, r.storage_key, n.transcript "
        "from notes n join note_recordings r on r.note_id = n.id "
        "where n.transcript is not null and n.transcript != '' order by n.id"
    ).fetchall()
    notes = []
    for id, title, participants, key, tr in rows:
        segs = json.loads(tr)
        spk = [s for s in segs if s.get("speaker")]
        if not spk:
            continue
        wav = MEETINGS / key
        if not wav.exists():
            continue
        notes.append({"id": id, "title": title, "wav": wav, "segments": segs})
    return notes


def read_wav(path: Path) -> np.ndarray:
    with wave.open(str(path)) as w:
        assert w.getframerate() == RATE, (path, w.getframerate())
        assert w.getsampwidth() == 2
        frames = w.readframes(w.getnframes())
        data = np.frombuffer(frames, dtype=np.int16).astype(np.float32) / 32768.0
        if w.getnchannels() > 1:
            data = data.reshape(-1, w.getnchannels()).mean(axis=1)
    return data


def write_wav(path: Path, samples: np.ndarray):
    with wave.open(str(path), "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(RATE)
        w.writeframes((np.clip(samples, -1, 1) * 32767).astype(np.int16).tobytes())


def seconds(v):
    # Meeting segments from the whole-file pass carry seconds; epoch ms would be > 1e9.
    return float(v) / 1000.0 if v is not None and v > 1e9 else v


def spans(segments):
    """(speaker, start, end) for each saved segment with a speaker label."""
    out = []
    for i, s in enumerate(segments):
        if not s.get("speaker"):
            continue
        start = seconds(s.get("timestamp"))
        end = seconds(s.get("endTimestamp"))
        if start is None or end is None or end <= start:
            continue
        out.append((s["speaker"], start, end))
    return out


def unit(v):
    n = np.linalg.norm(v)
    return v / n if n > 0 else v


class Extractor:
    def __init__(self):
        cfg = sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=str(EMBEDDING_MODEL), num_threads=4, provider="cpu")
        assert cfg.validate(), "embedding config invalid"
        self.ex = sherpa_onnx.SpeakerEmbeddingExtractor(cfg)

    def embed(self, samples: np.ndarray) -> np.ndarray:
        stream = self.ex.create_stream()
        stream.accept_waveform(sample_rate=RATE, waveform=samples)
        stream.input_finished()
        assert self.ex.is_ready(stream)
        return unit(np.array(self.ex.compute(stream), dtype=np.float32))


def route1(notes):
    ex = Extractor()
    log(f"embedding dim: {ex.ex.dim}")
    voices = []  # {note, speaker, embedding, seconds, n, within}
    t0 = time.time()
    for n in notes:
        audio = read_wav(n["wav"])
        per = {}
        for spk, start, end in spans(n["segments"]):
            if end - start < MIN_SEGMENT_SECONDS:
                continue
            a, b = int(start * RATE), min(int(end * RATE), len(audio))
            if b - a < int(MIN_SEGMENT_SECONDS * RATE):
                continue
            per.setdefault(spk, []).append((ex.embed(audio[a:b]), (b - a) / RATE))
        for spk, items in per.items():
            embs = np.stack([e for e, _ in items])
            secs = sum(d for _, d in items)
            mean = unit(embs.mean(axis=0))
            within = float(np.mean(embs @ mean)) if len(items) > 1 else None
            voices.append({"note": n["id"], "title": n["title"], "speaker": spk, "embedding": mean,
                           "seconds": round(secs, 1), "n": len(items), "within": within})
    log(f"route 1: {len(voices)} (note, speaker) voices from {len(notes)} notes in {time.time() - t0:.1f}s")
    return voices


def route1_report(voices, out):
    E = np.stack([v["embedding"] for v in voices])
    S = E @ E.T
    labels = [f"n{v['note']}/{v['speaker']}" for v in voices]
    lines = ["# Route 1: CAM++ mean embedding per (note, speaker), cosine similarity", ""]
    lines.append("| voice | title | segments | seconds | within-speaker mean cos |")
    lines.append("| --- | --- | --- | --- | --- |")
    for v in voices:
        w = "" if v["within"] is None else f"{v['within']:.3f}"
        lines.append(f"| n{v['note']}/{v['speaker']} | {v['title'][:32]} | {v['n']} | {v['seconds']} | {w} |")
    lines += ["", "## Pairwise similarity (across notes only, sorted)", ""]
    pairs = []
    for i in range(len(voices)):
        for j in range(i + 1, len(voices)):
            same_note = voices[i]["note"] == voices[j]["note"]
            pairs.append((float(S[i, j]), labels[i], labels[j], same_note))
    cross = sorted([p for p in pairs if not p[3]], reverse=True)
    within_note = sorted([p for p in pairs if p[3]], reverse=True)
    lines.append("| cos | a | b |")
    lines.append("| --- | --- | --- |")
    for c, a, b, _ in cross[:40]:
        lines.append(f"| {c:.3f} | {a} | {b} |")
    lines += ["", "## Different speakers inside one note (should be low: these are what clustering separated)", ""]
    lines.append("| cos | a | b |")
    lines.append("| --- | --- | --- |")
    for c, a, b, _ in within_note:
        lines.append(f"| {c:.3f} | {a} | {b} |")
    # Identity grouping at thresholds, greedy, in note order.
    lines += ["", "## Greedy identity grouping across notes at a threshold (first match wins)", ""]
    lines.append("| threshold | identities formed | voices merged into an earlier identity |")
    lines.append("| --- | --- | --- |")
    for th in (0.40, 0.45, 0.50, 0.55, 0.60, 0.65, 0.70, 0.75):
        ids = []  # list of (centroid, members)
        merged = 0
        for i, v in enumerate(voices):
            best, bi = -1.0, None
            for k, (c, members) in enumerate(ids):
                s = float(v["embedding"] @ c)
                if s > best:
                    best, bi = s, k
            if bi is not None and best >= th:
                c, members = ids[bi]
                members.append(labels[i])
                ids[bi] = (unit(np.mean([voices[m]["embedding"] for m in range(len(voices)) if labels[m] in members], axis=0)), members)
                merged += 1
            else:
                ids.append((v["embedding"], [labels[i]]))
        lines.append(f"| {th:.2f} | {len(ids)} | {merged} |")
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")
    np.save(HERE / "voices.npy", E)
    (HERE / "voices.json").write_text(json.dumps([{k: v for k, v in x.items() if k != "embedding"} for x in voices], indent=1), encoding="utf-8")
    return S, labels


def run_diarize(wav: Path):
    args = [str(DIARIZE_EXE),
            f"--segmentation.pyannote-model={SEGMENTATION_MODEL}",
            f"--embedding.model={EMBEDDING_MODEL}",
            f"--clustering.cluster-threshold={CLUSTER_THRESHOLD}",
            f"--min-duration-on={MIN_ON}",
            f"--min-duration-off={MIN_OFF}",
            str(wav)]
    t0 = time.time()
    p = subprocess.run(args, capture_output=True, text=True)
    dt = time.time() - t0
    turns = []
    for line in p.stdout.splitlines():
        parts = line.split()
        if len(parts) == 4 and parts[1] == "--":
            turns.append((float(parts[0]), float(parts[2]), parts[3]))
    return turns, dt, p.returncode


def overlap(a0, a1, b0, b1):
    return max(0.0, min(a1, b1) - max(a0, b0))


def reference_clip(note, audio, speaker):
    """Up to REFERENCE_SECONDS of one speaker's speech, concatenated in order."""
    pieces, total = [], 0.0
    for spk, start, end in spans(note["segments"]):
        if spk != speaker or end - start < MIN_SEGMENT_SECONDS:
            continue
        a, b = int(start * RATE), min(int(end * RATE), len(audio))
        take = min(b - a, int((REFERENCE_SECONDS - total) * RATE))
        if take <= 0:
            break
        pieces.append(audio[a:a + take])
        total += take / RATE
        if total >= REFERENCE_SECONDS:
            break
    return (np.concatenate(pieces) if pieces else np.zeros(0, dtype=np.float32)), total


def route2(notes, voices, S, labels, out):
    """Prepend a reference clip from note A's speaker to note B; which cluster does it land in?"""
    by_key = {f"n{v['note']}/{v['speaker']}": i for i, v in enumerate(voices)}
    by_id = {n["id"]: n for n in notes}
    lines = ["# Route 2: prepend an 8 s reference clip and run the shipped diarisation CLI once", "",
             "The reference voice's cluster in B is *predicted by route 1* (the B voice most similar to the "
             "reference embedding) because nobody has labelled who is who in these recordings.", "",
             "| A (reference) | B (recording) | B clusters before | B clusters after | ref cluster | route-1 best B voice (cos) | ref cluster == that voice's cluster | seconds |",
             "| --- | --- | --- | --- | --- | --- | --- | --- |"]
    # Pick reference voices: the most-spoken voice of each single-speaker note, plus each multi-speaker note's main voice.
    refs = []
    for n in notes:
        cands = [v for v in voices if v["note"] == n["id"]]
        if not cands:
            continue
        main = max(cands, key=lambda v: v["seconds"])
        if main["seconds"] >= REFERENCE_SECONDS:
            refs.append(main)
    targets = [n for n in notes if len({v["speaker"] for v in voices if v["note"] == n["id"]}) >= 2]
    results = []
    budget = 0
    for ref in refs:
        A = by_id[ref["note"]]
        a_audio = read_wav(A["wav"])
        clip, secs = reference_clip(A, a_audio, ref["speaker"])
        if secs < REFERENCE_SECONDS * 0.75:
            continue
        for B in targets:
            if B["id"] == A["id"]:
                continue
            if budget >= 12:
                break
            budget += 1
            b_audio = read_wav(B["wav"])
            offset = len(clip) / RATE + GAP_SECONDS
            combined = np.concatenate([clip, np.zeros(int(GAP_SECONDS * RATE), dtype=np.float32), b_audio])
            tmp = HERE / f"combo-n{A['id']}-{ref['speaker']}-to-n{B['id']}.wav"
            write_wav(tmp, combined)
            turns, dt, rc = run_diarize(tmp)
            before = len({v["speaker"] for v in voices if v["note"] == B["id"]})
            after = len({t[2] for t in turns})
            # Cluster covering the reference span.
            cover = {}
            for s, e, c in turns:
                cover[c] = cover.get(c, 0.0) + overlap(s, e, 0.0, secs)
            ref_cluster = max(cover, key=cover.get) if cover else None
            # Which of B's saved speakers does each cluster overlap most (after offset)?
            best_label, best_cos = None, -1.0
            ri = by_key[f"n{ref['note']}/{ref['speaker']}"]
            for v in voices:
                if v["note"] != B["id"]:
                    continue
                c = float(S[ri, by_key[f"n{v['note']}/{v['speaker']}"]])
                if c > best_cos:
                    best_cos, best_label = c, v["speaker"]
            # Cluster for that saved speaker: the cluster with most overlap with its spans.
            spk_cover = {}
            for spk, s0, s1 in spans(B["segments"]):
                if spk != best_label:
                    continue
                for s, e, c in turns:
                    spk_cover[c] = spk_cover.get(c, 0.0) + overlap(s, e, s0 + offset, s1 + offset)
            voice_cluster = max(spk_cover, key=spk_cover.get) if spk_cover else None
            match = (ref_cluster is not None and ref_cluster == voice_cluster)
            results.append({"A": ref["note"], "ref": ref["speaker"], "B": B["id"], "before": before, "after": after,
                            "ref_cluster": ref_cluster, "best_voice": best_label, "best_cos": round(best_cos, 3),
                            "voice_cluster": voice_cluster, "match": match, "seconds": round(dt, 1), "rc": rc})
            lines.append(f"| n{ref['note']}/{ref['speaker']} | n{B['id']} | {before} | {after} | {ref_cluster} | {best_label} ({best_cos:.3f}) | {'yes' if match else 'no'} | {dt:.1f} |")
            log(f"route 2: n{ref['note']}/{ref['speaker']} -> n{B['id']}: clusters {before}->{after}, ref in {ref_cluster}, best voice {best_label} ({best_cos:.3f}) in {voice_cluster}, match={match}, {dt:.1f}s")
            tmp.unlink(missing_ok=True)
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")
    (HERE / "route2.json").write_text(json.dumps(results, indent=1), encoding="utf-8")


def main():
    notes = load_notes()
    log("diarised notes with a recording on disk:", [(n["id"], n["title"][:24]) for n in notes])
    voices = route1(notes)
    S, labels = route1_report(voices, HERE / "route1.md")
    log("route 1 report written")
    if "--skip-route2" not in sys.argv:
        route2(notes, voices, S, labels, HERE / "route2.md")
        log("route 2 report written")


if __name__ == "__main__":
    main()
