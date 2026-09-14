"""Build pinned CPU-only macOS Whisper packages; never publish or modify pins."""
import hashlib
import json
from pathlib import Path
import platform
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time
import urllib.request

REVISION = "371b5a7561823ab2bb32142d2751e35e7534727b"

def run(*args, **kwargs):
    try:
        return subprocess.check_output(args, text=True, stderr=subprocess.STDOUT, **kwargs).strip()
    except subprocess.CalledProcessError as error:
        print(error.output, file=sys.stderr)
        raise

def digest(path):
    with path.open("rb") as stream:
        return hashlib.file_digest(stream, "sha256").hexdigest()

def main():
    source, output, arch = Path(sys.argv[1]).resolve(), Path(sys.argv[2]).resolve(), sys.argv[3]
    if platform.system() != "Darwin" or arch not in ("arm64", "x86_64") or platform.machine() != arch:
        raise SystemExit("Requires a native Mac runner matching the requested architecture")
    assert run("git", "-C", str(source), "rev-parse", "HEAD") == REVISION
    output.mkdir(parents=True, exist_ok=True)
    build = output / "build"
    flags = ["-DCMAKE_BUILD_TYPE=Release", "-DBUILD_SHARED_LIBS=OFF", "-DWHISPER_BUILD_TESTS=OFF",
             "-DWHISPER_BUILD_SERVER=ON", "-DGGML_NATIVE=OFF", "-DGGML_METAL=OFF", "-DGGML_OPENMP=OFF",
             "-DGGML_BACKEND_DL=OFF", "-DWHISPER_COREML=OFF", "-DWHISPER_CURL=OFF",
             "-DCMAKE_OSX_DEPLOYMENT_TARGET=13.0", f"-DCMAKE_OSX_ARCHITECTURES={arch}"]
    if arch == "x86_64":
        flags.extend(f"-DGGML_{feature}=OFF" for feature in ("AVX", "AVX2", "FMA", "F16C"))
    run("cmake", "-S", str(source), "-B", str(build), *flags)
    run("cmake", "--build", str(build), "--config", "Release", "--target", "whisper-server", "-j", "3")
    package = output / f"whisper-b4938-macos-{arch}"
    package.mkdir(exist_ok=True)
    binary = package / "whisper-server"
    shutil.copy2(build / "bin" / "whisper-server", binary)
    shutil.copy2(source / "LICENSE", package / "LICENSE")
    notices = package / "licenses"
    notices.mkdir(exist_ok=True)
    for index, license_path in enumerate(sorted(source.rglob("LICENSE*"))):
        if license_path.is_file() and ".git" not in license_path.parts:
            shutil.copy2(license_path, notices / f"{index}-{license_path.name}")
    run("codesign", "--force", "--sign", "-", str(binary))
    assert run("lipo", "-archs", str(binary)) == arch
    dependencies = run("otool", "-L", str(binary)).splitlines()[1:]
    assert dependencies and all(line.strip().startswith(("/usr/lib/", "/System/Library/")) for line in dependencies), dependencies
    (package / "provenance.json").write_text(json.dumps({"sourceCommit": REVISION, "architecture": arch,
        "compiler": run("clang", "--version"), "flags": flags, "signing": "ad-hoc; not notarized",
        "minimumMacOS": "13.0", "dependencies": dependencies}, indent=2))
    artifacts = output / "artifacts"
    artifacts.mkdir(exist_ok=True)
    archive = artifacts / f"{package.name}.tar.gz"
    with tarfile.open(archive, "w:gz") as tar:
        tar.add(package, arcname=package.name)
    with tempfile.TemporaryDirectory(prefix="kw-whisper-smoke-") as temp:
        fresh = Path(temp)
        with tarfile.open(archive) as tar:
            tar.extractall(fresh, filter="data")
        executable = fresh / package.name / "whisper-server"
        subprocess.run([str(executable), "--help"], check=True, capture_output=True)
        registry = json.loads(Path("config/provider-registry.json").read_text())
        tiny = next(m for m in registry["localAsr"]["whisper"] if m["id"] == "tiny")
        model = fresh / "tiny.bin"
        urllib.request.urlretrieve(tiny["downloadUrl"], model)
        assert digest(model) == tiny["sha256"]
        assert model.stat().st_size == tiny["expectedSizeBytes"]
        with (output / "smoke-server.log").open("w") as log:
            server = subprocess.Popen([str(executable), "--model", str(model), "--host", "127.0.0.1", "--port", "18178"], stdout=log, stderr=log)
            try:
                deadline = time.monotonic() + 120
                while True:
                    if server.poll() is not None:
                        raise RuntimeError("Server exited during startup")
                    try:
                        with urllib.request.urlopen("http://127.0.0.1:18178/health", timeout=2) as response:
                            if response.status == 200:
                                break
                    except OSError:
                        pass
                    if time.monotonic() > deadline:
                        raise TimeoutError("Server startup timed out")
                    time.sleep(1)
                result = run("curl", "--fail", "--silent", "--max-time", "120", "http://127.0.0.1:18178/inference",
                             "-F", f"file=@{source / 'samples/jfk.wav'}", "-F", "language=en", "-F", "response_format=json")
                transcript = json.loads(result)["text"].lower()
                assert "country" in transcript and "ask" in transcript, "Speech fixture transcription failed"
            finally:
                server.terminate()
                try:
                    server.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    server.kill()
                    server.wait()
    (artifacts / "measurement.json").write_text(json.dumps({"archive": archive.name, "sha256": digest(archive),
        "sizeBytes": archive.stat().st_size, "extractedSizeBytes": sum(p.stat().st_size for p in package.rglob("*") if p.is_file()),
        "sourceCommit": REVISION, "architecture": arch, "smokeTest": "passed"}, indent=2))

if __name__ == "__main__":
    main()
