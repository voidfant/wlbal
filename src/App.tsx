import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Check, Clock3, ListChecks, MessageCircle, Play, Settings as SettingsIcon, Shield, Terminal, type LucideIcon } from "lucide-react";
import type { ReactNode } from "react";
import { useEffect, useMemo, useState } from "react";
import { AppManager } from "./components/AppManager";
import { PhaseIndicator } from "./components/PhaseIndicator";
import { Settings } from "./components/Settings";
import { TelegramManager } from "./components/TelegramManager";
import { TelegramSurface } from "./components/TelegramSurface";
import { Timer } from "./components/Timer";
import { WebManager } from "./components/WebManager";
import { TimerState, useTimer } from "./hooks/useTimer";

export type AppInfo = {
  name: string;
  bundle_id: string;
  path: string;
  executable: string;
  icon_path?: string | null;
};

export type Config = {
  schedule: {
    mode: "pomodoro" | "custom";
    work_mins: number;
    leisure_mins: number;
    long_break_mins: number;
    sessions_before_long_break: number;
  };
  apps: {
    work_blocked: string[];
    leisure_blocked: string[];
    work_allowed_only: string[];
    leisure_allowed_only: string[];
  };
  sites: {
    work_blocked: string[];
    leisure_blocked: string[];
    hosts_blocking_enabled: boolean;
  };
  telegram: {
    enabled: boolean;
    block_official_clients_during_work: boolean;
    api_id?: number | null;
    api_hash: string;
    work_allowed_chats: Array<{
      id: string;
      title: string;
    }>;
  };
  override: {
    resume_after_override: "continue" | "fresh_cycle";
  };
  notifications: boolean;
  log_blocked_attempts: boolean;
  onboarding: {
    completed: boolean;
  };
  actions: {
    get_to_work_script: string;
  };
};

type Tab = "timer" | "rules" | "telegram" | "settings";

const tabs: Array<{ id: Tab; label: string; icon: LucideIcon }> = [
  { id: "timer", label: "Timer", icon: Clock3 },
  { id: "rules", label: "Rules", icon: ListChecks },
  { id: "telegram", label: "Telegram", icon: MessageCircle },
  { id: "settings", label: "Settings", icon: SettingsIcon },
];

const defaultConfig: Config = {
  schedule: {
    mode: "pomodoro",
    work_mins: 25,
    leisure_mins: 5,
    long_break_mins: 15,
    sessions_before_long_break: 4,
  },
  apps: {
    work_blocked: [],
    leisure_blocked: [],
    work_allowed_only: [],
    leisure_allowed_only: [],
  },
  sites: {
    work_blocked: ["reddit.com", "twitter.com", "youtube.com"],
    leisure_blocked: [],
    hosts_blocking_enabled: false,
  },
  telegram: {
    enabled: false,
    block_official_clients_during_work: true,
    api_id: null,
    api_hash: "",
    work_allowed_chats: [],
  },
  override: {
    resume_after_override: "continue",
  },
  notifications: true,
  log_blocked_attempts: true,
  onboarding: {
    completed: false,
  },
  actions: {
    get_to_work_script: "",
  },
};

