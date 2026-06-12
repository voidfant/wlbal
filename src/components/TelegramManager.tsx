import { invoke } from "@tauri-apps/api/core";
import { Lock, MessageCircle, Minus, Plus, RefreshCw } from "lucide-react";
import type { ReactNode } from "react";
import { useEffect, useMemo, useState } from "react";
import type { Config } from "../App";
import type { Phase } from "../hooks/useTimer";

type TelegramChatRule = Config["telegram"]["work_allowed_chats"][number];

type TelegramStatus = {
  enabled: boolean;
  connected: boolean;
  configured: boolean;
  bridge_running: boolean;
  auth_state: string;
  message: string;
};

type TelegramChatSummary = {
  id: string;
  title: string;
  selected: boolean;
};

const defaultStatus: TelegramStatus = {
  enabled: false,
  connected: false,
  configured: false,
  bridge_running: false,
  auth_state: "idle",
  message: "Telegram restricted mode is off.",
};

export function TelegramManager({
  config,
  setConfig,
  activePhase,
}: {
  config: Config;
  setConfig: (config: Config) => void;
  activePhase: Phase;
}) {
  const [status, setStatus] = useState<TelegramStatus>(defaultStatus);
  const [chats, setChats] = useState<TelegramChatSummary[]>([]);
  const [chatId, setChatId] = useState("");
  const [chatTitle, setChatTitle] = useState("");
  const [phoneNumber, setPhoneNumber] = useState("");
  const [code, setCode] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);

  const selectedIds = useMemo(
    () => new Set(config.telegram.work_allowed_chats.map((chat) => chat.id)),
    [config.telegram.work_allowed_chats],
  );

  const refreshTelegram = async () => {
    try {
      const [nextStatus, nextChats] = await Promise.all([
        invoke<TelegramStatus>("get_telegram_status"),
        invoke<TelegramChatSummary[]>("get_telegram_chats"),
      ]);
      setStatus(nextStatus);
      setChats(nextChats);
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  };

  useEffect(() => {
    refreshTelegram();
  }, []);

  const updateTelegram = (patch: Partial<Config["telegram"]>) => {
    setConfig({ ...config, telegram: { ...config.telegram, ...patch } });
  };

  const startBridge = async () => {
    try {
      const nextStatus = await invoke<TelegramStatus>("start_telegram_bridge", {
        apiId: config.telegram.api_id ?? null,
        apiHash: config.telegram.api_hash,
      });
      setStatus(nextStatus);
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  };

  const submitPhone = async () => {
    try {
      await invoke("telegram_set_phone_number", { phoneNumber });
      setPhoneNumber("");
      await refreshTelegram();
    } catch (err) {
      setError(String(err));
    }
  };

  const submitCode = async () => {
    try {
      await invoke("telegram_check_code", { code });
      setCode("");
      await refreshTelegram();
    } catch (err) {
      setError(String(err));
    }
  };

  const submitPassword = async () => {
    try {
      await invoke("telegram_check_password", { password });
      setPassword("");
      await refreshTelegram();
    } catch (err) {
      setError(String(err));
    }
  };

  const addChat = (chat: TelegramChatRule) => {
    const normalized = {
      id: chat.id.trim(),
      title: chat.title.trim() || chat.id.trim(),
    };
    if (!normalized.id || selectedIds.has(normalized.id)) return;
    updateTelegram({
      work_allowed_chats: [...config.telegram.work_allowed_chats, normalized].sort((a, b) =>
        a.title.localeCompare(b.title),
      ),
    });
    setChatId("");
    setChatTitle("");
  };

  const removeChat = (id: string) => {
    updateTelegram({
      work_allowed_chats: config.telegram.work_allowed_chats.filter((chat) => chat.id !== id),
    });
  };

  return (
    <section className="flex min-w-0 flex-col border-l border-line">
      <div className="border-b border-line px-6 py-5">
        <div className="mb-4 flex items-center justify-between">
          <h2 className="font-mono text-xl font-semibold uppercase">Telegram</h2>
          <span className="font-mono text-xs uppercase text-muted">Active: {activePhase}</span>
        </div>

        <div className="space-y-3">
          <ToggleRow
            icon={<MessageCircle size={16} />}
            label="Restricted Mode"
            checked={config.telegram.enabled}
            onChange={(enabled) => updateTelegram({ enabled })}
          />
          <ToggleRow
            icon={<Lock size={16} />}
            label="Block Telegram UI"
            checked={config.telegram.block_official_clients_during_work}
            onChange={(block_official_clients_during_work) => updateTelegram({ block_official_clients_during_work })}
          />
        </div>
      </div>

      <div className="min-h-0 flex-1 overflow-auto p-6">
        <div className="mb-5 border border-line bg-[#121212] p-4">
          <div className="mb-3 flex items-center justify-between gap-3">
            <div>
              <div className="font-mono text-sm uppercase text-[#d0d0d0]">Bridge</div>
              <div className={`mt-1 font-mono text-xs ${status.connected ? "text-leisure" : "text-muted"}`}>
                {status.connected ? "Connected" : status.bridge_running ? status.auth_state : status.configured ? "Configured" : "Not Connected"}
              </div>
            </div>
            <div className="flex gap-2">
              <button className="icon-button" onClick={refreshTelegram} title="Refresh Telegram state">
                <RefreshCw size={15} />
              </button>
              <button className="primary-action" onClick={startBridge}>Start</button>
            </div>
          </div>
          <p className="text-sm leading-6 text-[#a8a8a8]">{error ?? status.message}</p>
        </div>

        <div className="mb-5 border border-line bg-[#121212] p-4">
          <div className="mb-3 font-mono text-sm uppercase text-[#d0d0d0]">Credentials</div>
          <div className="space-y-2">
            <input
              className="telegram-input"
              value={config.telegram.api_id ?? ""}
              onChange={(event) =>
                updateTelegram({
                  api_id: parseApiId(event.target.value),
                })
              }
              placeholder="API ID"
              inputMode="numeric"
            />
            <input
              className="telegram-input"
              value={config.telegram.api_hash}
              onChange={(event) => updateTelegram({ api_hash: event.target.value })}
              placeholder="API hash"
              spellCheck={false}
            />
          </div>
        </div>

        {status.auth_state === "authorizationStateWaitPhoneNumber" && (
          <AuthRow value={phoneNumber} onChange={setPhoneNumber} placeholder="Phone number" button="Send" onSubmit={submitPhone} />
        )}
        {status.auth_state === "authorizationStateWaitCode" && (
          <AuthRow value={code} onChange={setCode} placeholder="Login code" button="Verify" onSubmit={submitCode} />
        )}
        {status.auth_state === "authorizationStateWaitPassword" && (
          <AuthRow value={password} onChange={setPassword} placeholder="Password" button="Unlock" onSubmit={submitPassword} password />
        )}

        <div className="mb-5 border border-line bg-[#121212] p-4">
          <div className="mb-3 font-mono text-sm uppercase text-[#d0d0d0]">Allow During Work</div>
          <div className="space-y-2">
            <input
              className="telegram-input"
              value={chatTitle}
              onChange={(event) => setChatTitle(event.target.value)}
              placeholder="Chat title"
            />
            <div className="flex gap-2">
              <input
                className="telegram-input"
                value={chatId}
                onChange={(event) => setChatId(event.target.value)}
                placeholder="Chat ID"
              />
              <button className="primary-action" onClick={() => addChat({ id: chatId, title: chatTitle })}>
                <Plus size={16} />
                Add
              </button>
            </div>
          </div>
        </div>

        <ChatList
          title="Selected Chats"
          chats={config.telegram.work_allowed_chats}
          empty="No chats selected"
          onRemove={removeChat}
        />

        {chats.length > 0 && (
          <div className="mt-5 border border-line bg-[#121212] p-4">
            <div className="mb-3 font-mono text-sm uppercase text-[#d0d0d0]">Known Chats</div>
            <div className="space-y-2">
              {chats.map((chat) => (
                <div key={chat.id} className="telegram-chat-row">
                  <div className="min-w-0 flex-1">
                    <div className="truncate text-sm text-[#ededed]">{chat.title}</div>
                    <div className="truncate font-mono text-[11px] text-muted">{chat.id}</div>
                  </div>
                  <button
                    className="icon-button"
                    onClick={() => addChat({ id: chat.id, title: chat.title })}
                    disabled={selectedIds.has(chat.id)}
                    title="Allow chat"
                  >
                    <Plus size={15} />
                  </button>
                </div>
              ))}
            </div>
          </div>
        )}
      </div>
    </section>
  );
}

function AuthRow({
  value,
  onChange,
  placeholder,
  button,
  onSubmit,
  password,
}: {
  value: string;
  onChange: (value: string) => void;
  placeholder: string;
  button: string;
  onSubmit: () => void;
  password?: boolean;
}) {
  return (
    <div className="mb-5 border border-line bg-[#121212] p-4">
      <div className="mb-3 font-mono text-sm uppercase text-[#d0d0d0]">Authorization</div>
      <div className="flex gap-2">
        <input
          className="telegram-input"
          type={password ? "password" : "text"}
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder={placeholder}
        />
        <button className="primary-action" onClick={onSubmit}>{button}</button>
      </div>
    </div>
  );
}

function ToggleRow({
  icon,
  label,
  checked,
  onChange,
}: {
  icon: ReactNode;
  label: string;
  checked: boolean;
  onChange: (checked: boolean) => void;
}) {
  return (
    <label className="telegram-toggle-row">
      <span className="flex items-center gap-2">
        {icon}
        {label}
      </span>
      <input type="checkbox" checked={checked} onChange={(event) => onChange(event.target.checked)} />
    </label>
  );
}

function ChatList({
  title,
  chats,
  empty,
  onRemove,
}: {
  title: string;
  chats: TelegramChatRule[];
  empty: string;
  onRemove: (id: string) => void;
}) {
  return (
    <div className="border border-line bg-[#121212] p-4">
      <div className="mb-3 font-mono text-sm uppercase text-[#d0d0d0]">{title}</div>
      {chats.length === 0 ? (
        <div className="font-mono text-xs uppercase text-muted">{empty}</div>
      ) : (
        <div className="space-y-2">
          {chats.map((chat) => (
            <div key={chat.id} className="telegram-chat-row">
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm text-[#ededed]">{chat.title}</div>
                <div className="truncate font-mono text-[11px] text-muted">{chat.id}</div>
              </div>
              <button className="icon-button" onClick={() => onRemove(chat.id)} title="Remove chat">
                <Minus size={15} />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function parseApiId(value: string) {
  const trimmed = value.trim();
  if (!trimmed) return null;
  if (!/^\d+$/.test(trimmed)) return null;
  return Number(trimmed);
}
