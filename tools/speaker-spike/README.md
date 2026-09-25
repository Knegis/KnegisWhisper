# Speaker-recognition spike scripts (throwaway)

The two scripts behind `docs/SPEAKER_RECOGNITION_SPIKE.md` (KNEG-319, 2026-09-25). They are
not part of the product and nothing imports them; they are kept so the measurement can be
re-run rather than trusted.

- `speaker_spike.py` — route 1 on transcription segments, and route 2 (prepend a reference clip,
  run the shipped diarisation CLI).
- `spike2.py` — route 1 on diarisation turns, with a control set of unrelated voices.

Run from a scratch directory (they write their reports beside themselves), read-only against
the application's database and recordings under `%APPDATA%\KnegisWhisper`:

```
python -m venv venv
venv\Scripts\pip install sherpa-onnx==1.13.8 numpy
copy <repo>\tools\speaker-spike\*.py .
venv\Scripts\python speaker_spike.py
venv\Scripts\python spike2.py
```

Needs Python 3.14 and the two diarisation models installed by Settings > Meetings > Speaker
labels.