export default function App() {
  const { state, flashPhase } = useTimer();
  const [tab, setTab] = useState<Tab>("timer");
  const [config, setConfig] = useState<Config>(defaultConfig);
  const [installedApps, setInstalledApps] = useState<AppInfo[]>([]);
  const [enforcementEnabled, setEnforcementEnabled] = useState(false);
  const [saveState, setSaveState] = useState<"idle" | "saving" | "saved" | "error">("idle");
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    invoke<Config>("get_config").then(setConfig).catch((err) => setError(String(err)));
    invoke<boolean>("get_enforcement_enabled").then(setEnforcementEnabled).catch(() => undefined);
    invoke<AppInfo[]>("get_installed_apps").then(setInstalledApps).catch(() => undefined);

    const configChanged = listen<Config>("config-changed", (event) => setConfig(event.payload));
    const appError = listen<string>("wlbal-error", (event) => setError(event.payload));
    return () => {
      configChanged.then((off) => off());
      appError.then((off) => off());
    };
  }, []);

  const completeOnboarding = async () => {
    const next = { ...config, onboarding: { completed: true } };
    setConfig(next);
    await saveConfig(next);
    await setEnforcement(true);
  };

  async function setEnforcement(enabled: boolean) {
    try {
      if (enabled) {
        await saveConfig(config);
      }
      const next = await invoke<boolean>("set_enforcement_enabled", { enabled });
      setEnforcementEnabled(next);
    } catch (err) {
      setError(String(err));
    }
  }

  async function saveConfig(next = config) {
    setSaveState("saving");
    try {
      await invoke("save_config", { config: next });
      setSaveState("saved");
      window.setTimeout(() => setSaveState("idle"), 1200);
    } catch (err) {
      setSaveState("error");
      setError(String(err));
    }
  }

  async function runGetToWork() {
    try {
      await saveConfig(config);
      const result = await invoke<string>("run_get_to_work_script");
      setError(result);
      window.setTimeout(() => setError(null), 1800);
    } catch (err) {
      setError(String(err));
    }
  }

  const accent = state.phase === "Work" ? "work" : "leisure";
  const showOnboarding = !config.onboarding.completed;

  const startWindowDrag = (event: React.MouseEvent<HTMLElement>) => {
    if (event.button !== 0) return;
    const target = event.target as HTMLElement;
    if (target.closest("button, input, textarea, select, a, [data-no-drag]")) return;
    getCurrentWindow().startDragging().catch(() => undefined);
  };

  return (
    <div className="min-h-screen bg-ink text-[#F0F0F0]">
      {flashPhase && <div className={`phase-flash ${flashPhase === "Work" ? "bg-work" : "bg-leisure"}`} />}
      <div className="grid min-h-screen grid-rows-[64px_1fr]">
        <header
          className="app-titlebar flex items-center justify-between border-b border-line bg-[#111] px-6"
          onMouseDown={startWindowDrag}
        >
          <div className="flex items-center gap-3">
            <div className={`h-3 w-3 ${state.phase === "Work" ? "bg-work" : "bg-leisure"}`} />
            <div>
              <div className="font-mono text-xl font-semibold tracking-normal">wlbal</div>
              <div className="text-xs uppercase text-muted">Work-Leisure Balance</div>
            </div>
          </div>

          <nav className="flex items-center gap-1">
            {tabs.map((item) => {
              const Icon = item.icon;
              const active = tab === item.id;
              return (
                <button
                  key={item.id}
                  className={`nav-button ${active ? `nav-button-active nav-${accent}` : ""}`}
                  onClick={() => setTab(item.id)}
                  title={item.label}
                >
                  <Icon size={17} />
                  <span>{item.label}</span>
                </button>
              );
            })}
          </nav>

          <div className="flex items-center gap-3">
            <PhaseIndicator state={state} enforcementEnabled={enforcementEnabled} />
            <button className="secondary-action" onClick={runGetToWork} title="Run get-to-work script">
              <Play size={16} />
              <span>Get to Work</span>
            </button>
            <button
              className={enforcementEnabled ? "secondary-action" : "primary-action"}
              onClick={() => setEnforcement(!enforcementEnabled)}
              title={enforcementEnabled ? "Suspend enforcement" : "Arm enforcement"}
            >
              <Shield size={16} />
              <span>{enforcementEnabled ? "Disarm" : "Arm"}</span>
            </button>
            <button className="save-button" onClick={() => saveConfig()} disabled={saveState === "saving"}>
              <Check size={16} />
              <span>{saveState === "saving" ? "Saving" : saveState === "saved" ? "Saved" : "Save"}</span>
            </button>
          </div>
        </header>

        <main className="relative overflow-hidden">
          {error && (
            <div className="absolute left-6 right-6 top-4 z-20 flex items-center justify-between border border-work bg-[#210c10] px-4 py-3 text-sm text-[#ffd6da]">
              <span>{error}</span>
              <button className="text-[#ffd6da]" onClick={() => setError(null)}>Dismiss</button>
            </div>
          )}

          {showOnboarding ? (
            <Onboarding
              config={config}
              setConfig={setConfig}
              installedApps={installedApps}
              onFinish={completeOnboarding}
            />
          ) : tab === "timer" ? (
            <Timer state={state} config={config} />
          ) : tab === "rules" ? (
            <Rules
              config={config}
              setConfig={setConfig}
              installedApps={installedApps}
              state={state}
            />
          ) : tab === "telegram" ? (
            <TelegramSurface config={config} setConfig={setConfig} />
          ) : (
            <Settings config={config} setConfig={setConfig} />
          )}
        </main>
      </div>
    </div>
  );
}

function Rules({
  config,
  setConfig,
  installedApps,
  state,
}: {
  config: Config;
  setConfig: (config: Config) => void;
  installedApps: AppInfo[];
  state: TimerState;
}) {
  return (
    <div className="grid h-full grid-cols-[1.1fr_1fr_0.95fr] gap-0">
      <AppManager config={config} setConfig={setConfig} installedApps={installedApps} activePhase={state.phase} />
      <WebManager config={config} setConfig={setConfig} activePhase={state.phase} />
      <TelegramManager config={config} setConfig={setConfig} activePhase={state.phase} />
    </div>
  );
}

