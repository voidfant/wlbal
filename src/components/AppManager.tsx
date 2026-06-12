import { Minus, Plus, Search } from "lucide-react";
import type { ReactNode } from "react";
import { useMemo, useState } from "react";
import { AppInfo, Config } from "../App";
import { Phase } from "../hooks/useTimer";

type AppListKey = "work_blocked" | "leisure_blocked" | "work_allowed_only" | "leisure_allowed_only";

export function AppManager({
  config,
  setConfig,
  installedApps,
  activePhase,
}: {
  config: Config;
  setConfig: (config: Config) => void;
  installedApps: AppInfo[];
  activePhase: Phase;
}) {
  const [query, setQuery] = useState("");
  const [dragged, setDragged] = useState<string | null>(null);

  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return installedApps
      .filter((app) => !q || `${app.name} ${app.bundle_id}`.toLowerCase().includes(q))
      .slice(0, 80);
  }, [installedApps, query]);

  const addTo = (key: AppListKey, bundleId: string) => {
    if (config.apps[key].includes(bundleId)) return;
    const nextApps = {
      ...config.apps,
      [key]: [...config.apps[key], bundleId],
    };

    if (key === "work_blocked") {
      nextApps.work_allowed_only = nextApps.work_allowed_only.filter((id) => id !== bundleId);
    }
    if (key === "work_allowed_only") {
      nextApps.work_blocked = nextApps.work_blocked.filter((id) => id !== bundleId);
    }
    if (key === "leisure_blocked") {
      nextApps.leisure_allowed_only = nextApps.leisure_allowed_only.filter((id) => id !== bundleId);
    }
    if (key === "leisure_allowed_only") {
      nextApps.leisure_blocked = nextApps.leisure_blocked.filter((id) => id !== bundleId);
    }

    setConfig({
      ...config,
      apps: nextApps,
    });
  };

  const removeFrom = (key: AppListKey, bundleId: string) => {
    setConfig({
      ...config,
      apps: {
        ...config.apps,
        [key]: config.apps[key].filter((id) => id !== bundleId),
      },
    });
  };

  const appById = new Map(installedApps.map((app) => [app.bundle_id, app]));

  return (
    <section className="flex min-w-0 flex-col border-r border-line">
      <div className="border-b border-line px-6 py-5">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="font-mono text-xl font-semibold uppercase">Apps</h2>
          <span className="font-mono text-xs uppercase text-muted">Active: {activePhase}</span>
        </div>
        <label className="search-field">
          <Search size={16} />
          <input value={query} onChange={(event) => setQuery(event.target.value)} placeholder="Search installed apps" />
        </label>
      </div>

      <div className="grid min-h-0 flex-1 grid-cols-[1fr_1.1fr]">
        <div className="min-h-0 overflow-auto border-r border-line p-4">
          <div className="mb-3 font-mono text-xs uppercase text-muted">Installed</div>
          <div className="space-y-2">
            {filtered.map((app) => (
              <AppRow
                key={app.bundle_id}
                app={app}
                draggable
                onDragStart={() => setDragged(app.bundle_id)}
                actions={
                  <AppRuleButtons
                    app={app}
                    config={config}
                    addTo={addTo}
                  />
                }
              />
            ))}
          </div>
        </div>

        <div className="min-h-0 overflow-auto p-4">
          <PhaseColumn
            title="Work Phase"
            blockedKey="work_blocked"
            allowedKey="work_allowed_only"
            config={config}
            appById={appById}
            dragged={dragged}
            addTo={addTo}
            removeFrom={removeFrom}
          />
          <div className="my-5 h-px bg-line" />
          <PhaseColumn
            title="Leisure Phase"
            blockedKey="leisure_blocked"
            allowedKey="leisure_allowed_only"
            config={config}
            appById={appById}
            dragged={dragged}
            addTo={addTo}
            removeFrom={removeFrom}
          />
        </div>
      </div>
    </section>
  );
}

