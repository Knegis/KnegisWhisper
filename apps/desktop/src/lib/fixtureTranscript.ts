import type { TranscriptRow, Word } from "../generated/ipc-types.js";

/**
 * Transcripts for the browser dev fixtures (KNEG-448). Pure, with nothing from
 * `devFixtures.ts` or the DOM, so a node test can lay the long note out with
 * `transcriptSections` and search it with `findMatches` exactly as the column
 * will.
 */

/** A stored segment as an import local Whisper timed word by word saves it (KNEG-408). */
export type StoredSegment = { text: string; timestamp: number; endTimestamp: number; words?: Word[] };

/**
 * One row of fixture note 105: `text`, a word every `step` seconds from `start`, each lasting
 * most of its step, and the row ending just after its last word.
 */
export function wordedSegment(start: number, step: number, text: string): StoredSegment {
  const hundredths = (seconds: number) => Math.round(seconds * 100) / 100;
  const words = text.split(" ").map((word, at) => ({
    text: word,
    start: hundredths(start + at * step),
    end: hundredths(start + at * step + step * 0.8),
  }));
  return { text, timestamp: start, endTimestamp: hundredths((words.at(-1)?.end ?? start) + 0.3), words };
}

/**
 * A stored segment of the long meeting note. As `Segment` in
 * `crates/notes-core/src/transcript.rs`, `timestamp` and `endTimestamp` may be null (an engine
 * that gave no time) and `speaker` is a diarisation label, absent when the segment has none.
 */
export type LongSegment = {
  text: string;
  timestamp: number | null;
  endTimestamp: number | null;
  speaker?: string;
  words?: Word[];
};

/** How long fixture note 106's recording is: its last line ends here. */
export const LONG_TRANSCRIPT_SECONDS = 690;

// Diarisation labels. `LONG_TRANSCRIPT_NAMES` names the first two; speaker_2 is left unnamed, as a
// speaker nobody has named yet is.
const MAJA = "speaker_0";
const JONAS = "speaker_1";
const OTHER = "speaker_2";

function line(speaker: string | undefined, start: number, end: number, text: string): LongSegment {
  return { text, timestamp: start, endTimestamp: end, ...(speaker === undefined ? {} : { speaker }) };
}

function worded(speaker: string, start: number, step: number, text: string): LongSegment {
  return { ...wordedSegment(start, step, text), speaker };
}

/**
 * An 11 minute 30 second meeting, laid out to exercise the transcript column:
 * three time sections (0:00, 5:00 and 10:00), turns of several lines, word-timed lines in the
 * first and last section, three lines no speaker is known for around 6:00, one line with no time
 * in the middle of a turn, and a pause of 84 s between two lines of the same speaker (7:06 to
 * 8:30) that splits their turn. "möte" is in two lines, "Mötet" in one and "remote" in one:
 * the find tests count on exactly that.
 */
