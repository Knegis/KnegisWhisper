"""Spike, second pass: embeddings on the diarisation turns rather than on
transcription segments, plus an unrelated-voice control set.

Pass 1 embedded the saved transcription segments, which the app assigns to a
speaker by overlapped duration and which can straddle a turn change. This pass
runs the same pyannote + CAM++ pipeline through the Python bindings at the
app's own tuning, cuts the audio on the turns it returns, and embeds those.

Control: sherpa's own bundled test WAVs (other people, other channels). If the
owner's voices are no more similar to each other than to these, the number is
the channel, not the person.
"""

import json
import os
import time
import wave
from pathlib import Path

import numpy as np
import sherpa_onnx

from speaker_spike import (APPDATA, CLUSTER_THRESHOLD, EMBEDDING_MODEL, HERE, MEETINGS, MIN_OFF, MIN_ON,
                           MIN_SEGMENT_SECONDS, RATE, SEGMENTATION_MODEL, Extractor, load_notes, read_wav,
                           unit, log)

CONTROLS = [
    APPDATA / "models/sherpa/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/test_wavs/en.wav",
    APPDATA / "models/sherpa/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/test_wavs/de.wav",
    APPDATA / "models/sherpa/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/test_wavs/fr.wav",
    APPDATA / "models/sherpa/sherpa-onnx-nemo-parakeet-unified-en-0.6b-int8-non-streaming/test_wavs/0.wav",
    APPDATA / "models/sherpa/sherpa-onnx-nemotron-speech-streaming-en-0.6b-560ms-int8-2026-04-25/test_wavs/1.wav",
    APPDATA / "models/sherpa/sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-560ms-int8-2026-06-11/test_wavs/uk.wav",
    APPDATA / "models/sherpa/sherpa-onnx-nemotron-3.5-asr-streaming-0.6b-560ms-int8-2026-06-11/test_wavs/ko.wav",
]


def read_any_wav(path: Path) -> np.ndarray:
    with wave.open(str(path)) as w:
        rate, ch, width = w.getframerate(), w.getnchannels(), w.getsampwidth()
        frames = w.readframes(w.getnframes())
    assert width == 2, path
    data = np.frombuffer(frames, dtype=np.int16).astype(np.float32) / 32768.0
    if ch > 1:
        data = data.reshape(-1, ch).mean(axis=1)
    if rate != RATE:
        # Linear resample; good enough for a control embedding.
        n = int(len(data) * RATE / rate)
        data = np.interp(np.linspace(0, len(data) - 1, n), np.arange(len(data)), data).astype(np.float32)
    return data


def diarizer():
    cfg = sherpa_onnx.OfflineSpeakerDiarizationConfig(
        segmentation=sherpa_onnx.OfflineSpeakerSegmentationModelConfig(
            pyannote=sherpa_onnx.OfflineSpeakerSegmentationPyannoteModelConfig(model=str(SEGMENTATION_MODEL)),
            num_threads=4),
        embedding=sherpa_onnx.SpeakerEmbeddingExtractorConfig(model=str(EMBEDDING_MODEL), num_threads=4),
        clustering=sherpa_onnx.FastClusteringConfig(num_clusters=-1, threshold=CLUSTER_THRESHOLD),
        min_duration_on=MIN_ON, min_duration_off=MIN_OFF)
    assert cfg.validate()
    return sherpa_onnx.OfflineSpeakerDiarization(cfg)


