/**
 * The first-run setup guide, on Dictation (KNEG-329).
 *
 * A new profile opened on "Press Control Win to dictate" with nothing saying
 * what would happen, into which application, or where the audio would go. It
 * also read as ready while set to a cloud service with no key, and the first
 * dictation failed. This is three steps, in the order a first dictation needs
 * them, each of which says what is true now and offers the one thing to do
 * about it:
 *
 * 1. Where speech becomes text: on this computer, or at a service.
 * 2. The microphone: a five-second check, nothing recorded.
 * 3. A first try, with what was heard shown here as well as typed.
 *
 * Rules it holds to (the issue's):
 *
 * - **Nothing downloads and nothing is sent because of the guide.** It links
 *   to where a model is installed; it does not install one. A key is saved
 *   where it is typed and sends nothing until the service is chosen and used.
 * - **It can be skipped at any point, and is not shown again** once closed.
 * - **It never says more than the page.** Step one is done by the same reading
 *   `dictationReadiness` uses (`lib/setupGuide.ts`).
 *
 * It draws and asks; it writes nothing itself. The page it sits on does the
 * dismissing, because the settings gate looks for that writer there
 * (`tools/settings-audit`, rule F).
 */

import { useState } from "react";

import { checkMicrophone, type MicrophoneCheck, type ProviderSummary, type RouteReport } from "../lib/ipc";
import { MICROPHONE_CHECK_LISTENING, microphoneCheckLine } from "../lib/meetingView";
import { closeLabel, routeStep, stepsDone } from "../lib/setupGuide";
import { message } from "../lib/useRefresh";
import { FailureLine } from "./FailureLine";
import { Keycaps } from "./Keycaps";
import { LevelMeter } from "./LevelMeter";
import { ProviderKeyForm } from "./ProviderKeyForm";

