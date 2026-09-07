import { useEffect, useState } from "react";
import { getLocalStatus } from "../lib/ipc";

export function LocalServerHealth() {
  const [message, setMessage] = useState("Checking local model server...");
  useEffect(() => {
    let active = true;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const refresh = async () => {
      try {
        const status = await getLocalStatus();
        if (active) setMessage(status.runtimeMessage);
      } catch (error) {
        if (active) setMessage(`Could not check local model server: ${String(error)}`);
      }
      if (active) timer = setTimeout(() => { void refresh(); }, 2000);
    };
    void refresh();
    return () => { active = false; clearTimeout(timer); };
  }, []);
  return <p className="muted" role="status">{message}</p>;
}
