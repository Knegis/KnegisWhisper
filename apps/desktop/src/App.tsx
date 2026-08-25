import { useCallback, useEffect, useState } from "react";
import {
  cancelDictation,
  clearHistory,
  createFolder,
  deleteFolder,
  deleteNote,
  exportNote,
  exportNoteToFile,
  getDiagnostics,
  getDictationState,
  getFolders,
  getHistory,
  getHotkeys,
  getInputDevices,
  getLocalApiStatus,
  getDictationRoute,
  getLocalStatus,
  getNotes,
  getProviders,
  installLocalModel,
  isCaptureExclusionConfirmed,
  isInsertionSupported,
  isProviderConnected,
  onDictationOutcome,
  onDictationState,
  onHotkey,
  onInstallProgress,
  probeProviders,
  searchNotes,
  setInputDevice,
  startLocalApi,
  startLocalServer,
  stopLocalApi,
  stopLocalServer,
  setHotkey,
  toggleDictation,
  useLocalModel,
  EXPORT_FORMATS,
  type Accelerator,
  type Binding,
  type CaptureExclusion,
  type Diagnostics,
  type DictationOutcome,
  type ExportedTranscript,
  type ExportFormat,
  type Folder,
  type HotkeyChange,
  type HotkeyReport,
  type InputDevice,
  type InstallProgress,
  type LocalApiStatus,
  type LocalStatus,
  type NonActivating,
  type Note,
  type ProbeReport,
  type ProviderSummary,
  type RouteReport,
  type SearchHit,
  type SessionState,
  type Transcription,
} from "./lib/ipc";
import { Overlay } from "./Overlay";

/**
 * Render a capture-exclusion state without overclaiming.
 *
 * `unknown` deliberately gets the same neutral treatment as `unsupported`
 * rather than a checkmark: the call succeeded but nothing verified it, and
 * telling the user they are hidden on that basis is exactly the failure mode
 * documented in docs/UPSTREAM_AUDIT.md.
 */
function captureExclusionLabel(c: CaptureExclusion): {
  text: string;
  detail: string;
} {
  switch (c.state) {
    case "active":
      return {
        text: "Hidden from screen capture",
        detail: `verified via ${c.verifiedBy.api} = ${c.verifiedBy.value}`,
      };
    case "unsupported":
      return { text: "Not supported on this system", detail: c.reason };
    case "failed":
      return { text: "Could not be applied", detail: c.error };
    case "unknown":
      return { text: "Applied, but cannot be verified", detail: c.reason };
  }
}

function nonActivatingLabel(n: NonActivating): { text: string; detail: string } {
  switch (n.state) {
    case "active":
      return { text: "Will not take focus", detail: `verified via ${n.verifiedBy}` };
    case "unsupported":
      return { text: "Not supported on this system", detail: n.reason };
    case "failed":
      return { text: "Could not be applied", detail: n.error };
  }
}

function acceleratorLabel(a: Accelerator): { text: string; problem: string | null } {
  switch (a.kind) {
    case "standard":
      return { text: a.accelerator, problem: null };
    case "modifier-only":
      return {
        text: a.accelerator,
        problem:
          "Uses only modifier keys. That needs a low-level keyboard hook, which is not implemented yet, so this shortcut does nothing.",
      };
    case "invalid":
      return { text: a.input, problem: a.reason };
  }
}

/** What an action's accelerators read as, for prefilling the editor. */
function acceleratorText(b: Binding): string {
  return b.accelerators.map((a) => acceleratorLabel(a).text).join(", ");
}

/**
 * What a hotkey change achieved, in one line.
 *
 * Saved and registered are different events and the user needs to tell them
 * apart: a well-formed accelerator that another application already owns is
 * stored successfully and still does nothing.
 */
function changeLabel(change: HotkeyChange): string {
  if (change.live) {
    const taken = change.registrations.find((r) => r.state === "registered");
    return `${taken?.accelerator ?? acceleratorText(change.binding)} is live.`;
  }
  // Narrowed to the two refusal variants, both of which carry a reason.
  const refused = change.registrations.find((r) => r.state !== "registered");
  return refused
    ? `Saved, but not working: ${refused.reason}`
    : "Saved, but nothing was registered.";
}