function PhaseColumn({
  title,
  blockedKey,
  allowedKey,
  config,
  appById,
  dragged,
  addTo,
  removeFrom,
}: {
  title: string;
  blockedKey: AppListKey;
  allowedKey: AppListKey;
  config: Config;
  appById: Map<string, AppInfo>;
  dragged: string | null;
  addTo: (key: AppListKey, bundleId: string) => void;
  removeFrom: (key: AppListKey, bundleId: string) => void;
}) {
  return (
    <div>
      <div className="mb-3 font-mono text-sm uppercase text-[#d0d0d0]">{title}</div>
      <DropList title="Blocked Apps" onDrop={() => dragged && addTo(blockedKey, dragged)}>
        {config.apps[blockedKey].map((id) => (
          <AppRow
            key={id}
            app={appById.get(id) ?? missingApp(id)}
            actions={
              <button className="icon-button" onClick={() => removeFrom(blockedKey, id)} title="Remove app">
                <Minus size={15} />
              </button>
            }
          />
        ))}
      </DropList>
      <DropList title="Allowed Apps" onDrop={() => dragged && addTo(allowedKey, dragged)}>
        {config.apps[allowedKey].map((id) => (
          <AppRow
            key={id}
            app={appById.get(id) ?? missingApp(id)}
            actions={
              <button className="icon-button" onClick={() => removeFrom(allowedKey, id)} title="Remove app">
                <Minus size={15} />
              </button>
            }
          />
        ))}
      </DropList>
    </div>
  );
}

function DropList({ title, children, onDrop }: { title: string; children: ReactNode; onDrop: () => void }) {
  return (
    <div
      className="mb-4 min-h-[92px] border border-line bg-[#121212] p-3"
      onDragOver={(event) => event.preventDefault()}
      onDrop={(event) => {
        event.preventDefault();
        onDrop();
      }}
    >
      <div className="mb-2 font-mono text-[11px] uppercase text-muted">{title}</div>
      <div className="space-y-2">{children}</div>
    </div>
  );
}

function AppRow({
  app,
  actions,
  draggable,
  onDragStart,
}: {
  app: AppInfo;
  actions: ReactNode;
  draggable?: boolean;
  onDragStart?: () => void;
}) {
  return (
    <div className="app-row" draggable={draggable} onDragStart={onDragStart}>
      <div className="app-glyph">{app.name.slice(0, 2).toUpperCase()}</div>
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm text-[#ededed]">{app.name}</div>
        <div className="truncate font-mono text-[11px] text-muted">{app.bundle_id}</div>
      </div>
      {actions}
    </div>
  );
}

function AppRuleButtons({
  app,
  config,
  addTo,
}: {
  app: AppInfo;
  config: Config;
  addTo: (key: AppListKey, bundleId: string) => void;
}) {
  const buttons: Array<{ key: AppListKey; label: string; title: string }> = [
    { key: "work_blocked", label: "WB", title: "Block during Work" },
    { key: "work_allowed_only", label: "WA", title: "Allow during Work" },
    { key: "leisure_blocked", label: "LB", title: "Block during Leisure" },
    { key: "leisure_allowed_only", label: "LA", title: "Allow during Leisure" },
  ];

  return (
    <div className="app-actions">
      {buttons.map((button) => {
        const active = config.apps[button.key].includes(app.bundle_id);
        return (
          <button
            key={button.key}
            className={`rule-chip ${active ? "rule-chip-active" : ""}`}
            onClick={() => addTo(button.key, app.bundle_id)}
            disabled={active}
            title={button.title}
          >
            {active ? <Minus size={12} /> : <Plus size={12} />}
            <span>{button.label}</span>
          </button>
        );
      })}
    </div>
  );
}

function missingApp(bundleId: string): AppInfo {
  return {
    name: "Missing app",
    bundle_id: bundleId,
    path: "",
    executable: "",
  };
}
