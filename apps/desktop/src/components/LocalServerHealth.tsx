import { useEffect, useState } from "react";
import { getLocalStatus, type LocalStatus } from "../lib/ipc";
import { localModelLine } from "../lib/localModelLine";
import { LearnMore } from "./LearnMore";

/**
 * The local model server's state, polled every two seconds.
 *
 * Diagnostics shows the server's whole sentence, model id and port included.
 * `plain` is for Dictation, which is for colleagues who are not technical:
 * nothing while the server works, and failures in plain words with the
 * mechanism behind Learn more (KNEG-357, `lib/localModelLine.ts`).
 */
export function LocalServerHealth({ plain = false }: { plain?: boolean }) {
  const [status, setStatus] = useState<LocalStatus | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const refresh = async () => {
      try {
        const next = await getLocalStatus();
        if (active) {
          setStatus(next);
          setFailure(null);
        }
      } catch (error) {
        if (active) setFailure(String(error));
      }
      if (active) timer = setTimeout(() => { void refresh(); }, 2000);
    };
    void refresh();
    return () => { active = false; clearTimeout(timer); };
  }, []);

  if (!plain) {
    const message = failure !== null
      ? `Could not check local model server: ${failure}`
      : status?.runtimeMessage ?? "Checking local model server...";
    // The bars wave and the port shows only while the server is actually up.
    const port = status?.runtime.state === "ready" ? status.runtime.port : null;
    return (
      <div className="server-banner" role="status" data-live={port !== null}>
        <span className="server-banner-bars" aria-hidden="true">
          <span />
          <span />
          <span />
          <span />
        </span>
        <span className="server-banner-text">{message}</span>
        {port !== null ? <span className="server-banner-port" aria-hidden="true">{port}</span> : null}
      </div>
    );
  }

  const line = failure !== null
    ? { text: "Couldn't check on the local model.", problem: true, more: failure }
    : status ? localModelLine(status.runtime) : null;
  // The live region is always rendered, so it exists before a failure
  // arrives in it.
  return (
    <div role="status">
      {line ? (
        <>
          <p className={line.problem ? "error" : "muted"}>{line.text}</p>
          {line.more ? (
            <LearnMore>
              <p>{line.more}</p>
            </LearnMore>
          ) : null}
        </>
      ) : null}
    </div>
  );
}
