import { useEffect, useRef, useState } from "react";
import type { UiBridge, UiConnection, UiConnectionSnapshot } from "../../crates/metldr-panel/pkg/metldr_panel.js";

type State = Pick<UiConnectionSnapshot, "connected" | "reconnecting" | "can_reconnect" | "stale" | "uncertain">;

export function useConnection(bridge: UiBridge) {
  const current = useRef<UiConnection | null>(null);
  const [state, setState] = useState<State>({ connected: false, reconnecting: false, can_reconnect: false, stale: false, uncertain: false });
  const [error, setError] = useState("");
  useEffect(() => {
    const connection = bridge.connection();
    current.current = connection;
    setError("");
    let subscription: ReturnType<UiConnection["subscribe"]> | undefined;
    try {
      subscription = connection.subscribe(snapshot => {
        try {
          setState({ connected: snapshot.connected, reconnecting: snapshot.reconnecting,
            can_reconnect: snapshot.can_reconnect, stale: snapshot.stale, uncertain: snapshot.uncertain });
        } finally { snapshot.free(); }
      });
    } catch (error) { setError(String(error)); }
    return () => {
      if (current.current === connection) current.current = null;
      subscription?.close();
      subscription?.free();
      connection.free();
    };
  }, [bridge]);

  function reconnect() {
    const connection = current.current;
    if (!connection) return;
    setError("");
    try {
      void connection.reconnect().catch(error => {
        if (current.current === connection) setError(String(error));
      });
    } catch (error) { setError(String(error)); }
  }
  return { ...state, error, reconnect };
}
