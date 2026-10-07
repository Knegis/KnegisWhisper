/**
 * A recording the browser dev fixtures can play (KNEG-412).
 *
 * In the Tauri window a note's player streams its WAV from Rust's `recording`
 * scheme (`lib/ipc.ts`' `recordingSrc`). A plain browser has nothing behind
 * that URL, so without this the fixtures' player could only ever show its
 * error, and a headless check could not measure playback at all: whether it
 * keeps playing, and from where, when the note moves its one player between
 * the transcript column, the mini player and Replay (`NoteWorkspace.tsx`).
 *
 * The sound is a soft 0.2 s beep once a second, so a person who opens the
 * fixtures hears that something plays and roughly where, and silence fills
 * the rest. `devFixtures.ts` builds it once, the first time a note with a
 * recording asks for one, and serves it as a blob URL.
 */

/** How long the fixtures' recording is. Fixture transcript rows that a check plays must end before it. */
export const FIXTURE_RECORDING_SECONDS = 120;

export type ToneWavOptions = {
  seconds: number;
  sampleRate?: number;
  frequency?: number;
  /** The beep's peak, as a fraction of full scale. */
  amplitude?: number;
};

const BEEP_SECONDS = 0.2;
// Each beep fades in and out over this, so it starts and stops without a click.
const RAMP_SECONDS = 0.01;
const HEADER_BYTES = 44;

/** A 44-byte RIFF header and 16-bit mono PCM: a beep at the start of every second. */
export function toneWav({ seconds, sampleRate = 8000, frequency = 440, amplitude = 0.05 }: ToneWavOptions): Uint8Array<ArrayBuffer> {
  if (!(seconds > 0) || !Number.isInteger(sampleRate) || sampleRate <= 0) {
    throw new RangeError(`toneWav needs a positive length and a positive whole sample rate, not ${seconds} s at ${sampleRate} Hz`);
  }
  const count = Math.round(seconds * sampleRate);
  const dataBytes = count * 2;
  const bytes = new Uint8Array(HEADER_BYTES + dataBytes);
  const view = new DataView(bytes.buffer);
  const ascii = (at: number, text: string) => {
    for (let i = 0; i < text.length; i += 1) bytes[at + i] = text.charCodeAt(i);
  };
  ascii(0, "RIFF");
  view.setUint32(4, HEADER_BYTES - 8 + dataBytes, true);
  ascii(8, "WAVE");
  ascii(12, "fmt ");
  view.setUint32(16, 16, true); // the size of this fmt chunk
  view.setUint16(20, 1, true); // PCM
  view.setUint16(22, 1, true); // mono
  view.setUint32(24, sampleRate, true);
  view.setUint32(28, sampleRate * 2, true); // byte rate
  view.setUint16(32, 2, true); // block align
  view.setUint16(34, 16, true); // bits per sample
  ascii(36, "data");
  view.setUint32(40, dataBytes, true);

  const peak = amplitude * 32767;
  for (let i = 0; i < count; i += 1) {
    const within = (i % sampleRate) / sampleRate;
    if (within >= BEEP_SECONDS) continue;
    const envelope = Math.min(1, within / RAMP_SECONDS, (BEEP_SECONDS - within) / RAMP_SECONDS);
    const value = Math.round(peak * envelope * Math.sin((2 * Math.PI * frequency * i) / sampleRate));
    view.setInt16(HEADER_BYTES + i * 2, value, true);
  }
  return bytes;
}
