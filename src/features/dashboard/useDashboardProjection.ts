import { useCallback, useEffect, useState } from "react";
import { bridge, parseUiError, type UiError } from "../../bridge";
import {
  initialProjectionTransportState,
  projectionReadFailed,
  projectionReadSucceeded,
} from "../../projectionTransport";

/**
 * Owns the backend projection subscription and the command error surface.
 *
 * The UI never derives lifecycle state locally: it renders the revision the
 * backend has published, and reports what a command actually returned. A read
 * failure becomes a transport error rather than a stale success.
 */
export function useDashboardProjection() {
  const [transport, setTransport] = useState(initialProjectionTransportState);
  const [error, setError] = useState<UiError | null>(null);

  const refresh = useCallback(async () => {
    try {
      setTransport(projectionReadSucceeded(await bridge.read()));
    } catch (value) {
      const failure = parseUiError(value, "无法读取后端状态");
      setTransport(projectionReadFailed(failure));
      throw failure;
    }
  }, []);

  useEffect(() => {
    let cancelled = false;
    void (async () => {
      let revision = 0;
      let retryDelayMs = 250;
      while (!cancelled) {
        try {
          const next = await bridge.read();
          if (cancelled) return;
          setTransport(projectionReadSucceeded(next));
          revision = next.projectionRevision;
          retryDelayMs = 250;
          await bridge.waitForProjectionChange(revision);
        } catch (value) {
          if (cancelled) return;
          setTransport(projectionReadFailed(parseUiError(value, "无法读取后端状态")));
          await new Promise((resolve) => window.setTimeout(resolve, retryDelayMs));
          retryDelayMs = Math.min(retryDelayMs * 2, 5000);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  const run = useCallback(
    async (action: () => Promise<void>) => {
      setError(null);
      try {
        await action();
        await refresh();
      } catch (value) {
        setError(parseUiError(value, "操作未完成"));
      }
    },
    [refresh],
  );

  return { projection: transport.projection, transportError: transport.error, error, run };
}

export type DashboardRun = ReturnType<typeof useDashboardProjection>["run"];
