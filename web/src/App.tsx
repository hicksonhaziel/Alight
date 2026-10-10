import { useEffect, useMemo, useState, type ComponentType } from "react";
import {
  Activity,
  ArrowUpRight,
  ChartNoAxesCombined,
  CircleHelp,
  Command,
  FlaskConical,
  GitBranch,
  KeyRound,
  LayoutDashboard,
  Menu,
  Moon,
  Radio,
  Search,
  ShieldCheck,
  Sun,
  Wallet,
  Unplug,
  RefreshCw,
} from "lucide-react";
import { AlightClient, type ForecastEntry } from "../../sdk/ts/src/index";
import { useDashboard } from "./data";
import { Dialog, Empty, Loading, Notice } from "./ui";
import { Cockpit } from "./pages/Cockpit";
import { Quote } from "./pages/Quote";
import { Prove } from "./pages/Prove";
import { Ledger } from "./pages/Ledger";
import { Regimes } from "./pages/Regimes";
import { Health } from "./pages/Health";
import { Tape } from "./pages/Tape";
const navigation: {
  id: string;
  name: string;
  description: string;
  icon: ComponentType<{ size?: number; "aria-hidden"?: boolean }>;
}[] = [
  {
    id: "cockpit",
    name: "Cockpit",
    description: "Clock, curves and canary activity",
    icon: LayoutDashboard,
  },
  {
    id: "quote",
    name: "Quote console",
    description: "Find a supported configuration",
    icon: ChartNoAxesCombined,
  },
  {
    id: "prove",
    name: "Prove",
    description: "Test a frozen claim",
    icon: FlaskConical,
  },
  {
    id: "ledger",
    name: "Forecast ledger",
    description: "Inspect and verify issued claims",
    icon: ShieldCheck,
  },
  {
    id: "regimes",
    name: "Regimes",
    description: "Observed condition labels",
    icon: GitBranch,
  },
  {
    id: "health",
    name: "Observer health",
    description: "Freshness, gaps and disagreements",
    icon: Activity,
  },
  {
    id: "tape",
    name: "Tape & receipts",
    description: "Market transfers and coverage",
    icon: Wallet,
  },
];
export function App() {
  const [route, setRoute] = useState(location.hash.slice(1) || "cockpit"),
    [theme, setTheme] = useState<"dark" | "light">(
      document.documentElement.dataset.theme === "light" ? "light" : "dark",
    );
  const [reconnect, setReconnect] = useState(0),
    [menu, setMenu] = useState(false),
    [search, setSearch] = useState(false),
    [query, setQuery] = useState(""),
    [operatorOpen, setOperatorOpen] = useState(false),
    [operatorKey, setOperatorKey] = useState(""),
    [draftKey, setDraftKey] = useState("");
  const [frozen, setFrozen] = useState<ForecastEntry | null>(null);
  const { data, connection, error } = useDashboard(reconnect);
  const page =
      route.split("/")[0] === "datasets" ? "ledger" : route.split("/")[0],
    selected = navigation.find((n) => n.id === page);
  let reference: string | undefined;
  try {
    reference = route.split("/")[1]
      ? decodeURIComponent(route.split("/")[1])
      : undefined;
  } catch {
    reference = undefined;
  }
  const source = data?.health.source;
  const operator = useMemo(
    () =>
      operatorKey && source
        ? new AlightClient({ endpoint: location.origin, source, operatorKey })
        : null,
    [operatorKey, source],
  );
  useEffect(() => {
    const change = () => {
      setRoute(location.hash.slice(1) || "cockpit");
      setMenu(false);
      setSearch(false);
      window.scrollTo({ top: 0 });
      requestAnimationFrame(() => document.getElementById("main")?.focus());
    };
    window.addEventListener("hashchange", change);
    return () => window.removeEventListener("hashchange", change);
  }, []);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    document
      .querySelector('meta[name="theme-color"]')
      ?.setAttribute("content", theme === "dark" ? "#141416" : "#ffffff");
    try {
      localStorage.setItem("alight-theme", theme);
    } catch {
      /* Theme remains usable without storage. */
    }
  }, [theme]);
  useEffect(() => {
    const key = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setSearch((v) => !v);
        setMenu(false);
      }
    };
    window.addEventListener("keydown", key);
    return () => window.removeEventListener("keydown", key);
  }, []);
  const reconnectNow = () => {
    setOperatorKey("");
    setDraftKey("");
    setFrozen(null);
    setReconnect((v) => v + 1);
  };
  const go = (id: string) => {
    setMenu(false);
    setSearch(false);
    location.hash = id;
  };
  const sidebar = (
    <>
      <a
        className="brand"
        href="#cockpit"
        onClick={() => setMenu(false)}
        aria-label="Alight cockpit"
      >
        <img src="/alight-mark.svg" alt="" />
        <span>
          Alight<span className="brand-product">WORKBENCH</span>
        </span>
      </a>
      <div className="workspace-label">
        <span className="workspace-avatar">
          <Radio size={16} aria-hidden="true" />
        </span>
        <div>
          <strong>Solana observatory</strong>
          <span>
            {data?.health.region ?? "Local connection"}
            <span className="workspace-network">
              {source === "sim"
                ? " / Simulation"
                : source === "replay"
                  ? " / Recorded evidence"
                  : source === "live"
                    ? " / Mainnet evidence"
                    : " / Source unknown"}
            </span>
          </span>
        </div>
      </div>
      <button
        className="workspace-search"
        onClick={() => {
          setSearch(true);
          setMenu(false);
        }}
      >
        <Search size={15} aria-hidden="true" />
        <span>Go to…</span>
        <kbd>⌘ K</kbd>
      </button>
      <span className="nav-label">WORKSPACE</span>
      <nav aria-label="Workbench">
        {navigation.map(({ id, name, icon: Icon }) => (
          <a
            key={id}
            href={`#${id}`}
            aria-current={page === id ? "page" : undefined}
            onClick={() => setMenu(false)}
            className={page === id ? "selected" : ""}
          >
            <Icon size={17} aria-hidden />
            <span>{name}</span>
            {id === "prove" &&
              data?.proves.some(
                (p) => !["COMPLETE", "VOIDED"].includes(p.state),
              ) && <span className="nav-count">Active</span>}
          </a>
        ))}
      </nav>
      <div className="sidebar-bottom">
        <a
          href="https://github.com/hicksonhaziel/Alight"
          target="_blank"
          rel="noreferrer"
        >
          <Command size={16} />
          Source code
          <ArrowUpRight size={14} />
        </a>
        <a href="/v1/openapi.json" target="_blank" rel="noreferrer">
          <CircleHelp size={16} />
          API contract
          <ArrowUpRight size={14} />
        </a>
        <div className="connection-card">
          <span className={`connection-indicator ${connection}`} />
          <div>
            <strong>
              {connection === "connected"
                ? "Stream connected"
                : connection === "paused"
                  ? "Updates paused"
                  : connection === "reconnecting"
                    ? "Reconnecting"
                    : connection === "offline"
                      ? "API offline"
                      : "Connecting"}
            </strong>
            <span>
              {data?.health.source === "sim"
                ? "Local simulation · no sends"
                : data
                  ? `${data.health.mode} · ${data.health.region}`
                  : "Waiting for source metadata"}
            </span>
          </div>
        </div>
      </div>
    </>
  );
  return (
    <div className="app">
      <a
        className="skip"
        href="#main"
        onClick={(e) => {
          e.preventDefault();
          document.getElementById("main")?.focus();
        }}
      >
        Skip to content
      </a>
      <aside className="sidebar desktop-sidebar">{sidebar}</aside>
      <Dialog open={menu} close={() => setMenu(false)} title="Navigation">
        <div className="mobile-sidebar">{sidebar}</div>
      </Dialog>
      <Dialog
        open={search}
        close={() => setSearch(false)}
        title="Go to a screen"
      >
        <label className="command-search">
          <Search size={18} />
          <span className="sr-only">Search screens</span>
          <input
            autoFocus
            placeholder="Search screens…"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
          />
        </label>
        <div className="command-results">
          {navigation
            .filter((n) =>
              `${n.name} ${n.description}`
                .toLowerCase()
                .includes(query.toLowerCase()),
            )
            .map((n) => (
              <button key={n.id} onClick={() => go(n.id)}>
                <n.icon size={19} />
                <div>
                  <strong>{n.name}</strong>
                  <span>{n.description}</span>
                </div>
                <ArrowUpRight size={14} />
              </button>
            ))}
        </div>
        <div className="command-foot">
          <kbd>Tab</kbd> Navigate <kbd>Enter</kbd> Open <kbd>Esc</kbd> Close
        </div>
      </Dialog>
      <Dialog
        open={operatorOpen}
        close={() => {
          setOperatorOpen(false);
          setDraftKey("");
        }}
        title="Operator access"
      >
        <form
          className="operator-form"
          onSubmit={(e) => {
            e.preventDefault();
            setOperatorKey(draftKey);
            setDraftKey("");
            setOperatorOpen(false);
          }}
        >
          <p>
            Use the Alight API operator key to lock forecasts and start governed
            Prove runs. Public reads work without it.
          </p>
          <label>
            Operator key
            <input
              type="password"
              autoComplete="off"
              minLength={16}
              maxLength={256}
              pattern="[!-~]{16,256}"
              value={draftKey}
              onChange={(e) => setDraftKey(e.target.value)}
              required
            />
          </label>
          <p className="field-hint">
            Kept in memory for this page. Cleared on reload or disconnect.
            Provider and wallet keys stay on the server.
          </p>
          <button className="primary wide">
            <KeyRound size={16} />
            Connect operator
          </button>
        </form>
      </Dialog>
      <div className="workspace">
        <header className="topbar">
          <div className="breadcrumbs">
            <button
              className="icon-button mobile-toggle"
              aria-label="Open navigation"
              onClick={() => setMenu(true)}
            >
              <Menu size={19} />
            </button>
            <span className="crumb-project">Workbench</span>
            <span className="crumb-divider">/</span>
            <strong>{selected?.name ?? "Page not found"}</strong>
          </div>
          <div className="topbar-actions">
            <span
              className={`source-badge ${data?.health.source ?? "unknown"}`}
            >
              {data
                ? data.health.source === "sim"
                  ? "SIMULATED"
                  : data.health.source.toUpperCase()
                : "SOURCE UNKNOWN"}
            </span>
            <span className="topbar-divider" />
            <button
              className="icon-button"
              aria-label={
                theme === "dark" ? "Use light theme" : "Use dark theme"
              }
              title={theme === "dark" ? "Light theme" : "Dark theme"}
              onClick={() => setTheme((t) => (t === "dark" ? "light" : "dark"))}
            >
              {theme === "dark" ? <Sun size={17} /> : <Moon size={17} />}
            </button>
            <button
              className={`operator-button ${operator ? "authorized" : ""}`}
              aria-label={operator ? "Disconnect operator" : "Operator access"}
              disabled={!data?.evidence.operator_enabled}
              onClick={() =>
                operator ? setOperatorKey("") : setOperatorOpen(true)
              }
            >
              <KeyRound size={15} />
              <span>
                {operator ? "Disconnect operator" : "Operator access"}
              </span>
            </button>
          </div>
        </header>
        <main id="main" tabIndex={-1}>
          {data && connection === "reconnecting" && (
            <div className="stale-bar" role="status">
              Updates interrupted. Values are the last received evidence.
              <button onClick={reconnectNow}>
                Reconnect
                <RefreshCw size={13} />
              </button>
            </div>
          )}
          <div className="page-content" key={`${page}-${reference ?? ""}`}>
            {error && <Notice danger>{error}</Notice>}
            {!data ? (
              error ? (
                <div className="connect-screen">
                  <div className="connect-art">
                    <img src="/alight-mark.svg" alt="" />
                    <span>ALIGHT / OBSERVATORY</span>
                  </div>
                  <div>
                    <span className="eyebrow">LOCAL WORKBENCH</span>
                    <h1>Connect to your observatory.</h1>
                    <p>
                      The workbench needs a running Alight API. Sim mode
                      provides reproducible landing evidence without provider
                      traffic.
                    </p>
                    <button className="primary" onClick={reconnectNow}>
                      <Unplug size={16} />
                      Reconnect
                    </button>
                    <code>npm --prefix web run dev</code>
                  </div>
                </div>
              ) : (
                <Loading text="Connecting to the observatory…" />
              )
            ) : !selected ? (
              <Empty
                title="Page not found"
                text="Choose a screen from the navigation."
              >
                <a href="#cockpit" className="button primary">
                  Open cockpit
                </a>
              </Empty>
            ) : page === "quote" ? (
              <Quote
                data={data}
                operator={operator}
                connect={() => setOperatorOpen(true)}
                locked={(entry) => {
                  setFrozen(entry);
                  location.hash = `prove/${encodeURIComponent(entry.hash)}`;
                }}
              />
            ) : page === "prove" ? (
              <Prove
                data={data}
                operator={operator}
                connect={() => setOperatorOpen(true)}
                initial={frozen}
                reference={reference}
              />
            ) : page === "ledger" ? (
              <Ledger data={data} />
            ) : page === "regimes" ? (
              <Regimes data={data} />
            ) : page === "health" ? (
              <Health data={data} reference={reference} />
            ) : page === "tape" ? (
              <Tape data={data} />
            ) : (
              <Cockpit data={data} />
            )}
          </div>
        </main>
        <footer className="workspace-footer">
          <span>
            <img src="/alight-mark.svg" alt="" />
            Alight
          </span>
          <span>Solana landing observatory.</span>
          <span className="mono">v0.0.0</span>
        </footer>
      </div>
    </div>
  );
}