export function SetupGuide({
  route,
  providers,
  pressable,
  tried,
  canInsert,
  onInstallLocal,
  onUseService,
  onKeySaved,
  onClose,
}: {
  route: RouteReport;
  providers: readonly ProviderSummary[];
  /** The dictation shortcut as it can be pressed, for the keycaps. Empty when none works. */
  pressable: readonly string[];
  /** The text of a dictation made since the page opened, or `null`. */
  tried: string | null;
  /** Whether this platform can type into other applications at all. */
  canInsert: boolean;
  /** Go to where a model for this computer is installed. */
  onInstallLocal: () => void;
  /** Point dictation at a service that has a key. */
  onUseService: (providerId: string) => void;
  /** A key was saved or removed: re-read what the page shows. */
  onKeySaved: () => void | Promise<void>;
  /** Finished or skipped. */
  onClose: () => void;
}) {
  const where = routeStep(route, providers);
  // Services a key could make usable; one whose client is not built is left out.
  const services = providers.filter((p) => p.dispatch.state !== "needs-client");
  const inUse = route.isLocal ? null : (providers.find((p) => p.id === route.destination)?.id ?? null);
  const [picked, setPicked] = useState<string | null>(null);
  const service = services.find((p) => p.id === (picked ?? inUse ?? "")) ?? null;

  const [listening, setListening] = useState(false);
  const [check, setCheck] = useState<MicrophoneCheck | null>(null);
  const [checkProblem, setCheckProblem] = useState<string | null>(null);
  const testMicrophone = () => {
    setListening(true);
    setCheck(null);
    setCheckProblem(null);
    checkMicrophone()
      .then(setCheck)
      .catch((cause: unknown) => setCheckProblem(message(cause)))
      .finally(() => setListening(false));
  };

  const heard = check?.heard === true;
  const done = stepsDone(where, heard, tried !== null);

  return (
    <section className="setup-guide" aria-labelledby="setup-guide-title">
      <div className="setup-head">
        <div className="setup-heading">
          <h2 id="setup-guide-title" className="section-label">set up</h2>
          <p className="setup-intro">
            Three steps to a first dictation. Nothing is downloaded or sent until you choose it.
          </p>
        </div>
        <span className="setup-count" aria-label={`${done} of 3 steps done`}>{done} / 3</span>
      </div>

      <ol className="setup-steps">
        <li className="setup-step" data-done={where.done}>
          <span className="setup-number" aria-hidden="true">1</span>
          <div className="setup-body">
            <h3 className="setup-title">Where your speech becomes text</h3>
            <p className="setup-state">{where.line}</p>
            {where.done ? null : (
              <div className="setup-choices">
                <div className="setup-choice">
                  <p className="setup-choice-name">On this computer</p>
                  <p className="muted">Audio never leaves it. You install a speech model first.</p>
                  <button type="button" className="pill" onClick={onInstallLocal}>
                    Choose a model to install
                  </button>
                </div>
                {services.length > 0 ? (
                  <div className="setup-choice">
                    <p className="setup-choice-name">At a cloud service</p>
                    <p className="muted">Nothing to install. Your audio is sent to the service you pick, with a key from your account there.</p>
                    <select
                      aria-label="Cloud service"
                      value={service?.id ?? ""}
                      onChange={(e) => setPicked(e.target.value === "" ? null : e.target.value)}
                    >
                      <option value="">Choose a service</option>
                      {services.map((p) => (
                        <option key={p.id} value={p.id}>{p.name}</option>
                      ))}
                    </select>
                    {service ? (
                      <ProviderKeyForm slot={service.credentialSlot} name={service.name} stored={service.configured} onSaved={onKeySaved} />
                    ) : null}
                    {service && service.configured && service.id !== inUse ? (
                      <button type="button" className="pill" onClick={() => onUseService(service.id)}>
                        {`Use ${service.name} for dictation`}
                      </button>
                    ) : null}
                  </div>
                ) : null}
              </div>
            )}
          </div>
        </li>

        <li className="setup-step" data-done={heard}>
          <span className="setup-number" aria-hidden="true">2</span>
          <div className="setup-body">
            <h3 className="setup-title">Your microphone</h3>
            {listening ? (
              <>
                <p className="setup-state" role="status">{MICROPHONE_CHECK_LISTENING}</p>
                <LevelMeter session="microphone-check" source="microphone" segments={9} className="level-wave" />
              </>
            ) : check ? (
              <p className="setup-state" role="status">{microphoneCheckLine(check)}</p>
            ) : (
              <p className="setup-state">A five-second check that it hears you. Nothing is recorded and nothing is sent.</p>
            )}
            {checkProblem ? <FailureLine text="The microphone couldn't be checked." detail={checkProblem} /> : null}
            <button type="button" className="pill" disabled={listening} onClick={testMicrophone}>
              {listening ? "Listening…" : check ? "Test again" : "Test the microphone"}
            </button>
          </div>
        </li>

        <li className="setup-step" data-done={tried !== null}>
          <span className="setup-number" aria-hidden="true">3</span>
          <div className="setup-body">
            <h3 className="setup-title">Try it</h3>
            {where.done ? (
              <p className="setup-state">
                {pressable.length > 0 ? (
                  <>Press <Keycaps accelerators={[...pressable]} />, say a sentence, and press it again.</>
                ) : (
                  <>Press Start dictation below, say a sentence, and press Stop.</>
                )}{" "}
                {canInsert
                  ? "Your words are typed wherever the cursor is, in any application, and shown here."
                  : "Your words are shown here; this computer can't type them into other applications."}
              </p>
            ) : (
              <p className="setup-state">Once step 1 is done, a first dictation can be made here.</p>
            )}
            <p className={tried === null ? "setup-sample setup-sample-empty" : "setup-sample"} role="status">
              {tried ?? "Your words appear here."}
            </p>
          </div>
        </li>
      </ol>

      <div className="setup-foot">
        <button type="button" className={done === 3 ? "action-iris" : "pill"} onClick={onClose}>
          {closeLabel(done)}
        </button>
        {done === 3 ? null : <span className="muted">Everything here is also under Settings.</span>}
      </div>
    </section>
  );
}
