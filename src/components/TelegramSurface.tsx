import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Lock, MessageCircle, RefreshCw, Send } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { Config } from "../App";

type TelegramStatus = {
  enabled: boolean;
  connected: boolean;
  configured: boolean;
  bridge_running: boolean;
  auth_state: string;
  message: string;
};

type TelegramMessage = {
  id: string;
  chat_id: string;
  sender: string;
  date: number;
  outgoing: boolean;
  text: string;
};

const defaultStatus: TelegramStatus = {
  enabled: false,
  connected: false,
  configured: false,
  bridge_running: false,
  auth_state: "idle",
  message: "Telegram bridge is stopped.",
};

export function TelegramSurface({
  config,
  setConfig,
}: {
  config: Config;
  setConfig: (config: Config) => void;
}) {
  const allowedChats = config.telegram.work_allowed_chats;
  const [status, setStatus] = useState<TelegramStatus>(defaultStatus);
  const [activeChatId, setActiveChatId] = useState(() => allowedChats[0]?.id ?? "");
  const [messages, setMessages] = useState<TelegramMessage[]>([]);
  const [draft, setDraft] = useState("");
  const [phoneNumber, setPhoneNumber] = useState("");
  const [code, setCode] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);

  const activeChat = useMemo(
    () => allowedChats.find((chat) => chat.id === activeChatId) ?? allowedChats[0],
    [activeChatId, allowedChats],
  );

  useEffect(() => {
    if (!activeChatId && allowedChats[0]) {
      setActiveChatId(allowedChats[0].id);
    }
  }, [activeChatId, allowedChats]);

  useEffect(() => {
    refreshStatus();
    const stateChanged = listen<{ status: TelegramStatus }>("telegram-state-changed", (event) => {
      setStatus(event.payload.status);
    });
    const messageReceived = listen<TelegramMessage>("telegram-message", (event) => {
      if (event.payload.chat_id === activeChatId) {
        setMessages((current) => upsertMessages([...current, event.payload]));
      }
    });
    return () => {
      stateChanged.then((off) => off());
      messageReceived.then((off) => off());
    };
  }, [activeChatId]);

  useEffect(() => {
    if (activeChat?.id && status.connected) {
      refreshMessages(activeChat.id);
    } else {
      setMessages([]);
    }
  }, [activeChat?.id, status.connected]);

  const updateTelegram = (patch: Partial<Config["telegram"]>) => {
    setConfig({ ...config, telegram: { ...config.telegram, ...patch } });
  };

  const refreshStatus = async () => {
    try {
      const next = await invoke<TelegramStatus>("get_telegram_status");
      setStatus(next);
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  };

  const startBridge = async () => {
    try {
      const next = await invoke<TelegramStatus>("start_telegram_bridge", {
        apiId: config.telegram.api_id ?? null,
        apiHash: config.telegram.api_hash,
        tdjsonPath: config.telegram.tdjson_path,
      });
      setStatus(next);
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  };

  const refreshMessages = async (chatId = activeChat?.id) => {
    if (!chatId) return;
    try {
      const next = await invoke<TelegramMessage[]>("get_telegram_messages", {
        chatId,
        fromMessageId: 0,
      });
      setMessages(upsertMessages(next));
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  };

  const sendMessage = async () => {
    const text = draft.trim();
    if (!activeChat?.id || !text) return;
    try {
      const sent = await invoke<TelegramMessage>("send_telegram_message", {
        chatId: activeChat.id,
        text,
      });
      setMessages((current) => upsertMessages([...current, sent]));
      setDraft("");
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  };

  const submitPhone = async () => {
    try {
      await invoke("telegram_set_phone_number", { phoneNumber });
      setPhoneNumber("");
      await refreshStatus();
    } catch (err) {
      setError(String(err));
    }
  };

  const submitCode = async () => {
    try {
      await invoke("telegram_check_code", { code });
      setCode("");
      await refreshStatus();
    } catch (err) {
      setError(String(err));
    }
  };

  const submitPassword = async () => {
    try {
      await invoke("telegram_check_password", { password });
      setPassword("");
      await refreshStatus();
    } catch (err) {
      setError(String(err));
    }
  };

  return (
    <section className="grid h-full grid-cols-[300px_1fr]">
      <aside className="min-h-0 border-r border-line">
        <div className="border-b border-line px-6 py-5">
          <div className="mb-4 flex items-center justify-between">
            <h2 className="font-mono text-xl font-semibold uppercase">Telegram</h2>
            <button className="icon-button" onClick={refreshStatus} title="Refresh Telegram">
              <RefreshCw size={15} />
            </button>
          </div>
          <div className={`font-mono text-xs uppercase ${status.connected ? "text-leisure" : "text-muted"}`}>
            {status.connected ? "Connected" : status.bridge_running ? status.auth_state : "Not Connected"}
          </div>
        </div>

        <div className="min-h-0 overflow-auto p-4">
          {allowedChats.length === 0 ? (
            <div className="border border-line bg-[#121212] p-4 text-sm leading-6 text-[#a8a8a8]">
              Select Telegram chats in Rules before they appear here.
            </div>
          ) : (
            <div className="space-y-2">
              {allowedChats.map((chat) => (
                <button
                  key={chat.id}
                  className={`telegram-chat-select ${activeChat?.id === chat.id ? "telegram-chat-select-active" : ""}`}
                  onClick={() => setActiveChatId(chat.id)}
                >
                  <MessageCircle size={16} />
                  <span className="min-w-0">
                    <span className="block truncate">{chat.title}</span>
                    <small className="block truncate">{chat.id}</small>
                  </span>
                </button>
              ))}
            </div>
          )}
        </div>
      </aside>

      <div className="grid min-h-0 grid-rows-[auto_1fr_auto]">
        <header className="border-b border-line px-6 py-5">
          <div className="flex items-center justify-between gap-4">
            <div className="min-w-0">
              <div className="truncate font-mono text-xl font-semibold uppercase">{activeChat?.title ?? "No Chat Selected"}</div>
              <div className="mt-1 truncate font-mono text-xs text-muted">{activeChat?.id ?? "Allowlist is empty"}</div>
            </div>
            <button className="secondary-action" onClick={() => refreshMessages()} disabled={!status.connected || !activeChat}>
              <RefreshCw size={16} />
              Refresh
            </button>
          </div>
          {(error || !status.connected) && (
            <div className="mt-4 border border-line bg-[#121212] p-4 text-sm leading-6 text-[#a8a8a8]">
              {error ?? status.message}
            </div>
          )}
        </header>

        <main className="min-h-0 overflow-auto px-6 py-5">
          {!status.connected ? (
            <AuthPanel
              config={config}
              updateTelegram={updateTelegram}
              status={status}
              startBridge={startBridge}
              phoneNumber={phoneNumber}
              setPhoneNumber={setPhoneNumber}
              submitPhone={submitPhone}
              code={code}
              setCode={setCode}
              submitCode={submitCode}
              password={password}
              setPassword={setPassword}
              submitPassword={submitPassword}
            />
          ) : messages.length === 0 ? (
            <div className="grid h-full place-items-center text-center text-sm text-muted">
              <div>
                <Lock className="mx-auto mb-3" size={24} />
                No messages loaded
              </div>
            </div>
          ) : (
            <div className="space-y-3">
              {messages.map((message) => (
                <div key={message.id} className={`telegram-message ${message.outgoing ? "telegram-message-outgoing" : ""}`}>
                  <div className="mb-1 font-mono text-[11px] uppercase text-muted">
                    {message.outgoing ? "You" : message.sender}
                  </div>
                  <div className="whitespace-pre-wrap text-sm leading-6">{message.text || "[Unsupported message]"}</div>
                </div>
              ))}
            </div>
          )}
        </main>

        <footer className="border-t border-line p-4">
          <div className="flex gap-2">
            <textarea
              className="telegram-compose"
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              placeholder={status.connected && activeChat ? "Message" : "Connect Telegram first"}
              disabled={!status.connected || !activeChat}
              onKeyDown={(event) => {
                if ((event.metaKey || event.ctrlKey) && event.key === "Enter") {
                  sendMessage();
                }
              }}
            />
            <button className="primary-action self-stretch" onClick={sendMessage} disabled={!status.connected || !activeChat || !draft.trim()}>
              <Send size={16} />
              Send
            </button>
          </div>
        </footer>
      </div>
    </section>
  );
}

function AuthPanel({
  config,
  updateTelegram,
  status,
  startBridge,
  phoneNumber,
  setPhoneNumber,
  submitPhone,
  code,
  setCode,
  submitCode,
  password,
  setPassword,
  submitPassword,
}: {
  config: Config;
  updateTelegram: (patch: Partial<Config["telegram"]>) => void;
  status: TelegramStatus;
  startBridge: () => void;
  phoneNumber: string;
  setPhoneNumber: (value: string) => void;
  submitPhone: () => void;
  code: string;
  setCode: (value: string) => void;
  submitCode: () => void;
  password: string;
  setPassword: (value: string) => void;
  submitPassword: () => void;
}) {
  return (
    <div className="mx-auto max-w-xl space-y-5">
      <div>
        <div className="mb-2 font-mono text-3xl font-semibold uppercase">Connect Telegram</div>
        <p className="text-sm leading-6 text-[#a8a8a8]">
          TDLib signs in as your Telegram account locally, then wlbal shows only chats you allow in Rules.
        </p>
      </div>

      <div className="border border-line bg-[#121212] p-4">
        <div className="mb-3 font-mono text-sm uppercase text-[#d0d0d0]">API Credentials</div>
        <div className="space-y-2">
          <input
            className="telegram-input"
            value={config.telegram.api_id ?? ""}
            onChange={(event) => updateTelegram({ api_id: parseApiId(event.target.value) })}
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
          <input
            className="telegram-input"
            value={config.telegram.tdjson_path}
            onChange={(event) => updateTelegram({ tdjson_path: event.target.value })}
            placeholder="/opt/homebrew/lib/libtdjson.dylib"
            spellCheck={false}
          />
          <button className="primary-action" onClick={startBridge}>Start Bridge</button>
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
    </div>
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
    <div className="border border-line bg-[#121212] p-4">
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

function upsertMessages(messages: TelegramMessage[]) {
  return Array.from(new Map(messages.map((message) => [message.id, message])).values()).sort(
    (a, b) => a.date - b.date || Number(a.id) - Number(b.id),
  );
}

function parseApiId(value: string) {
  const trimmed = value.trim();
  if (!trimmed) return null;
  if (!/^\d+$/.test(trimmed)) return null;
  return Number(trimmed);
}