function Onboarding({
  config,
  setConfig,
  installedApps,
  onFinish,
}: {
  config: Config;
  setConfig: (config: Config) => void;
  installedApps: AppInfo[];
  onFinish: () => void;
}) {
  const [step, setStep] = useState(0);
  const [permission, setPermission] = useState<string>("Not checked");
  const commonApps = useMemo(
    () =>
      installedApps.filter((app) =>
        /spotify|discord|steam|telegram|slack|messages|music|tv|chrome|firefox/i.test(
          `${app.name} ${app.bundle_id}`,
        ),
      ),
    [installedApps],
  );

  const steps = ["Welcome", "Permissions", "Schedule", "Apps", "Sites", "Start"];

  const checkPermissions = async () => {
    const result = await invoke<{ hosts_writable: boolean; hosts_message: string }>("check_permissions");
    setPermission(result.hosts_message);
  };

  const installCli = async () => {
    try {
      const path = await invoke<string>("install_cli_binary");
      setPermission(`CLI installed at ${path}`);
    } catch (err) {
      setPermission(String(err));
    }
  };

  return (
    <div className="mx-auto flex h-full max-w-5xl flex-col px-8 py-10">
      <div className="mb-8 flex items-center gap-3">
        {steps.map((label, index) => (
          <button
            key={label}
            className={`h-1 flex-1 ${index <= step ? "bg-work" : "bg-line"}`}
            onClick={() => setStep(index)}
            title={label}
          />
        ))}
      </div>

      <div className="grid flex-1 grid-cols-[280px_1fr] gap-10">
        <aside className="border-r border-line pr-8">
          <div className="mb-4 font-mono text-4xl font-semibold">wlbal</div>
          <div className="text-sm leading-6 text-[#9a9a9a]">
            Strict phases, explicit rules, no skip button. CLI overrides are logged.
          </div>
          <div className="mt-8 space-y-2">
            {steps.map((label, index) => (
              <div key={label} className={`font-mono text-sm ${index === step ? "text-[#F0F0F0]" : "text-muted"}`}>
                {String(index + 1).padStart(2, "0")} {label}
              </div>
            ))}
          </div>
        </aside>

        <section className="min-h-[480px]">
          {step === 0 && (
            <Panel title="Enforced Balance">
              <p className="max-w-xl text-lg leading-8 text-[#cfcfcf]">
                Work blocks leisure rules. Leisure can block work rules or open the machine back up. The phase changes by timer or by CLI only.
              </p>
            </Panel>
          )}
          {step === 1 && (
            <Panel title="Permissions">
              <div className="flex flex-wrap gap-3">
                <button className="primary-action" onClick={checkPermissions}><Shield size={17} /> Check Hosts Access</button>
                <button className="secondary-action" onClick={installCli}><Terminal size={17} /> Install CLI</button>
              </div>
              <p className="mt-5 text-sm text-[#b4b4b4]">{permission}</p>
            </Panel>
          )}
          {step === 2 && <Settings config={config} setConfig={setConfig} compact />}
          {step === 3 && (
            <Panel title="Quick App Rules">
              <div className="grid grid-cols-2 gap-3">
                {commonApps.slice(0, 12).map((app) => {
                  const selected = config.apps.work_blocked.includes(app.bundle_id);
                  return (
                    <button
                      key={app.bundle_id}
                      className={`select-row ${selected ? "select-row-active" : ""}`}
                      onClick={() => {
                        const work_blocked = selected
                          ? config.apps.work_blocked.filter((id) => id !== app.bundle_id)
                          : [...config.apps.work_blocked, app.bundle_id];
                        setConfig({ ...config, apps: { ...config.apps, work_blocked } });
                      }}
                    >
                      <span>{app.name}</span>
                      <small>{app.bundle_id}</small>
                    </button>
                  );
                })}
              </div>
            </Panel>
          )}
          {step === 4 && <WebManager config={config} setConfig={setConfig} activePhase="Work" compact />}
          {step === 5 && (
            <Panel title="Start Work Session">
              <p className="max-w-xl text-lg leading-8 text-[#cfcfcf]">
                The first session begins in Work and arms enforcement for this app run. Use <code className="code-chip">wlbal status</code> to inspect it from a terminal.
              </p>
              <button className="primary-action mt-8" onClick={onFinish}>Start wlbal</button>
            </Panel>
          )}
        </section>
      </div>

      <footer className="mt-8 flex justify-between">
        <button className="secondary-action" disabled={step === 0} onClick={() => setStep((value) => Math.max(0, value - 1))}>
          Back
        </button>
        {step < steps.length - 1 && (
          <button className="primary-action" onClick={() => setStep((value) => Math.min(steps.length - 1, value + 1))}>
            Continue
          </button>
        )}
      </footer>
    </div>
  );
}

function Panel({ title, children }: { title: string; children: ReactNode }) {
  return (
    <div className="animate-enter">
      <div className="mb-6 font-mono text-3xl font-semibold uppercase">{title}</div>
      {children}
    </div>
  );
}