/** A dictation outcome as one line of plain language. */
function outcomeLabel(outcome: DictationOutcome): string {
  switch (outcome.outcome) {
    case "inserted":
      return outcome.clipboardRestored
        ? `Inserted ${outcome.text.length} characters; your clipboard was put back.`
        : `Inserted ${outcome.text.length} characters. Your clipboard was left as you changed it.`;
    case "nothing-to-transcribe":
      // Not an error. Saying "failed" here would send someone looking for a
      // fault when the microphone simply heard nothing.
      return outcome.reason === "silence"
        ? "Nothing was sent: the recording was silent."
        : `Nothing was sent: ${outcome.reason.replace("-", " ")}.`;
    case "cancelled":
      return "Cancelled.";
    case "failed":
      return outcome.message;
  }
}

/** How a model's bytes were checked, said plainly rather than implied. */
function verificationLabel(verifiedBy: ModelVerification): string {
  switch (verifiedBy) {
    case "digest":
      return "checksum verified";
    case "size":
      // The honest wording. A file of about the right length has not been proven
      // to be the file the publisher built.
      return "size checked only";
    case "nothing":
      return "not verified";
  }
}

type ModelVerification = "digest" | "size" | "nothing";

function bytes(written: number, total: number | null): string {
  const mb = (n: number) => `${(n / 1_000_000).toFixed(1)} MB`;
  return total ? `${mb(written)} of ${mb(total)}` : mb(written);
}

/** Progress as one short line. */
function progressLabel(progress: InstallProgress): string {
  switch (progress.stage) {
    case "server":
      return `Downloading the server — ${bytes(progress.written, progress.total)}`;
    case "model":
      return `Downloading ${progress.id} — ${bytes(progress.written, progress.total)}`;
    case "verifying":
      return "Verifying…";
    case "extracting":
      return "Unpacking…";
    case "done":
      return `Installed (${verificationLabel(progress.verifiedBy)})`;
  }
}

function Row({
  label,
  value,
  detail,
}: {
  label: string;
  value: string;
  // `| undefined` explicitly, because exactOptionalPropertyTypes is on: a
  // caller passing a computed `string | undefined` is the normal case here.
  detail?: string | undefined;
}) {
  return (
    <div className="row">
      <span className="row-label">{label}</span>
      <span className="row-value">
        {value}
        {detail ? <em className="row-detail">{detail}</em> : null}
      </span>
    </div>
  );
}