export const LONG_TRANSCRIPT_SEGMENTS: LongSegment[] = [
  // Section 1, 0:00 to 5:00.
  worded(MAJA, 0.5, 0.5, "Hej allihopa, tack för att ni kunde vara med på färdplansgenomgången."),
  line(MAJA, 6, 14, "Vi går igenom tre saker: vad som är klart, vad som släpar och vad vi väljer bort."),
  worded(MAJA, 15, 0.45, "Först visar Jonas var bygget står just nu."),
  line(JONAS, 22, 30, "Bygget går igenom på alla tre plattformarna sedan i tisdags."),
  line(JONAS, 31, 40, "Det som återstår är installationsprogrammet och själva uppdateringsflödet."),
  line(JONAS, 41, 50, "Jag räknar med att det är klart inom en vecka om inget oväntat dyker upp."),
  line(MAJA, 53, 60, "Bra, det låter rimligt."),
  line(OTHER, 63, 72, "Kan jag få fråga om testerna? De körs fortfarande bara på en maskin."),
  line(JONAS, 75, 84, "Ja, vi flyttar dem till den delade bygg-servern nästa sprint."),
  line(OTHER, 87, 95, "Då slipper vi vänta på att någon startar dem för hand."),
  line(JONAS, 98, 104, "Precis så."),
  line(MAJA, 108, 118, "Nästa punkt är kundernas önskemål, och där har vi fått mycket att tänka på."),
  line(MAJA, 120, 131, "Flera av dem vill kunna exportera till ett vanligt textformat."),
  line(MAJA, 133, 142, "Det är ett litet arbete men det syns direkt."),
  line(OTHER, 146, 156, "Jag skulle prioritera det före sökfunktionen."),
  line(JONAS, 160, 170, "Sökningen är redan halvvägs, så den är nästan klar."),
  line(MAJA, 174, 182, "Då tar vi båda, men exporten först."),
  line(JONAS, 186, 194, "Okej, jag skriver upp det."),
  line(OTHER, 198, 208, "Vem ska ta kontakt med kunderna när det är klart?"),
  line(MAJA, 212, 220, "Det gör jag, jag har redan listan."),
  line(MAJA, 222, 232, "Jag skickar ett kort mejl så fort exporten finns i en testversion."),
  line(JONAS, 236, 246, "Kan vi ha ett första möte med dem redan nästa vecka?"),
  line(MAJA, 250, 258, "Ja, jag bokar in det på måndag."),
  line(OTHER, 262, 272, "Och dokumentationen då? Den ligger tre versioner efter."),
  line(JONAS, 275, 285, "Den tar jag, jag har skrivit om hälften."),
  worded(MAJA, 289.4, 0.95, "Bra, då landar vi där för den här delen."),

  // Section 2, 5:00 to 10:00.
  line(MAJA, 302, 312, "Då går vi vidare till teamets kapacitet under hösten."),
  line(MAJA, 314, 324, "Två personer är borta i november, och en slutar i december."),
  line(JONAS, 328, 338, "Det betyder att vi måste skjuta på åtminstone en av punkterna."),
  line(OTHER, 342, 352, "Kan vi inte låna någon från plattformsgruppen?"),
  // Nobody is known to be speaking in these three.
  line(undefined, 360, 366, "Det är svårt att höra vem som pratar här."),
  line(undefined, 368, 376, "Flera personer talar i mun på varandra en stund."),
  line(undefined, 378, 386, "Någon frågar om budgeten och någon skrattar."),
  line(JONAS, 390, 398, "Jag tror att vi klarar oss om vi stryker mobilappen från planen."),
  line(JONAS, 400, 407, "Den har ändå ingen kund som väntar på den."),
  line(OTHER, 409, 416, "Det håller jag med om."),
  line(MAJA, 420, 426, "Då stryker vi den, och jag tar det med ledningen."),
  // 84 s of silence after the line above: the same speaker, but a new turn.
  line(MAJA, 510, 520, "Ledningen har sagt ja, så mobilappen är ute ur planen."),
  line(MAJA, 522, 532, "Det frigör ungefär sex veckor för resten av gruppen."),
  // No time at all, in the middle of Maja's turn.
  { text: "Det är i alla fall vad de sa på tisdagens ledningsgrupp.", timestamp: null, endTimestamp: null, speaker: MAJA },
  line(MAJA, 536, 545, "Så vi kan lägga mer tid på exporten och på ljudkvaliteten."),
  line(JONAS, 548, 557, "Ljudkvaliteten är det som kunderna klagar mest på."),
  line(OTHER, 560, 570, "Särskilt när någon sitter remote och mikrofonen är dålig."),
  line(JONAS, 573, 580, "Då kan vi testa en ny brusfilterinställning."),
  line(MAJA, 582, 594, "Mötet med leverantören är redan bokat, så vi får svar snart."),

  // Section 3, 10:00 to 11:30.
  line(JONAS, 601, 610, "Då sammanfattar jag: exporten först, sedan ljudet."),
  worded(MAJA, 612, 0.55, "Och vi bestämmer en ny avstämning om två veckor."),
  line(OTHER, 620, 628, "Det passar mig."),
  line(JONAS, 630, 639, "Jag lägger in det i kalendern direkt."),
  worded(MAJA, 641, 0.5, "Perfekt, då ses vi nästa möte med färdplanen i handen."),
  line(MAJA, 648, 656, "Tack alla för idag, det här var bra."),
  line(OTHER, 658, 666, "Tack, ha en bra vecka."),
  line(JONAS, 668, 676, "Detsamma, vi hörs."),
  line(MAJA, 680, LONG_TRANSCRIPT_SECONDS, "Hej då!"),
];

/** The names saved for the first two speakers, as `note_set_speaker_name` would keep them. */
const LONG_TRANSCRIPT_NAMES: Readonly<Record<string, string>> = { [MAJA]: "Maja", [JONAS]: "Jonas" };

/**
 * `LONG_TRANSCRIPT_SEGMENTS` as `note_transcript_rows` returns them, shaped as `rows()` in
 * `crates/notes-core/src/transcript.rs` shapes them: a saved name is the row's `speaker` and the
 * label stays in `speakerId`; a label with no saved name shows as "Speaker N" (the label's number
 * plus one) and keeps the label too; a segment with no speaker has neither. A row with no start
 * ends where the next timed row starts, and words are kept only on rows that carry them.
 */
export function longTranscriptRows(): TranscriptRow[] {
  const segments = LONG_TRANSCRIPT_SEGMENTS;
  return segments.map((segment, index) => {
    const label = segment.speaker ?? null;
    const number = label === null ? undefined : /^speaker_(\d+)$/.exec(label)?.[1];
    const speaker =
      label === null ? null : (LONG_TRANSCRIPT_NAMES[label] ?? (number === undefined ? label : `Speaker ${Number(number) + 1}`));
    const nextStart = segments.slice(index + 1).find((later) => later.timestamp !== null)?.timestamp ?? null;
    return {
      index,
      startSeconds: segment.timestamp,
      endSeconds: segment.timestamp === null ? nextStart : segment.endTimestamp,
      speaker,
      speakerId: label,
      text: segment.text,
      words: segment.words ?? [],
    };
  });
}
