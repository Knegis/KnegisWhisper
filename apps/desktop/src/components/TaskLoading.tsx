import { useEffect, useState } from "react";

/** Layout only: readiness and controls must wait for the fresh backend read. */
export function TaskLoading({ task, error }: { task: "dictation" | "meetings"; error: string | null }) {
  const [details, setDetails] = useState(false);
  useEffect(() => {
    if (error) return;
    const timer = setTimeout(() => setDetails(true), 150);
    return () => clearTimeout(timer);
  }, [error]);
  const title = task === "dictation" ? "Dictation" : "Meetings";
  return (
    <div className="split-main">
      <div className="task-page task-loading" data-details={details}>
        <div className="task-notice">
          <span role={error ? "alert" : "status"} className={error ? "error" : "sr-only"}>
            {error ?? `Loading ${title.toLowerCase()}…`}
          </span>
        </div>
        <section data-surface={task === "dictation" ? "dictation.task" : "meetings.record"}
          className={`task${task === "meetings" ? " task-meetings" : ""}`} aria-label={title} aria-busy={!error}>
          <h1 className="sr-only">{title}</h1>
          <div className="task-hero" aria-hidden="true">
            <span className="task-placeholder task-placeholder-title" />
            <span className="task-placeholder task-placeholder-subtitle" />
          </div>
          {!error && <div className="task-grid" aria-hidden="true">
            <div className="control-bar task-loading-card">
              <div className="task-loading-lines">
                <span className="task-placeholder" /><span className="task-placeholder" /><span className="task-placeholder" />
              </div>
              <div className="task-loading-lines"><span className="task-placeholder" /><span className="task-placeholder" /></div>
            </div>
            <div className="task-loading-art" />
          </div>}
        </section>
      </div>
    </div>
  );
}
