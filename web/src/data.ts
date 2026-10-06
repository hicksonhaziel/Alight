import { useEffect, useState } from "react";
import {
  AlightClient,
  AlightError,
  type ApiHealth,
  type ApiClock,
  type CurvePage,
  type WorkbenchEvidence,
  type ProveReport,
} from "../../sdk/ts/src/index";
import { schemas } from "../../sdk/ts/src/schemas";
import { matches } from "../../sdk/ts/src/validation";

export interface DashboardData {
  client: AlightClient;
  health: ApiHealth;
  clock: ApiClock;
  curves: CurvePage;
  evidence: WorkbenchEvidence;
  proves: ProveReport[];
  receivedAt: number;
}
export type Connection =
  | "connecting"
  | "connected"
  | "reconnecting"
  | "paused"
  | "offline";
export function errorText(error: unknown): string {
  if (error instanceof AlightError) {
    const messages: Record<string, string> = {
      SOURCE_MISMATCH:
        "The server changed data source. Reconnect before continuing.",
      CONTRACT: "The server response does not match the published contract.",
      OPERATOR_REQUIRED:
        "Connect an operator key to lock a forecast or start Prove.",
      OPERATOR_DISABLED: "This server has operator actions disabled.",
      UNAUTHORIZED: "The operator key was rejected.",
      PROVE_CONFLICT:
        "This forecast expired, changed regime, or already has a locked cell. Request a new quote.",
      RATE_LIMITED: "The server is busy. Wait a moment and try again.",
      LEDGER_INVALID:
        "Ledger verification failed. Inspect the stored evidence before using these forecasts.",
      TRANSPORT_RECONCILE_WRITES:
        "The connection ended. Check the ledger and Prove history before repeating an action.",
    };
    return (
      messages[error.code] ?? `The request could not complete (${error.code}).`
    );
  }
  return "Could not connect to the Alight API. Check that the daemon is running.";
}
/** One source-scoped stream, bounded reconnect backoff, and non-overlapping evidence refresh. */
export function useDashboard(reconnect: number) {
  const [data, setData] = useState<DashboardData | null>(null);
  const [connection, setConnection] = useState<Connection>("connecting");
  const [error, setError] = useState("");
  useEffect(() => {
    let stopped = false,
      socket: WebSocket | undefined,
      retry = 1000;
    let streamTimer: ReturnType<typeof setTimeout>,
      refreshTimer: ReturnType<typeof setTimeout>,
      watchdog: ReturnType<typeof setInterval>;
    let latest = 0,
      client: AlightClient | undefined;
    const controller = new AbortController();
    setData(null);
    setError("");
    setConnection("connecting");
    const fatal = (e: unknown) => {
      if (stopped) return;
      setError(errorText(e));
      setConnection("offline");
      if (
        e instanceof AlightError &&
        ["SOURCE_MISMATCH", "CONTRACT"].includes(e.code)
      ) {
        stopped = true;
        socket?.close();
        setData(null);
      }
    };
    const stream = () => {
      if (stopped || !client || document.hidden) return;
      socket = client.stream((snapshot) => {
        if (stopped) return;
        latest = Date.now();
        retry = 1000;
        setConnection("connected");
        setData((old) =>
          old
            ? {
                ...old,
                health: snapshot.health,
                clock: snapshot.clock,
                proves: snapshot.proves,
                receivedAt: latest,
              }
            : old,
        );
      }, fatal);
      socket.onclose = () => {
        if (stopped || document.hidden) return;
        setConnection("reconnecting");
        streamTimer = setTimeout(stream, retry);
        retry = Math.min(retry * 2, 30000);
      };
    };
    const refresh = async () => {
      if (stopped || !client) return;
      try {
        if (!document.hidden) {
          const [curves, evidence] = await Promise.all([
            client.curves(),
            client.workbench(),
          ]);
          if (!stopped)
            setData((old) => (old ? { ...old, curves, evidence } : old));
        }
      } catch (e) {
        fatal(e);
      }
      if (!stopped) refreshTimer = setTimeout(refresh, 15000);
    };
    const start = async () => {
      try {
        const response = await fetch("/v1/health", {
          signal: controller.signal,
          credentials: "omit",
          redirect: "error",
        });
        const text = await response.text();
        if (text.length > 65536 || !response.ok)
          throw new AlightError("TRANSPORT");
        const health: unknown = JSON.parse(text);
        if (!matches(schemas.ApiHealth, health, schemas))
          throw new AlightError("CONTRACT");
        const h = health as ApiHealth;
        client = new AlightClient({
          endpoint: location.origin,
          source: h.source,
        });
        const [clock, curves, evidence, proves] = await Promise.all([
          client.clock(),
          client.curves(),
          client.workbench(),
          client.proves(8),
        ]);
        if (stopped) return;
        setData({
          client,
          health: h,
          clock,
          curves,
          evidence,
          proves: proves.reports,
          receivedAt: Date.now(),
        });
        latest = Date.now();
        setError("");
        stream();
        refreshTimer = setTimeout(refresh, 15000);
        watchdog = setInterval(() => {
          if (
            !stopped &&
            !document.hidden &&
            latest &&
            Date.now() - latest > 5000
          )
            setConnection("reconnecting");
        }, 2000);
      } catch (e) {
        if (!controller.signal.aborted) fatal(e);
      }
    };
    const visible = () => {
      clearTimeout(streamTimer);
      if (document.hidden) {
        socket?.close();
        setConnection("paused");
      } else if (client && !stopped) {
        stream();
      }
    };
    document.addEventListener("visibilitychange", visible);
    void start();
    return () => {
      stopped = true;
      controller.abort();
      clearTimeout(streamTimer);
      clearTimeout(refreshTimer);
      clearInterval(watchdog);
      socket?.close();
      document.removeEventListener("visibilitychange", visible);
    };
  }, [reconnect]);
  return { data, connection, error };
}
