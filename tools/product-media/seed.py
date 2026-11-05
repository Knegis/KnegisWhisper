"""Seed only a fresh, disposable debug profile under this checkout's target/."""
import argparse
import json
import sqlite3
import subprocess
import wave
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--profile", required=True, type=Path)
args = parser.parse_args()
root = Path(__file__).resolve().parents[2]
profile = args.profile.resolve()
if not profile.is_relative_to(root / "target"):
    raise SystemExit("Profile must be inside this checkout's target directory")
db = profile / "knegiswhisper.db"
if not db.exists():
    raise SystemExit("Launch the native debug app once with this isolated profile first")
connection = sqlite3.connect(db)
if connection.execute("SELECT COUNT(*) FROM notes").fetchone()[0] or connection.execute("SELECT COUNT(*) FROM transcriptions").fetchone()[0]:
    raise SystemExit("Refusing to modify a profile that already contains content")
recording = profile / "meetings" / "example.wav"
if recording.exists():
    raise SystemExit("Refusing to replace an existing recording")
recording.parent.mkdir(exist_ok=True)
speech = ("Let us start with a Windows preview and make the first dictation feel simple. "
          "Agreed. People should be able to choose local processing and keep their own notes. "
          "Before we share it, record the introduction and test the installer on a clean machine.")
quote = lambda value: "'" + str(value).replace("'", "''") + "'"
script = ("Add-Type -AssemblyName System.Speech; $speaker = New-Object System.Speech.Synthesis.SpeechSynthesizer; "
          "$speaker.Rate=2; $speaker.SetOutputToWaveFile(" + quote(recording) + "); "
          "$speaker.Speak(" + quote(speech) + "); $speaker.Dispose()")
subprocess.run(["powershell.exe", "-NoProfile", "-Command", script], check=True,
               creationflags=subprocess.CREATE_NO_WINDOW)
with wave.open(str(recording)) as audio:
    duration = audio.getnframes() / audio.getframerate()
segments = [{"text": text, "timestamp": start * duration, "endTimestamp": end * duration,
             "speakerName": name}
            for text, start, end, name in [
                ("Let us start with a Windows preview and make the first dictation feel simple.", 0, .325, "Alex"),
                ("Agreed. People should be able to choose local processing and keep their own notes.", .325, .655, "Sam"),
                ("Before we share it, record the introduction and test the installer on a clean machine.", .655, 1, "Alex")]]
content = """## The idea

Make room for the conversation. Keep the details close.

## What we decided

- Start with a Windows preview.
- Keep local processing easy to choose.
- Make every note editable.

## Next steps

- [ ] Record a short product introduction
- [ ] Test the installer on a clean machine
- [ ] Share the preview and listen to feedback

*Fictional example for the product demonstration.*
"""
with connection:
    folder = connection.execute("INSERT INTO folders(name) VALUES(?)", ("Product launch",)).lastrowid
    note = connection.execute("INSERT INTO notes(title,content,note_type,folder_id,transcript,audio_duration_seconds) VALUES(?,?,?,?,?,?)",
        ("A calmer way to capture ideas", content, "meeting", folder, json.dumps(segments), duration)).lastrowid
    connection.execute("INSERT INTO note_recordings(note_id,storage_key,duration_seconds,original_filename) VALUES(?,?,?,?)",
        (note, "example.wav", duration, "Fictional product demo.wav"))
    for key, value in [("transcription.route", {"useLocalWhisper": True}), ("meetings.provider", "local")]:
        connection.execute("INSERT OR REPLACE INTO settings(key,value) VALUES(?,?)", (key, json.dumps(value)))
print("Seeded fictional note", note, "in", profile, "- restart the isolated app before capture")
