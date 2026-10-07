/**
 * `toneWav`: the recording the browser dev fixtures play (KNEG-412). A
 * 44-byte RIFF header and 16-bit mono PCM, a soft 0.2 s beep once a second,
 * so a headless check can play, seek and hand a note's player across and
 * measure where it is.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { FIXTURE_RECORDING_SECONDS, toneWav } from "../src/lib/fixtureWav.js";

const view = (bytes: Uint8Array) => new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
const text = (bytes: Uint8Array, at: number) => String.fromCharCode(...bytes.subarray(at, at + 4));
const samples = (bytes: Uint8Array): number[] => {
  const data = view(bytes);
  const out: number[] = [];
  for (let at = 44; at < bytes.byteLength; at += 2) out.push(data.getInt16(at, true));
  return out;
};

test("the header is a 16-bit mono PCM WAV at the sample rate asked for", () => {
  const wav = toneWav({ seconds: 2 });
  const data = view(wav);
  assert.equal(text(wav, 0), "RIFF");
  assert.equal(data.getUint32(4, true), wav.byteLength - 8);
  assert.equal(text(wav, 8), "WAVE");
  assert.equal(text(wav, 12), "fmt ");
  assert.equal(data.getUint32(16, true), 16);
  assert.equal(data.getUint16(20, true), 1, "PCM");
  assert.equal(data.getUint16(22, true), 1, "mono");
  assert.equal(data.getUint32(24, true), 8000, "the default sample rate");
  assert.equal(data.getUint32(28, true), 16000, "byte rate");
  assert.equal(data.getUint16(32, true), 2, "block align");
  assert.equal(data.getUint16(34, true), 16, "bits per sample");
  assert.equal(text(wav, 36), "data");

  const other = view(toneWav({ seconds: 1, sampleRate: 16000 }));
  assert.equal(other.getUint32(24, true), 16000);
  assert.equal(other.getUint32(28, true), 32000);
});

test("the data is seconds x rate x 2 bytes, after the 44-byte header", () => {
  for (const [seconds, sampleRate] of [[2, 8000], [3, 16000], [FIXTURE_RECORDING_SECONDS, 8000]] as const) {
    const wav = toneWav({ seconds, sampleRate });
    assert.equal(view(wav).getUint32(40, true), seconds * sampleRate * 2);
    assert.equal(wav.byteLength, 44 + seconds * sampleRate * 2);
  }
});

test("no sample is louder than the amplitude", () => {
  for (const amplitude of [0.05, 0.5]) {
    const values = samples(toneWav({ seconds: 2, amplitude }));
    const peak = Math.max(...values.map(Math.abs));
    assert.ok(peak <= Math.round(amplitude * 32767), `peak ${peak} at amplitude ${amplitude}`);
    assert.ok(peak > 0, "there is a sound");
  }
});

test("the sound is a 0.2 s beep once a second, silent in between", () => {
  const rate = 8000;
  const values = samples(toneWav({ seconds: 3, sampleRate: rate }));
  for (let second = 0; second < 3; second += 1) {
    const beep = values.slice(second * rate, second * rate + 0.2 * rate);
    const gap = values.slice(second * rate + 0.2 * rate, (second + 1) * rate);
    assert.ok(beep.some((value) => value !== 0), `a beep in second ${second}`);
    assert.ok(gap.every((value) => value === 0), `silence after the beep in second ${second}`);
  }
});

test("the fixture recording is two minutes long", () => {
  assert.equal(FIXTURE_RECORDING_SECONDS, 120);
});
