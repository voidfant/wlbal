import { Minus, Plus } from "lucide-react";
import { useState } from "react";
import { Config } from "../App";
import { Phase } from "../hooks/useTimer";

const presets = {
  "Social Media": ["twitter.com", "reddit.com", "instagram.com", "tiktok.com", "facebook.com"],
  News: ["cnn.com", "bbc.com", "nytimes.com"],
  Entertainment: ["youtube.com", "netflix.com", "twitch.tv"],
};

export function WebManager({
  config,
  setConfig,
  activePhase,
  compact,
}: {
  config: Config;
  setConfig: (config: Config) => void;
  activePhase: Phase;
  compact?: boolean;
}) {
  const [input, setInput] = useState("");

  const addDomains = (phase: "work_blocked" | "leisure_blocked", raw: string) => {
    const domains = raw
      .split(/\n|,|\s+/)
      .map(normalizeDomain)
      .filter((value): value is string => Boolean(value));
    const next = Array.from(new Set([...config.sites[phase], ...domains])).sort();
    setConfig({ ...config, sites: { ...config.sites, [phase]: next } });
    setInput("");
  };

  const removeDomain = (phase: "work_blocked" | "leisure_blocked", domain: string) => {
    setConfig({
      ...config,
      sites: {
        ...config.sites,
        [phase]: config.sites[phase].filter((item) => item !== domain),
      },
    });
  };

  return (
    <section className={`${compact ? "" : "flex min-w-0 flex-col"}`}>
      {!compact && (
        <div className="border-b border-line px-6 py-5">
          <div className="mb-4 flex items-center justify-between">
            <h2 className="font-mono text-xl font-semibold uppercase">Websites</h2>
            <span className="font-mono text-xs uppercase text-muted">Active: {activePhase}</span>
          </div>
          <div className="flex gap-2">
            <textarea
              className="domain-input"
              value={input}
              onChange={(event) => setInput(event.target.value)}
              placeholder="reddit.com&#10;youtube.com"
            />
            <div className="flex w-40 flex-col gap-2">
              <button className="primary-action" onClick={() => addDomains("work_blocked", input)}>
                <Plus size={16} /> Work
              </button>
              <button className="secondary-action" onClick={() => addDomains("leisure_blocked", input)}>
                <Plus size={16} /> Leisure
              </button>
            </div>
          </div>
        </div>
      )}

      <div className={`${compact ? "" : "min-h-0 flex-1 overflow-auto"} p-6`}>
        <div className="mb-5 flex flex-wrap gap-2">
          {Object.entries(presets).map(([label, domains]) => (
            <button key={label} className="preset-button" onClick={() => addDomains("work_blocked", domains.join("\n"))}>
              {label}
            </button>
          ))}
        </div>

        {compact && (
          <div className="mb-6 flex gap-2">
            <textarea
              className="domain-input"
              value={input}
              onChange={(event) => setInput(event.target.value)}
              placeholder="Paste domains"
            />
            <button className="primary-action self-stretch" onClick={() => addDomains("work_blocked", input)}>
              <Plus size={16} /> Add
            </button>
          </div>
        )}

        <div className="grid grid-cols-2 gap-5">
          <DomainList title="Work Blocklist" domains={config.sites.work_blocked} onRemove={(domain) => removeDomain("work_blocked", domain)} />
          <DomainList title="Leisure Blocklist" domains={config.sites.leisure_blocked} onRemove={(domain) => removeDomain("leisure_blocked", domain)} />
        </div>
      </div>
    </section>
  );
}

function DomainList({ title, domains, onRemove }: { title: string; domains: string[]; onRemove: (domain: string) => void }) {
  return (
    <div className="border border-line bg-[#121212] p-4">
      <div className="mb-3 font-mono text-sm uppercase text-[#d0d0d0]">{title}</div>
      <div className="space-y-2">
        {domains.map((domain) => (
          <div key={domain} className="domain-row">
            <span>{domain}</span>
            <button className="icon-button" onClick={() => onRemove(domain)} title="Remove domain">
              <Minus size={15} />
            </button>
          </div>
        ))}
      </div>
    </div>
  );
}

function normalizeDomain(raw: string) {
  let value = raw.trim().toLowerCase();
  value = value.replace(/^https?:\/\//, "");
  value = value.replace(/^www\./, "");
  value = value.split("/")[0].trim().replace(/\.+$/, "");
  if (!value || value.includes(" ") || !value.includes(".")) return null;
  return value;
}