function Dashboard() {
  const [diagnostics, setDiagnostics] = useState<Diagnostics | null>(null);
  const [hotkeys, setHotkeys] = useState<HotkeyReport | null>(null);
  const [providers, setProviders] = useState<ProviderSummary[]>([]);
  const [probe, setProbe] = useState<ProbeReport | null>(null);
  const [probing, setProbing] = useState(false);
  const [history, setHistory] = useState<Transcription[]>([]);
  const [lastHotkey, setLastHotkey] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const [session, setSession] = useState<SessionState>("idle");
  const [lastOutcome, setLastOutcome] = useState<string | null>(null);
  const [devices, setDevices] = useState<InputDevice[]>([]);
  const [canInsert, setCanInsert] = useState<boolean | null>(null);
  const [local, setLocal] = useState<LocalStatus | null>(null);
  const [installing, setInstalling] = useState<string | null>(null);
  const [api, setApi] = useState<LocalApiStatus | null>(null);
  const [route, setRoute] = useState<RouteReport | null>(null);
  const [draftKeys, setDraftKeys] = useState<Record<string, string>>({});
  /** Per-action outcome of the last Set press, so the control is never silent. */
  const [hotkeyResult, setHotkeyResult] = useState<Record<string, string>>({});

  const [notes, setNotes] = useState<Note[]>([]);
  const [folders, setFolders] = useState<Folder[]>([]);
  const [folderFilter, setFolderFilter] = useState<number | null>(null);
  const [query, setQuery] = useState("");
  // `null` means "not searching" and is different from an empty array, which
  // means "searched and found nothing". Collapsing the two would show the full
  // note list as if it were the result.
  const [hits, setHits] = useState<SearchHit[] | null>(null);
  const [exportFormat, setExportFormat] = useState<ExportFormat>("txt");
  const [preview, setPreview] = useState<ExportedTranscript | null>(null);
  const [exportedTo, setExportedTo] = useState<string | null>(null);

  const message = (e: unknown) => (e instanceof Error ? e.message : String(e));

  const refresh = useCallback(async () => {
    try {
      const [
        d,
        h,
        p,
        rows,
        state,
        mics,
        insert,
        localStatus,
        apiStatus,
        routeReport,
        noteRows,
        folderRows,
      ] = await Promise.all([
        getDiagnostics(),
        getHotkeys(),
        getProviders(),
        getHistory(20),
        getDictationState(),
        getInputDevices(),
        isInsertionSupported(),
        getLocalStatus(),
        getLocalApiStatus(),
        getDictationRoute(),
        getNotes(folderFilter === null ? {} : { folderId: folderFilter }),
        getFolders(),
      ]);
      setDiagnostics(d);
      setHotkeys(h);
      setProviders(p);
      setHistory(rows);
      setSession(state);
      setDevices(mics);
      setCanInsert(insert);
      setLocal(localStatus);
      setApi(apiStatus);
      setRoute(routeReport);
      setNotes(noteRows);
      setFolders(folderRows);
    } catch (e) {
      setError(message(e));
    }
  }, [folderFilter]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  // Proves the hotkey path end to end: the shortcut fires in Rust and the event
  // arrives here. Until the dictation pipeline lands, this is the only visible
  // effect, and it is better than nothing happening at all.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void onHotkey((fired) => {
      setLastHotkey(`${fired.action} (${fired.accelerator}) at ${new Date().toLocaleTimeString()}`);
    }).then((fn) => {
      unlisten = fn;
    });
    return () => unlisten?.();
  }, []);

  // Dictation state arrives from Rust rather than being tracked here: the
  // hotkey works while this window is hidden, so the window cannot be the thing
  // that knows.
  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    void onDictationState(setSession).then((fn) => unlisteners.push(fn));
    void onDictationOutcome((outcome) => {
      setLastOutcome(outcomeLabel(outcome));
      // History and the session both moved, so re-read rather than guessing.
      void refresh();
    }).then((fn) => unlisteners.push(fn));
    void onInstallProgress((progress) => {
      setInstalling(progressLabel(progress));
      if (progress.stage === "done") {
        void refresh();
      }
    }).then((fn) => unlisteners.push(fn));
    return () => unlisteners.forEach((fn) => fn());
  }, [refresh]);

  /**
   * Save one action's accelerator and say what happened.
   *
   * Every path reports something. An empty field, a refusal by the platform, and
   * a working shortcut are three different outcomes and the row names which one
   * it was — the previous version of this control had a fourth, silence, which
   * is the only unacceptable one.
   */
  const applyHotkey = (b: Binding) => {
    const typed = (draftKeys[b.action] ?? acceleratorText(b)).trim();
    if (!typed) {
      setHotkeyResult((prev) => ({
        ...prev,
        [b.action]: "Type an accelerator first, for example Control+Shift+D.",
      }));
      return;
    }
    void guard(async () => {
      // A single comma-separated string, matching `hotkey_set`: one action can
      // have several accelerators.
      const change = await setHotkey(b.action, typed);
      setHotkeyResult((prev) => ({ ...prev, [b.action]: changeLabel(change) }));
      // Drop the draft so the field goes back to mirroring what is stored.
      setDraftKeys((prev) => {
        const next = { ...prev };
        delete next[b.action];
        return next;
      });
    });
  };

  const guard = async (work: () => Promise<unknown>) => {
    setError(null);
    try {
      await work();
      await refresh();
    } catch (e) {
      setError(message(e));
    }
  };

  const runProbe = async () => {
    setProbing(true);
    setError(null);
    try {
      setProbe(await probeProviders());
    } catch (e) {
      setError(message(e));
    } finally {
      setProbing(false);
    }
  };

  if (!diagnostics) {
    return (
      <main className="shell">
        <h1>KnegisWhisper</h1>
        {error ? <p className="error">{error}</p> : <p className="muted">Loading…</p>}
      </main>
    );
  }

  const capture = captureExclusionLabel(diagnostics.overlay.captureExclusion);
  const captureConfirmed = isCaptureExclusionConfirmed(diagnostics.overlay.captureExclusion);
  const focus = nonActivatingLabel(diagnostics.overlay.nonActivating);

  return (
    <main className="shell">
      <header>
        <h1>KnegisWhisper</h1>
        <p className="muted">
          v{diagnostics.appVersion} · {diagnostics.os} {diagnostics.arch}
          {diagnostics.webviewVersion ? ` · webview ${diagnostics.webviewVersion}` : null}
        </p>
      </header>

      {error ? <p className="error">{error}</p> : null}

      <section>
        <h2>Runtime</h2>
        <Row
          label="Credential store"
          value={
            diagnostics.credentialStore.state === "available"
              ? diagnostics.credentialStore.backend
              : "unavailable"
          }
          {...(diagnostics.credentialStore.state === "unavailable"
            ? { detail: diagnostics.credentialStore.reason }
            : {})}
        />
        <Row
          label="Model cache"
          value={diagnostics.modelCache.path}
          {...(diagnostics.modelCache.fellBack
            ? {
                detail:
                  "the per-user location contains a space or a non-ASCII character, which the native speech binaries cannot read, so a machine-wide location is used instead",
              }
            : {})}
        />
      </section>

      <section>
        <h2>Privacy overlay</h2>
        <Row label="Window" value={diagnostics.overlay.created ? "created" : "not created"} />
        <Row label="Capture exclusion" value={capture.text} detail={capture.detail} />
        <Row label="Focus" value={focus.text} detail={focus.detail} />
        {/*
          A live reading, not a setting — and it used to read like one. "Recording
          indicator: hidden" alongside "the overlay is visible whenever the
          microphone is live" says two opposite things, and the alarming one is
          the wrong one: nothing is recording, which is why nothing is showing.
        */}
        <Row
          label="Recording indicator"
          value={
            diagnostics.overlay.recordingIndicatorVisible ? "showing now" : "not showing"
          }
          detail={
            diagnostics.overlay.recordingIndicatorVisible
              ? "the microphone is live"
              : "nothing is recording; it appears whenever the microphone is live"
          }
        />
        {!captureConfirmed ? (
          <p className="caveat">
            This system cannot confirm the overlay is excluded from screen capture. Treat it as
            visible to recording software until verified. Display server:{" "}
            {diagnostics.overlay.displayServer}.
          </p>
        ) : (
          <p className="caveat">
            The exclusion flag is set and reads back correctly. That confirms the flag, not that a
            particular screen recorder honours it — no API can report that from inside this process.
          </p>
        )}
      </section>

      <section>
        <h2>Shortcuts</h2>
        {hotkeys ? (
          <>
            {hotkeys.bindings.map((b) => (
              <div key={b.action} className="row">
                <span className="row-label">{b.action}</span>
                <span className="row-value">
                  {b.accelerators.length === 0 ? (
                    <em className="row-detail">none configured</em>
                  ) : (
                    b.accelerators.map((a, i) => {
                      const label = acceleratorLabel(a);
                      return (
                        <span key={`${b.action}-${i}`}>
                          <code>{label.text}</code>
                          {label.problem ? (
                            <em className="row-detail warn">{label.problem}</em>
                          ) : null}
                        </span>
                      );
                    })
                  )}
                  {/*
                    Prefilled with what is actually configured, not a placeholder. A
                    placeholder reading `Control+Shift+D` is indistinguishable from a typed
                    value, so the field looked filled, the button was disabled for being
                    empty, and pressing it did nothing and said nothing. That cost a real
                    bench check.
                  */}
                  <input
                    aria-label={`accelerator for ${b.action}`}
                    value={draftKeys[b.action] ?? acceleratorText(b)}
                    onChange={(e) => {
                      setDraftKeys((prev) => ({ ...prev, [b.action]: e.target.value }));
                    }}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") applyHotkey(b);
                    }}
                  />
                  {/* Never disabled: a control that silently does nothing is the defect. */}
                  <button type="button" onClick={() => applyHotkey(b)}>
                    Set
                  </button>
                  {hotkeyResult[b.action] ? (
                    <em className="row-detail">{hotkeyResult[b.action]}</em>
                  ) : null}
                </span>
              </div>
            ))}
            {hotkeys.conflicts.length > 0 ? (
              <p className="caveat">
                {hotkeys.conflicts
                  .map((c) => `${c.accelerator} is claimed by ${c.actions.join(" and ")}`)
                  .join("; ")}
                .
              </p>
            ) : null}
            {lastHotkey ? <Row label="Last fired" value={lastHotkey} /> : null}
            <p className="caveat">
              A shortcut takes effect as soon as you set it. The dictation default,
              <code>Control+Super</code>, is two modifiers and no key — no cross-platform shortcut
              API can register that, so it has to be changed before the hotkey will do anything.
              Try <code>Control+Shift+D</code>.
            </p>
          </>
        ) : (
          <p className="muted">Loading…</p>
        )}
      </section>

      <section data-surface="settings.transcription">
        <h2>Transcription providers</h2>
        <p className="muted">
          A stored key is not a working provider. Only a connection test can establish that.
        </p>
        {providers.map((p) => {
          const connected = probe ? isProviderConnected(probe, p.id) : false;
          return (
            <Row
              key={p.id}
              label={p.name}
              value={
                !p.configured
                  ? "no key stored"
                  : connected
                    ? "connected"
                    : probe
                      ? "test failed"
                      : "key stored, untested"
              }
              detail={`${p.modelCount} model${p.modelCount === 1 ? "" : "s"} · slot ${p.credentialSlot}`}
            />
          );
        })}
        <button type="button" onClick={() => void runProbe()} disabled={probing}>
          {probing ? "Testing…" : "Test connections"}
        </button>
        {probe ? (
          <>
            {probe.audio === "tone" ? (
              <p className="caveat">
                The voice test used a generated tone, not speech. A pass proves the endpoint,
                credential and model id were accepted; it cannot prove words are recognised.
              </p>
            ) : null}
            <ul className="purposes">
              {probe.results
                .filter((r) => r.outcome.outcome !== "skipped")
                .map((r, i) => (
                  <li key={`${r.provider}-${r.kind}-${i}`}>
                    <code>
                      {r.provider}/{r.kind}
                    </code>{" "}
                    {r.outcome.outcome}
                    {"detail" in r.outcome && r.outcome.detail ? (
                      <em className="row-detail">{r.outcome.detail}</em>
                    ) : null}
                  </li>
                ))}
            </ul>
          </>
        ) : null}
      </section>

      <section>
        <h2>Dictation</h2>
        <Row
          label="Session"
          value={session}
          detail={session === "recording" ? "the overlay is visible" : undefined}
        />
        <Row
          label="Text insertion"
          value={
            canInsert === null ? "checking…" : canInsert ? "Supported" : "Not supported here"
          }
          detail={
            canInsert === false
              ? "dictations are transcribed and kept in history, but cannot be typed at the cursor"
              : undefined
          }
        />
        <Row
          label="Route"
          value={route === null ? "checking…" : route.problem ? "Not usable" : route.destination}
          detail={
            route?.problem
              ? route.problem
              : route?.leavesDevice === false
                ? "audio stays on this machine"
                : route?.leavesDevice === true
                  ? "audio leaves this machine"
                  : undefined
          }
        />
        {lastOutcome ? <Row label="Last result" value={lastOutcome} /> : null}
        <div className="row">
          <span className="row-label">Microphone</span>
          <span className="row-value">
            <select
              onChange={(e) => {
                void guard(() => setInputDevice(e.target.value || null));
              }}
            >
              <option value="">System default</option>
              {devices.map((d) => (
                <option key={d.name} value={d.name}>
                  {d.name}
                  {d.isDefault ? " (default)" : ""}
                </option>
              ))}
            </select>
          </span>
        </div>
        <button
          type="button"
          onClick={() => {
            void guard(toggleDictation);
          }}
        >
          {session === "recording" ? "Stop and transcribe" : "Start dictation"}
        </button>
        {session === "recording" ? (
          <button
            type="button"
            onClick={() => {
              void guard(cancelDictation);
            }}
          >
            Discard
          </button>
        ) : null}
      </section>

      <section data-surface="settings.transcription.local">
        <h2>Local transcription</h2>
        {local === null ? (
          <p className="muted">Loading…</p>
        ) : local.availability.state === "not-published" ? (
          <>
            <Row label="Availability" value="No server is published for this platform" />
            <p className="muted">{local.availability.reason}</p>
            <p className="muted">{local.availability.tracking}</p>
          </>
        ) : (
          <>
            <Row
              label="Server"
              value={local.serverInstalled ? "Installed" : "Not installed"}
              detail={local.port ? `running on 127.0.0.1:${local.port}` : undefined}
            />
            <Row
              label="Installed to"
              value={local.installDir}
              detail={
                local.installDirFellBack
                  ? "the per-user location contained a space or a non-ASCII character, which the native binaries cannot load from, so a machine-wide location is used instead"
                  : undefined
              }
            />
            {installing ? <Row label="Progress" value={installing} /> : null}
            <ul className="purposes">
              {local.models.map((m) => (
                <li key={m.id}>
                  <code>{m.id}</code> {m.name}
                  {m.sizeMb ? ` · ${m.sizeMb} MB` : ""}
                  {m.recommended ? " · recommended" : ""}
                  <em className="row-detail">
                    {m.installed
                      ? `installed, ${verificationLabel(m.verifiedBy)}`
                      : `not installed — will be ${verificationLabel(m.verifiedBy)}`}
                  </em>
                  {m.installed ? (
                    <button
                      type="button"
                      onClick={() => {
                        void guard(async () => {
                          await startLocalServer(m.id);
                          // Two steps under one label, and the label says both:
                          // a server that is up is not the same as a route
                          // pointing at it.
                          await useLocalModel(true);
                        });
                      }}
                    >
                      Use for dictation
                    </button>
                  ) : (
                    <button
                      type="button"
                      onClick={() => {
                        setInstalling("Starting…");
                        void guard(() => installLocalModel(m.id));
                      }}
                    >
                      Install
                    </button>
                  )}
                </li>
              ))}
            </ul>
            {local.port ? (
              <button
                type="button"
                onClick={() => {
                  void guard(async () => {
                    await useLocalModel(false);
                    await stopLocalServer();
                  });
                }}
              >
                Stop and use a provider instead
              </button>
            ) : null}
          </>
        )}
      </section>

      <section data-surface="integrations.api">
        <h2>Local API</h2>
        {api === null ? (
          <p className="muted">Loading…</p>
        ) : (
          <>
            <Row
              label="State"
              value={api.running ? `Listening on 127.0.0.1:${api.port}` : "Not listening"}
              detail={api.enabled && !api.running ? "switched on but failed to start" : undefined}
            />
            <p className="muted">
              Off by default. It exposes every note and transcript to anything running as you, so it
              only listens when you ask it to. It never binds anything but loopback.
            </p>
            {api.discoveryPath ? (
              <Row
                label="Discovery file"
                value={api.discoveryPath}
                detail={api.discoveryPermissions ?? undefined}
              />
            ) : null}
            {api.discoveryPermissionsAsExpected === false ? (
              <p className="error">
                The discovery file is more readable than intended. Anything that can read it can use
                the local API.
              </p>
            ) : null}
            <button
              type="button"
              onClick={() => {
                void guard(api.running ? stopLocalApi : startLocalApi);
              }}
            >
              {api.running ? "Stop the local API" : "Start the local API"}
            </button>
          </>
        )}
      </section>

      <section data-surface="notes.export">
        <h2>Notes</h2>

        <div className="row">
          <span className="row-label">Search</span>
          <span className="row-value controls">
            <input
              type="search"
              value={query}
              placeholder="Words to look for"
              onChange={(e) => {
                const next = e.target.value;
                setQuery(next);
                // An empty box is not a search for nothing — it is not
                // searching. Rust refuses an empty query, so never send one.
                if (next.trim() === "") {
                  setHits(null);
                  return;
                }
                void guard(async () => {
                  setHits(await searchNotes(next, 20));
                });
              }}
            />
          </span>
        </div>

        <div className="row">
          <span className="row-label">Folder</span>
          <span className="row-value controls">
            <select
              value={folderFilter ?? ""}
              onChange={(e) => {
                setFolderFilter(e.target.value === "" ? null : Number(e.target.value));
              }}
            >
              <option value="">All folders</option>
              {folders.map((f) => (
                <option key={f.id} value={f.id}>
                  {f.name}
                </option>
              ))}
            </select>
            <button
              type="button"
              onClick={() => {
                const name = window.prompt("Folder name");
                if (name === null || name.trim() === "") return;
                void guard(() => createFolder(name));
              }}
            >
              New folder
            </button>
            {folderFilter === null ? null : (
              <button
                type="button"
                onClick={() => {
                  void guard(async () => {
                    await deleteFolder(folderFilter);
                    // The notes survive with no folder, so the filter that
                    // pointed at it no longer selects anything real.
                    setFolderFilter(null);
                  });
                }}
              >
                Delete folder
              </button>
            )}
          </span>
        </div>

        {hits === null ? null : (
          <>
            <h3>
              {hits.length === 0
                ? `Nothing matches "${query}"`
                : `${hits.length} match${hits.length === 1 ? "" : "es"}`}
            </h3>
            <ul className="list">
              {hits.map((hit) => (
                <li key={hit.id}>
                  <strong>{hit.title}</strong>
                  <span className="muted"> {hit.snippet}</span>
                </li>
              ))}
            </ul>
          </>
        )}

        {notes.length === 0 ? (
          <p className="muted">
            No notes yet. Meetings and imported recordings write them; dictation writes history
            instead.
          </p>
        ) : (
          <ul className="list">
            {notes.map((note) => (
              <li key={note.id}>
                <strong>{note.title}</strong>
                <span className="muted">
                  {" "}
                  {note.noteType}
                  {note.transcript ? " · has a transcript" : ""}
                  {note.updatedAt ? ` · ${note.updatedAt}` : ""}
                </span>
                <button
                  type="button"
                  onClick={() => {
                    void guard(async () => {
                      setExportedTo(null);
                      setPreview(await exportNote(note.id, exportFormat));
                    });
                  }}
                >
                  Preview {exportFormat}
                </button>
                <button
                  type="button"
                  onClick={() => {
                    void guard(async () => {
                      const path = await exportNoteToFile(note.id, exportFormat);
                      // `null` is a cancelled dialog, not a failure. Saying
                      // "exported" for it would be a lie about a file that does
                      // not exist.
                      setExportedTo(path);
                    });
                  }}
                >
                  Export…
                </button>
                <button
                  type="button"
                  onClick={() => {
                    void guard(async () => {
                      await deleteNote(note.id);
                      setPreview(null);
                    });
                  }}
                >
                  Delete
                </button>
              </li>
            ))}
          </ul>
        )}

        <div className="row">
          <span className="row-label">Export format</span>
          <span className="row-value controls">
            <select
              value={exportFormat}
              onChange={(e) => {
                setExportFormat(e.target.value as ExportFormat);
                setPreview(null);
              }}
            >
              {EXPORT_FORMATS.map((f) => (
                <option key={f} value={f}>
                  {f}
                </option>
              ))}
            </select>
          </span>
        </div>

        {exportedTo ? <Row label="Written to" value={exportedTo} /> : null}

        {preview ? (
          <>
            <Row
              label={`Preview (${preview.filename})`}
              value={
                preview.segmentCount === 0
                  ? "No transcript segments"
                  : `${preview.segmentCount} segment${preview.segmentCount === 1 ? "" : "s"}`
              }
              detail={
                preview.segmentCount === 0
                  ? "this note was typed, not recorded"
                  : undefined
              }
            />
            <pre className="preview">{preview.content}</pre>
          </>
        ) : null}
      </section>

      <section>
        <h2>History</h2>
        {history.length === 0 ? (
          <p className="muted">Nothing dictated yet.</p>
        ) : (
          <>
            <ul className="purposes">
              {history.map((h) => (
                <li key={h.id}>
                  <code>{h.status}</code> {h.text || h.errorMessage || "(empty)"}
                  {h.provider ? <em className="row-detail">{h.provider}</em> : null}
                </li>
              ))}
            </ul>
            <button
              type="button"
              onClick={() => {
                void clearHistory().then(() => refresh());
              }}
            >
              Clear history
            </button>
          </>
        )}
      </section>

      <section>
        <h2>Network boundary</h2>
        <p className="muted">
          Outbound requests are permitted only for these purposes. Nothing is sent at startup —
          the connection test above is the only thing on this screen that reaches the network, and
          only when you press it.
        </p>
        <ul className="purposes">
          {diagnostics.allowedEgressPurposes.map((p) => (
            <li key={p}>
              <code>{p}</code>
            </li>
          ))}
        </ul>
      </section>
    </main>
  );
}

/**
 * Both windows are served by one bundle, routed on the URL fragment. One bundle
 * means one CSP and one build.
 */
export default function App() {
  const isOverlay = window.location.hash === "#overlay";
  return isOverlay ? <Overlay /> : <Dashboard />;
}