def main():
    notes = load_notes()
    ex = Extractor()
    sd = diarizer()
    voices = []
    lines = ["# Route 1, second pass: embeddings on diarisation turns (Python bindings, app tuning)", "",
             "| note | saved speakers (app CLI) | clusters (this run) | seconds | diarise s | embed s |",
             "| --- | --- | --- | --- | --- | --- |"]
    for n in notes:
        audio = read_wav(n["wav"])
        t0 = time.time()
        result = sd.process(audio).sort_by_start_time()
        t1 = time.time()
        per = {}
        for seg in result:
            if seg.end - seg.start < MIN_SEGMENT_SECONDS:
                continue
            a, b = int(seg.start * RATE), min(int(seg.end * RATE), len(audio))
            per.setdefault(seg.speaker, []).append((ex.embed(audio[a:b]), (b - a) / RATE))
        t2 = time.time()
        saved = len({s["speaker"] for s in n["segments"] if s.get("speaker")})
        lines.append(f"| n{n['id']} | {saved} | {len({s.speaker for s in result})} | {len(audio)/RATE:.0f} | {t1-t0:.1f} | {t2-t1:.1f} |")
        for spk, items in per.items():
            embs = np.stack([e for e, _ in items])
            mean = unit(embs.mean(axis=0))
            voices.append({"key": f"n{n['id']}/c{spk}", "note": n["id"], "cluster": spk, "embedding": mean,
                           "turns": len(items), "seconds": round(sum(d for _, d in items), 1),
                           "within": float(np.mean(embs @ mean)) if len(items) > 1 else None,
                           "turn_embeddings": embs})
        log(f"n{n['id']}: {len(per)} clusters, diarise {t1-t0:.1f}s, embed {t2-t1:.1f}s")
    controls = []
    for p in CONTROLS:
        if p.exists():
            a = read_any_wav(p)
            controls.append({"key": f"ctl/{p.parent.parent.name[:22]}/{p.name}", "embedding": ex.embed(a[: RATE * 20])})
    E = np.stack([v["embedding"] for v in voices])
    S = E @ E.T
    keys = [v["key"] for v in voices]
    lines += ["", "## Voices", "", "| voice | turns | seconds | within-cluster mean cos (turn vs centroid) |", "| --- | --- | --- | --- |"]
    for v in voices:
        lines.append(f"| {v['key']} | {v['turns']} | {v['seconds']} | {'' if v['within'] is None else f'{v['within']:.3f}'} |")
    # Separation the clusterer had: each turn's cos to own centroid vs best other centroid in the same note.
    lines += ["", "## Inside one recording: turn cos to own centroid vs to the nearest other cluster's centroid", "",
              "| note | turns | mean own | mean nearest other | turns closer to another cluster |", "| --- | --- | --- | --- | --- |"]
    for nid in sorted({v["note"] for v in voices}):
        vs = [v for v in voices if v["note"] == nid]
        if len(vs) < 2:
            continue
        own, other, wrong, count = [], [], 0, 0
        for v in vs:
            for t in v["turn_embeddings"]:
                o = float(t @ v["embedding"])
                best = max(float(t @ u["embedding"]) for u in vs if u is not v)
                own.append(o); other.append(best); count += 1
                if best > o:
                    wrong += 1
        lines.append(f"| n{nid} | {count} | {np.mean(own):.3f} | {np.mean(other):.3f} | {wrong} |")
    pairs = []
    for i in range(len(voices)):
        for j in range(i + 1, len(voices)):
            pairs.append((float(S[i, j]), keys[i], keys[j], voices[i]["note"] == voices[j]["note"]))
    cross = sorted([p for p in pairs if not p[3]], reverse=True)
    within = sorted([p for p in pairs if p[3]], reverse=True)
    lines += ["", "## Across recordings, top 30 pairs", "", "| cos | a | b |", "| --- | --- | --- |"]
    lines += [f"| {c:.3f} | {a} | {b} |" for c, a, b, _ in cross[:30]]
    lines += ["", "## Across recordings, bottom 10 pairs", "", "| cos | a | b |", "| --- | --- | --- |"]
    lines += [f"| {c:.3f} | {a} | {b} |" for c, a, b, _ in cross[-10:]]
    lines += ["", "## Different clusters inside one recording (what the clusterer separated)", "", "| cos | a | b |", "| --- | --- | --- |"]
    lines += [f"| {c:.3f} | {a} | {b} |" for c, a, b, _ in within]
    # Single-speaker sessions (one cluster) against each other: the presumed same voice across sessions.
    singles = [v for v in voices if sum(1 for u in voices if u["note"] == v["note"]) == 1]
    lines += ["", "## Recordings with exactly one cluster, against each other (presumed the same person at the same desk)", "",
              "| cos | a | b |", "| --- | --- | --- |"]
    for i in range(len(singles)):
        for j in range(i + 1, len(singles)):
            lines.append(f"| {float(singles[i]['embedding'] @ singles[j]['embedding']):.3f} | {singles[i]['key']} | {singles[j]['key']} |")
    # Controls.
    if controls:
        C = np.stack([c["embedding"] for c in controls])
        lines += ["", "## Control: unrelated voices (sherpa test WAVs) against every recording cluster", "",
                  "| control | max cos to any recording cluster | mean cos | mean cos to single-cluster recordings |", "| --- | --- | --- | --- |"]
        for c, row in zip(controls, C @ E.T):
            singles_idx = [keys.index(s["key"]) for s in singles]
            lines.append(f"| {c['key']} | {row.max():.3f} | {row.mean():.3f} | {np.mean(row[singles_idx]) if singles_idx else float('nan'):.3f} |")
        CC = C @ C.T
        lines += ["", f"Controls against each other: mean off-diagonal cos {np.mean(CC[np.triu_indices(len(controls), 1)]):.3f}, max {np.max(CC[np.triu_indices(len(controls), 1)]):.3f}"]
    (HERE / "route1b.md").write_text("\n".join(lines) + "\n", encoding="utf-8")
    log("route1b written")


if __name__ == "__main__":
    main()
