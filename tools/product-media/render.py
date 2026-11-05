"""Render captured native footage into a captioned 40-second product intro."""
import argparse
import shutil
import subprocess
from pathlib import Path

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("--capture", type=Path, default=Path("target/product-media"))
args = parser.parse_args()
out = args.capture
out.mkdir(parents=True, exist_ok=True)
# Windows fonts are local inputs, not redistributed assets.
shutil.copyfile("C:/Windows/Fonts/segoeui.ttf", out / "body.ttf")
shutil.copyfile("C:/Windows/Fonts/seguisb.ttf", out / "title.ttf")
font = (out / "body.ttf").as_posix()
title = (out / "title.ttf").as_posix()
if ":" in font:
    raise SystemExit("Use a relative --capture path so ffmpeg font paths are portable")

def run(*args):
    subprocess.run(["ffmpeg", "-hide_banner", "-loglevel", "error", "-y", *args], check=True)

def text(value, y, size=32, color="0x30312f", x="240", face=None):
    value = value.replace("'", "").replace(":", " -")
    return f"drawtext=fontfile={face or font}:text='{value}':x={x}:y={y}:fontsize={size}:fontcolor={color}"

common = ["-c:v", "libx264", "-preset", "medium", "-crf", "23", "-pix_fmt", "yuv420p", "-r", "30", "-an"]
segments = []
for name, duration, eyebrow, headline in [
    ("dictation", 10, "01 / DICTATION", "An idea starts with your voice."),
    ("meetings", 8, "02 / MEETINGS", "Make room for the conversation."),
    ("notes", 5, "03 / NOTES", "Keep the details. Make them yours."),
    ("edit", 3, "03 / NOTES", "Your notes stay editable."),
    ("replay", 4, "04 / REPLAY", "Return to what was said."),
]:
    dest = out / f"{name}.mp4"
    overlay = ",".join([text(eyebrow, 37, 21, "0x6e6388"), text(headline, 77, 48, face=title),
        text("Native Windows development build / fictional example content", 1057, 14, "0x68665f")])
    graph = "[1:v]scale=1440:900[app];[0:v]drawbox=x=232:y=146:w=1456:h=910:color=black@0.06:t=fill[bg];[bg][app]overlay=240:150," + overlay
    run("-f", "lavfi", "-i", "color=c=0xf3f1eb:s=1920x1080:r=30", "-framerate", "5", "-i", str(out / name / "%04d.jpg"), "-filter_complex", graph, "-t", str(duration), *common, str(dest))
    segments.append(dest)
for name, duration, headline, subtitle in [
    ("opening", 4, "Your voice. Your notes. Your machine.", "Dictation, meetings and notes. Together."),
    ("closing", 6, "KnegisWhisper", "Local models or your own provider. Your choice."),
]:
    dest = out / f"{name}.mp4"
    overlay = ",".join([
        text("KNEGISWHISPER", 235, 25, "0x6e6388", "(w-text_w)/2"),
        text(headline, 490, 64, x="(w-text_w)/2", face=title),
        text(subtitle, 595, 32, "0x62605b", "(w-text_w)/2"),
        text("github.com/Knegis/KnegisWhisper", 780, 26, "0x49434f", "(w-text_w)/2"),
        text("Windows preview in preparation", 832, 21, "0x77736f", "(w-text_w)/2"),
        "fade=t=in:st=0:d=0.35", f"fade=t=out:st={duration-0.4}:d=0.4"
    ])
    graph = "[1:v]scale=120:120[mark];[0:v][mark]overlay=(W-w)/2:310,"+overlay
    run("-f", "lavfi", "-i", "color=c=0xf3f1eb:s=1920x1080:r=30", "-loop", "1", "-i", "docs/assets/frog.png", "-filter_complex", graph, "-t", str(duration), *common, str(dest))
    if name == "opening": segments.insert(0, dest)
    else: segments.append(dest)
concat = out / "segments.txt"
concat.write_text("".join(f"file '{p.name}'\n" for p in segments))
final = out / "knegiswhisper-intro.mp4"
run("-f", "concat", "-safe", "0", "-i", str(concat), "-c", "copy", "-movflags", "+faststart", str(final))
run("-ss", "4", "-t", "4", "-i", str(final), "-ss", "22", "-t", "4", "-i", str(final),
    "-filter_complex", "[0:v]fps=8,scale=800:-1[a];[1:v]fps=8,scale=800:-1[b];[a][b]concat=n=2:v=1:a=0,split[x][y];[x]palettegen=max_colors=96[p];[y][p]paletteuse=dither=bayer:bayer_scale=4", "-loop", "0", "docs/assets/intro-preview.gif")
run("-ss", "24", "-i", str(final), "-frames:v", "1", "docs/assets/intro-poster.png")
if final.stat().st_size > 20_000_000:
    raise SystemExit("MP4 exceeds 20 MB budget")
if Path("docs/assets/intro-preview.gif").stat().st_size > 5_000_000:
    raise SystemExit("GIF exceeds 5 MB budget")
print(f"Rendered {final} ({final.stat().st_size:,} bytes)")

run("-f", "lavfi", "-i", "color=c=0xf3f1eb:s=1280x640", "-i", "docs/assets/frog.png", "-filter_complex", "[1:v]scale=100:100[mark];[0:v][mark]overlay=(W-w)/2:85," + ",".join([text("KnegisWhisper", 240, 68, x="(w-text_w)/2", face=title),text("Your voice. Your notes. Your machine.", 345, 35, x="(w-text_w)/2"),text("Local-first dictation, meetings and notes", 415, 25, "0x6e6388", "(w-text_w)/2")]), "-frames:v", "1", "docs/assets/social-preview.png")
