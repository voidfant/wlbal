import { Config } from "../App";

export function Settings({
  config,
  setConfig,
  compact,
}: {
  config: Config;
  setConfig: (config: Config) => void;
  compact?: boolean;
}) {
  const updateSchedule = (patch: Partial<Config["schedule"]>) => {
    setConfig({ ...config, schedule: { ...config.schedule, ...patch } });
  };

  return (
    <section className={`${compact ? "" : "mx-auto max-w-4xl px-8 py-10"} animate-enter`}>
      <div className="mb-8">
        <h2 className="font-mono text-3xl font-semibold uppercase">Settings</h2>
        <p className="mt-2 max-w-xl text-sm leading-6 text-[#9a9a9a]">
          Configure the schedule and override recovery behavior. Phase switching remains locked to timer expiry and CLI commands.
        </p>
      </div>

      <div className="space-y-8">
        <div>
          <div className="setting-label">Schedule Mode</div>
          <div className="segmented">
            <button
              className={config.schedule.mode === "pomodoro" ? "active" : ""}
              onClick={() => updateSchedule({ mode: "pomodoro", work_mins: 25, leisure_mins: 5, long_break_mins: 15, sessions_before_long_break: 4 })}
            >
              Pomodoro
            </button>
            <button
              className={config.schedule.mode === "custom" ? "active" : ""}
              onClick={() => updateSchedule({ mode: "custom" })}
            >
              Custom
            </button>
          </div>
        </div>

        <div className="grid grid-cols-4 gap-4">
          <NumberField label="Work Minutes" value={config.schedule.work_mins} min={1} max={180} onChange={(work_mins) => updateSchedule({ work_mins })} />
          <NumberField label="Leisure Minutes" value={config.schedule.leisure_mins} min={1} max={180} onChange={(leisure_mins) => updateSchedule({ leisure_mins })} />
          <NumberField label="Long Break" value={config.schedule.long_break_mins} min={1} max={240} onChange={(long_break_mins) => updateSchedule({ long_break_mins })} />
          <NumberField label="Sessions" value={config.schedule.sessions_before_long_break} min={1} max={12} onChange={(sessions_before_long_break) => updateSchedule({ sessions_before_long_break })} />
        </div>

        <div>
          <div className="setting-label">Override Recovery</div>
          <div className="segmented">
            <button
              className={config.override.resume_after_override === "continue" ? "active" : ""}
              onClick={() => setConfig({ ...config, override: { resume_after_override: "continue" } })}
            >
              Continue
            </button>
            <button
              className={config.override.resume_after_override === "fresh_cycle" ? "active" : ""}
              onClick={() => setConfig({ ...config, override: { resume_after_override: "fresh_cycle" } })}
            >
              Fresh Cycle
            </button>
          </div>
        </div>

        <div>
          <div className="setting-label">Get-To-Work Script</div>
          <textarea
            className="script-input"
            value={config.actions.get_to_work_script}
            onChange={(event) =>
              setConfig({
                ...config,
                actions: {
                  ...config.actions,
                  get_to_work_script: event.target.value,
                },
              })
            }
            placeholder={"open -a \"Visual Studio Code\" ~/work\nopen https://linear.app"}
            spellCheck={false}
          />
        </div>

        <div className="grid grid-cols-2 gap-4">
          <Toggle
            label="Notifications"
            checked={config.notifications}
            onChange={(notifications) => setConfig({ ...config, notifications })}
          />
          <Toggle
            label="Log Blocked Attempts"
            checked={config.log_blocked_attempts}
            onChange={(log_blocked_attempts) => setConfig({ ...config, log_blocked_attempts })}
          />
          <Toggle
            label="Use /etc/hosts Blocking"
            checked={config.sites.hosts_blocking_enabled}
            onChange={(hosts_blocking_enabled) =>
              setConfig({
                ...config,
                sites: { ...config.sites, hosts_blocking_enabled },
              })
            }
          />
        </div>
      </div>
    </section>
  );
}

function NumberField({
  label,
  value,
  min,
  max,
  onChange,
}: {
  label: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
}) {
  return (
    <label className="number-field">
      <span>{label}</span>
      <input
        type="number"
        min={min}
        max={max}
        value={value}
        onChange={(event) => onChange(clamp(Number(event.target.value), min, max))}
      />
    </label>
  );
}

function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (checked: boolean) => void }) {
  return (
    <label className="toggle-row">
      <span>{label}</span>
      <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
    </label>
  );
}

function clamp(value: number, min: number, max: number) {
  if (Number.isNaN(value)) return min;
  return Math.max(min, Math.min(max, value));
}
