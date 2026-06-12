import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { CornerUpLeft, Download, ExternalLink, Forward, Lock, MessageCircle, RefreshCw, Search, Send, X } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Config } from "../App";

type TelegramChatRule = Config["telegram"]["work_allowed_chats"][number];

type TelegramChatGroup = {
  id: string;
  title: string;
  rules: TelegramChatRule[];
};

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
  topic_id?: number | null;
  sender: string;
  date: number;
  outgoing: boolean;
  text: string;
  media?: {
    kind: string;
    file_id?: number | null;
    file_name?: string | null;
    mime_type?: string | null;
    local_path?: string | null;
    downloaded?: boolean;
    size?: number | null;
  } | null;
  reply_to_message_id?: string | null;
  reply_to_sender?: string | null;
  forward_label?: string | null;
  reactions: string[];
};

type TelegramDownloadedFile = {
  file_id: number;
  local_path?: string | null;
  downloaded: boolean;
  size?: number | null;
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
  const chatGroups = useMemo(() => groupAllowedChats(allowedChats), [allowedChats]);
  const [activeChatId, setActiveChatId] = useState(() => chatGroups[0]?.id ?? "");
  const [activeChatKey, setActiveChatKey] = useState(() => allowedChats[0] ? ruleKey(allowedChats[0]) : "");
  const [messages, setMessages] = useState<TelegramMessage[]>([]);
  const [loadingInitial, setLoadingInitial] = useState(false);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [hasOlderMessages, setHasOlderMessages] = useState(true);
  const [unreadByChat, setUnreadByChat] = useState<Record<string, number>>({});
  const [draft, setDraft] = useState("");
  const [replyTo, setReplyTo] = useState<TelegramMessage | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [searchResults, setSearchResults] = useState<TelegramMessage[]>([]);
  const [searchLoading, setSearchLoading] = useState(false);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [highlightedMessageId, setHighlightedMessageId] = useState<string | null>(null);
  const [downloadingByFile, setDownloadingByFile] = useState<Record<number, boolean>>({});
  const [phoneNumber, setPhoneNumber] = useState("");
  const [code, setCode] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const composeRef = useRef<HTMLTextAreaElement | null>(null);
  const searchRef = useRef<HTMLInputElement | null>(null);
  const messageScrollRef = useRef<HTMLElement | null>(null);
  const restoreScrollRef = useRef<{ height: number; top: number } | null>(null);
  const loadGenerationRef = useRef(0);

  const activeGroup = useMemo(
    () => chatGroups.find((group) => group.id === activeChatId),
    [activeChatId, chatGroups],
  );
  const activeChat = useMemo(() => {
    const groupDefault = activeGroup?.rules[0];
    return activeGroup?.rules.find((chat) => ruleKey(chat) === activeChatKey) ?? groupDefault ?? allowedChats[0];
  }, [activeChatKey, activeGroup, allowedChats]);
  const activeGroupHasTopics = activeGroup?.rules.some((chat) => chat.topic_id != null) ?? false;

  useEffect(() => {
    if (!activeGroup && chatGroups[0]) {
      setActiveChatId(chatGroups[0].id);
      setActiveChatKey(ruleKey(chatGroups[0].rules[0]));
      return;
    }
    if (activeGroup && !activeGroup.rules.some((chat) => ruleKey(chat) === activeChatKey)) {
      setActiveChatKey(ruleKey(activeGroup.rules[0]));
    }
  }, [activeChatKey, activeGroup, chatGroups]);

  useEffect(() => {
    refreshStatus();
    const stateChanged = listen<{ status: TelegramStatus }>("telegram-state-changed", (event) => {
      setStatus(event.payload.status);
    });
    const messageReceived = listen<TelegramMessage>("telegram-message", (event) => {
      const matchingRule = matchingRuleForMessage(event.payload, allowedChats);
      const key = matchingRule ? ruleKey(matchingRule) : messageKey(event.payload);
      if (activeChat && ruleAllowsMessage(activeChat, event.payload)) {
        const shouldStickToBottom = isNearBottom();
        setMessages((current) => upsertMessages([...current, event.payload]));
        if (shouldStickToBottom) queueScrollToBottom();
        markRead(event.payload.chat_id, [event.payload.id]);
      } else {
        setUnreadByChat((current) => ({ ...current, [key]: (current[key] ?? 0) + 1 }));
      }
    });
    return () => {
      stateChanged.then((off) => off());
      messageReceived.then((off) => off());
    };
  }, [activeChat?.id, activeChat?.topic_id, allowedChats]);

  useEffect(() => {
    if (activeChat?.id && status.connected) {
      refreshMessages(activeChat.id, activeChat.topic_id ?? null);
    } else {
      setMessages([]);
      setHasOlderMessages(true);
      setLoadingInitial(false);
      setLoadingOlder(false);
    }
    setSearchResults([]);
    setSearchQuery("");
    setHighlightedMessageId(null);
  }, [activeChat?.id, activeChat?.topic_id, status.connected]);

  useLayoutEffect(() => {
    const restore = restoreScrollRef.current;
    const scroller = messageScrollRef.current;
    if (!restore || !scroller) return;
    restoreScrollRef.current = null;
    scroller.scrollTop = scroller.scrollHeight - restore.height + restore.top;
  }, [messages]);

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

  const isNearBottom = () => {
    const scroller = messageScrollRef.current;
    if (!scroller) return true;
    return scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 120;
  };

  const queueScrollToBottom = () => {
    window.requestAnimationFrame(() => {
      window.requestAnimationFrame(() => {
        const scroller = messageScrollRef.current;
        if (scroller) {
          scroller.scrollTop = scroller.scrollHeight;
        }
      });
    });
  };

  const refreshMessages = async (chatId = activeChat?.id, topicId = activeChat?.topic_id ?? null) => {
    if (!chatId) return;
    const generation = ++loadGenerationRef.current;
    setLoadingInitial(true);
    setHasOlderMessages(true);
    try {
      const next = await invoke<TelegramMessage[]>("get_telegram_messages", {
        chatId,
        topicId,
        fromMessageId: 0,
      });
      if (generation !== loadGenerationRef.current) return;
      setMessages(upsertMessages(next));
      queueScrollToBottom();
      markRead(chatId, next.map((message) => message.id));
      if (activeChat) {
        setUnreadByChat((current) => ({ ...current, [ruleKey(activeChat)]: 0 }));
      }
      setHasOlderMessages(next.length > 0);
      setError(null);
    } catch (err) {
      if (generation !== loadGenerationRef.current) return;
      setError(String(err));
    } finally {
      if (generation === loadGenerationRef.current) {
        setLoadingInitial(false);
      }
    }
  };

  const loadOlderMessages = async () => {
    if (!activeChat?.id || loadingOlder || loadingInitial || !hasOlderMessages || messages.length === 0) return;
    const scroller = messageScrollRef.current;
    const oldest = messages[0];
    if (!oldest) return;
    setLoadingOlder(true);
    if (scroller) {
      restoreScrollRef.current = { height: scroller.scrollHeight, top: scroller.scrollTop };
    }
    try {
      const older = await invoke<TelegramMessage[]>("get_telegram_messages", {
        chatId: activeChat.id,
        topicId: activeChat.topic_id ?? null,
        fromMessageId: Number(oldest.id),
      });
      const before = new Set(messages.map((message) => message.id));
      if (!older.some((message) => !before.has(message.id))) {
        setHasOlderMessages(false);
      }
      setMessages((current) => upsertMessages([...older, ...current]));
      markRead(activeChat.id, older.map((message) => message.id));
      setError(null);
    } catch (err) {
      restoreScrollRef.current = null;
      setError(String(err));
    } finally {
      setLoadingOlder(false);
    }
  };

  const sendMessage = async () => {
    const text = draft.trim();
    if (!activeChat?.id || !text) return;
    try {
      const sent = await invoke<TelegramMessage>("send_telegram_message", {
        chatId: activeChat.id,
        topicId: activeChat.topic_id ?? null,
        replyToMessageId: replyTo?.id ?? null,
        text,
      });
      setMessages((current) => upsertMessages([...current, sent]));
      queueScrollToBottom();
      setDraft("");
      setReplyTo(null);
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  };

  useEffect(() => {
    const focusComposeOnTyping = (event: KeyboardEvent) => {
      if (!status.connected || !activeChat || event.defaultPrevented) return;
      if (event.metaKey || event.ctrlKey || event.altKey || event.key.length !== 1) return;
      const target = event.target as HTMLElement | null;
      const tagName = target?.tagName;
      if (tagName === "INPUT" || tagName === "TEXTAREA" || target?.isContentEditable) return;
      event.preventDefault();
      composeRef.current?.focus();
      setDraft((current) => current + event.key);
    };
    window.addEventListener("keydown", focusComposeOnTyping);
    return () => window.removeEventListener("keydown", focusComposeOnTyping);
  }, [status.connected, activeChat]);

  useEffect(() => {
    const openSearch = (event: KeyboardEvent) => {
      if (!(event.metaKey || event.ctrlKey) || event.key.toLowerCase() !== "f") return;
      if (!status.connected || !activeChat) return;
      event.preventDefault();
      setSearchOpen(true);
      window.requestAnimationFrame(() => searchRef.current?.focus());
    };
    window.addEventListener("keydown", openSearch);
    return () => window.removeEventListener("keydown", openSearch);
  }, [status.connected, activeChat]);

  useEffect(() => {
    if (!searchOpen) return;
    window.requestAnimationFrame(() => searchRef.current?.focus());
    const closeSearch = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      setSearchOpen(false);
      setHighlightedMessageId(null);
      composeRef.current?.focus();
    };
    window.addEventListener("keydown", closeSearch);
    return () => window.removeEventListener("keydown", closeSearch);
  }, [searchOpen]);

  useEffect(() => {
    if (!searchOpen || !activeChat?.id) return;
    const query = searchQuery.trim();
    if (!query) {
      setSearchResults([]);
      setSearchError(null);
      setSearchLoading(false);
      return;
    }
    const timeout = window.setTimeout(() => {
      searchMessages(query);
    }, 250);
    return () => window.clearTimeout(timeout);
  }, [searchOpen, searchQuery, activeChat?.id, activeChat?.topic_id]);

  const searchMessages = async (query = searchQuery.trim()) => {
    if (!activeChat?.id || !query) return;
    setSearchLoading(true);
    try {
      const results = await invoke<TelegramMessage[]>("search_telegram_messages", {
        chatId: activeChat.id,
        topicId: activeChat.topic_id ?? null,
        query,
        fromMessageId: 0,
      });
      setSearchResults(results);
      setSearchError(null);
    } catch (err) {
      setSearchError(String(err));
    } finally {
      setSearchLoading(false);
    }
  };

  const showSearchResult = (message: TelegramMessage) => {
    setMessages((current) => upsertMessages([...current, message]));
    setHighlightedMessageId(message.id);
    window.requestAnimationFrame(() => {
      document.getElementById(messageElementId(message.id))?.scrollIntoView({ block: "center" });
    });
  };

  const markRead = async (chatId: string, messageIds: string[]) => {
    if (messageIds.length === 0) return;
    try {
      await invoke("mark_telegram_messages_read", { chatId, messageIds });
    } catch {
      // Read markers are best-effort; message viewing should not fail because of them.
    }
  };

  const selectGroup = (group: TelegramChatGroup) => {
    const nextRule = group.rules[0];
    setActiveChatId(group.id);
    setActiveChatKey(ruleKey(nextRule));
    setUnreadByChat((current) => ({ ...current, [ruleKey(nextRule)]: 0 }));
  };

  const reactTo = async (message: TelegramMessage, emoji: string) => {
    if (!emoji) return;
    try {
      await invoke("react_to_telegram_message", { chatId: message.chat_id, messageId: message.id, emoji });
      await refreshMessages();
    } catch (err) {
      setError(String(err));
    }
  };

  const forwardMessage = async (message: TelegramMessage) => {
    const options = allowedChats.map((chat) => `${ruleKey(chat)} ${ruleTitle(chat)}`).join("\n");
    const targetKey = window.prompt(`Forward to allowed chat key:\n${options}`, activeChat ? ruleKey(activeChat) : "");
    if (!targetKey) return;
    const target = allowedChats.find((chat) => ruleKey(chat) === targetKey.trim());
    if (!target) {
      setError("Forward target is not in the allowlist");
      return;
    }
    try {
      const forwarded = await invoke<TelegramMessage[]>("forward_telegram_message", {
        chatId: target.id,
        topicId: target.topic_id ?? null,
        fromChatId: message.chat_id,
        messageId: message.id,
      });
      if (target.id === activeChat?.id && (target.topic_id ?? null) === (activeChat.topic_id ?? null)) {
        setMessages((current) => upsertMessages([...current, ...forwarded]));
      }
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  };

  const downloadMedia = async (message: TelegramMessage) => {
    const fileId = message.media?.file_id;
    if (!fileId) return;
    setDownloadingByFile((current) => ({ ...current, [fileId]: true }));
    try {
      const file = await invoke<TelegramDownloadedFile>("download_telegram_media", { fileId });
      applyDownloadedFile(file);
      setError(null);
    } catch (err) {
      setError(String(err));
    } finally {
      setDownloadingByFile((current) => ({ ...current, [fileId]: false }));
    }
  };

  const openMedia = async (path?: string | null) => {
    if (!path) return;
    try {
      await invoke("open_telegram_media", { path });
      setError(null);
    } catch (err) {
      setError(String(err));
    }
  };

  const applyDownloadedFile = (file: TelegramDownloadedFile) => {
    setMessages((current) =>
      current.map((message) => {
        if (message.media?.file_id !== file.file_id) return message;
        return {
          ...message,
          media: {
            ...message.media,
            local_path: file.local_path ?? message.media.local_path,
            downloaded: file.downloaded,
            size: file.size ?? message.media.size ?? null,
          },
        };
      }),
    );
    setSearchResults((current) =>
      current.map((message) => {
        if (message.media?.file_id !== file.file_id) return message;
        return {
          ...message,
          media: {
            ...message.media,
            local_path: file.local_path ?? message.media.local_path,
            downloaded: file.downloaded,
            size: file.size ?? message.media.size ?? null,
          },
        };
      }),
    );
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
    <section
      className="telegram-surface grid overflow-hidden"
      style={{ gridTemplateColumns: activeGroupHasTopics ? "280px 240px minmax(0, 1fr)" : "300px minmax(0, 1fr)" }}
    >
      <aside className="grid min-h-0 grid-rows-[auto_1fr] border-r border-line">
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
          {chatGroups.length === 0 ? (
            <div className="border border-line bg-[#121212] p-4 text-sm leading-6 text-[#a8a8a8]">
              Select Telegram chats in Rules before they appear here.
            </div>
          ) : (
            <div className="space-y-2">
              {chatGroups.map((group) => {
                const unread = group.rules.reduce((sum, rule) => sum + (unreadByChat[ruleKey(rule)] ?? 0), 0);
                return (
                <button
                  key={group.id}
                  className={`telegram-chat-select ${activeGroup?.id === group.id ? "telegram-chat-select-active" : ""}`}
                  onClick={() => selectGroup(group)}
                >
                  <MessageCircle size={16} />
                  <span className="min-w-0">
                    <span className="block truncate">{group.title}</span>
                    <small className="block truncate">
                      {groupSubtitle(group)}
                    </small>
                  </span>
                  {unread > 0 && <span className="telegram-unread">{unread}</span>}
                </button>
              );
              })}
            </div>
          )}
        </div>
      </aside>

      {activeGroupHasTopics && activeGroup && (
        <aside className="grid min-h-0 grid-rows-[auto_1fr] border-r border-line">
          <div className="border-b border-line px-5 py-5">
            <div className="font-mono text-sm font-semibold uppercase text-[#d0d0d0]">Topics</div>
            <div className="mt-1 truncate font-mono text-xs text-muted">{activeGroup.title}</div>
          </div>
          <div className="min-h-0 overflow-auto p-4">
            <div className="space-y-2">
              {activeGroup.rules.map((chat) => {
                const key = ruleKey(chat);
                const unread = unreadByChat[key] ?? 0;
                return (
                  <button
                    key={key}
                    className={`telegram-chat-select ${activeChat && ruleKey(activeChat) === key ? "telegram-chat-select-active" : ""}`}
                    onClick={() => {
                      setActiveChatKey(key);
                      setUnreadByChat((current) => ({ ...current, [key]: 0 }));
                    }}
                  >
                    <MessageCircle size={16} />
                    <span className="min-w-0">
                      <span className="block truncate">{topicTitle(chat)}</span>
                      <small className="block truncate">{chat.topic_id ? `Topic ${chat.topic_id}` : "Whole chat"}</small>
                    </span>
                    {unread > 0 && <span className="telegram-unread">{unread}</span>}
                  </button>
                );
              })}
            </div>
          </div>
        </aside>
      )}

      <div className="grid min-h-0 overflow-hidden grid-rows-[auto_minmax(0,1fr)_auto]">
        <header className="border-b border-line px-6 py-5">
          <div className="flex items-center justify-between gap-4">
            <div className="min-w-0">
              <div className="truncate font-mono text-xl font-semibold uppercase">{activeGroup?.title ?? "No Chat Selected"}</div>
              <div className="mt-1 truncate font-mono text-xs text-muted">
                {activeChat ? `${topicTitle(activeChat)} · ${activeChat.id}` : "Allowlist is empty"}
              </div>
            </div>
            <div className="flex gap-2">
              <button className="icon-button" onClick={() => setSearchOpen(true)} disabled={!status.connected || !activeChat} title="Search messages">
                <Search size={15} />
              </button>
              <button className="secondary-action" onClick={() => refreshMessages()} disabled={!status.connected || !activeChat}>
                <RefreshCw size={16} />
                Refresh
              </button>
            </div>
          </div>
          {searchOpen && (
            <form
              className="telegram-search-panel"
              onSubmit={(event) => {
                event.preventDefault();
                searchMessages();
              }}
            >
              <Search size={15} />
              <input
                ref={searchRef}
                value={searchQuery}
                onChange={(event) => setSearchQuery(event.target.value)}
                placeholder="Search messages"
              />
              <span className="telegram-search-count">
                {searchLoading ? "Searching" : searchQuery.trim() ? `${searchResults.length} found` : "Cmd+F"}
              </span>
              <button type="button" className="icon-button" onClick={() => {
                setSearchOpen(false);
                setHighlightedMessageId(null);
              }} title="Close search">
                <X size={14} />
              </button>
              {(searchError || searchResults.length > 0) && (
                <div className="telegram-search-results">
                  {searchError ? (
                    <div className="telegram-search-result text-[#e63946]">{searchError}</div>
                  ) : (
                    searchResults.map((message) => (
                      <button key={message.id} type="button" className="telegram-search-result" onClick={() => showSearchResult(message)}>
                        <span>{replyAuthor(message)}</span>
                        <small>{message.text || message.media?.file_name || message.media?.kind || "Message"}</small>
                      </button>
                    ))
                  )}
                </div>
              )}
            </form>
          )}
          {(error || !status.connected) && (
            <div className="mt-4 border border-line bg-[#121212] p-4 text-sm leading-6 text-[#a8a8a8]">
              {error ?? status.message}
            </div>
          )}
        </header>

        <main
          ref={messageScrollRef}
          className="telegram-message-scroll"
          onScroll={(event) => {
            if (event.currentTarget.scrollTop < 140) {
              loadOlderMessages();
            }
          }}
        >
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
          ) : loadingInitial && messages.length === 0 ? (
            <div className="grid h-full place-items-center text-center text-sm text-muted">
              Loading messages...
            </div>
          ) : messages.length === 0 ? (
            <div className="grid h-full place-items-center text-center text-sm text-muted">
              <div>
                <Lock className="mx-auto mb-3" size={24} />
                No messages loaded
              </div>
            </div>
          ) : (
            <div className="space-y-3">
              {loadingOlder && <div className="telegram-history-status">Loading earlier messages...</div>}
              {!hasOlderMessages && <div className="telegram-history-status">Beginning of loaded history</div>}
              {messages.map((message) => (
                <div
                  id={messageElementId(message.id)}
                  key={message.id}
                  className={`telegram-message ${message.outgoing ? "telegram-message-outgoing" : ""} ${highlightedMessageId === message.id ? "telegram-message-search-hit" : ""}`}
                >
                  {message.forward_label && <div className="mb-1 font-mono text-[11px] uppercase text-muted">{message.forward_label}</div>}
                  {message.reply_to_message_id && <div className="telegram-reply-ref">Reply to {replyLabel(message, messages)}</div>}
                  <div className="mb-1 font-mono text-[11px] uppercase text-muted">
                    {message.outgoing ? "You" : message.sender}
                  </div>
                  {message.media && (
                    <TelegramMediaBlock
                      message={message}
                      downloading={Boolean(message.media.file_id && downloadingByFile[message.media.file_id])}
                      onDownload={downloadMedia}
                      onOpen={openMedia}
                    />
                  )}
                  <div className="whitespace-pre-wrap text-sm leading-6">{message.text || "[Unsupported message]"}</div>
                  {message.reactions.length > 0 && <div className="telegram-reactions">{message.reactions.join("  ")}</div>}
                  <div className="telegram-message-actions">
                    <button className="icon-button" onClick={() => setReplyTo(message)} title="Reply">
                      <CornerUpLeft size={14} />
                    </button>
                    <button className="icon-button" onClick={() => forwardMessage(message)} title="Forward">
                      <Forward size={14} />
                    </button>
                    <div className="telegram-reaction-buttons" title="React">
                      {["👍", "🤝", "🔥"].map((emoji) => (
                        <button key={emoji} onClick={() => reactTo(message, emoji)}>{emoji}</button>
                      ))}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </main>

        <footer className="border-t border-line p-4">
          {replyTo && (
            <div className="mb-2 flex items-center justify-between border border-line bg-[#121212] px-3 py-2 text-sm text-[#a8a8a8]">
              <span className="truncate">Replying to {replyAuthor(replyTo)}: {replyTo.text || replyTo.id}</span>
              <button onClick={() => setReplyTo(null)}>Cancel</button>
            </div>
          )}
          <div className="flex gap-2">
            <textarea
              ref={composeRef}
              className="telegram-compose"
              value={draft}
              onChange={(event) => setDraft(event.target.value)}
              placeholder={status.connected && activeChat ? "Message" : "Connect Telegram first"}
              disabled={!status.connected || !activeChat}
              onKeyDown={(event) => {
                if (event.key === "Enter" && !event.shiftKey) {
                  event.preventDefault();
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

function TelegramMediaBlock({
  message,
  downloading,
  onDownload,
  onOpen,
}: {
  message: TelegramMessage;
  downloading: boolean;
  onDownload: (message: TelegramMessage) => void;
  onOpen: (path?: string | null) => void;
}) {
  const media = message.media;
  if (!media) return null;
  const path = media.local_path || null;
  const previewUrl = path && isPreviewableMedia(media) ? convertFileSrc(path) : null;
  const title = media.file_name ?? media.mime_type ?? `file ${media.file_id ?? ""}`;

  return (
    <div className="telegram-media">
      {previewUrl && media.kind === "photo" && <img src={previewUrl} alt={title} />}
      {previewUrl && media.kind === "video" && (
        <video src={previewUrl} controls preload="metadata" />
      )}
      <div className="telegram-media-meta">
        <span>{media.kind}</span>
        <small>{title}{media.size ? ` · ${formatBytes(media.size)}` : ""}</small>
      </div>
      <div className="telegram-media-actions">
        <button className="secondary-action" onClick={() => onDownload(message)} disabled={!media.file_id || downloading}>
          <Download size={14} />
          {downloading ? "Downloading" : path ? "Download again" : "Download"}
        </button>
        {path && (
          <button className="icon-button" onClick={() => onOpen(path)} title="Open file">
            <ExternalLink size={14} />
          </button>
        )}
      </div>
    </div>
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

function groupAllowedChats(chats: TelegramChatRule[]): TelegramChatGroup[] {
  const groups = new Map<string, TelegramChatGroup>();
  for (const chat of chats) {
    const existing = groups.get(chat.id);
    if (existing) {
      existing.rules.push(chat);
      if (!existing.title && chat.title) existing.title = chat.title;
    } else {
      groups.set(chat.id, {
        id: chat.id,
        title: chat.title || chat.id,
        rules: [chat],
      });
    }
  }
  return Array.from(groups.values())
    .map((group) => ({
      ...group,
      rules: [...group.rules].sort((a, b) => topicTitle(a).localeCompare(topicTitle(b))),
    }))
    .sort((a, b) => a.title.localeCompare(b.title));
}

function groupSubtitle(group: TelegramChatGroup) {
  const topicCount = group.rules.filter((rule) => rule.topic_id != null).length;
  if (topicCount === 0) return group.id;
  return `${topicCount} allowed ${topicCount === 1 ? "topic" : "topics"}`;
}

function ruleKey(chat: Pick<Config["telegram"]["work_allowed_chats"][number], "id" | "topic_id">) {
  return `${chat.id}:${chat.topic_id ?? "chat"}`;
}

function messageKey(message: Pick<TelegramMessage, "chat_id" | "topic_id">) {
  return `${message.chat_id}:${message.topic_id ?? "chat"}`;
}

function matchingRuleForMessage(message: Pick<TelegramMessage, "chat_id" | "topic_id">, chats: TelegramChatRule[]) {
  const topicId = message.topic_id ?? null;
  return (
    chats.find((chat) => chat.id === message.chat_id && (chat.topic_id ?? null) === topicId) ??
    chats.find((chat) => chat.id === message.chat_id && chat.topic_id == null)
  );
}

function ruleAllowsMessage(rule: Pick<TelegramChatRule, "id" | "topic_id">, message: Pick<TelegramMessage, "chat_id" | "topic_id">) {
  return rule.id === message.chat_id && (rule.topic_id == null || (message.topic_id ?? null) === rule.topic_id);
}

function replyLabel(message: TelegramMessage, messages: TelegramMessage[]) {
  if (message.reply_to_sender) return message.reply_to_sender;
  const repliedTo = messages.find((candidate) => candidate.id === message.reply_to_message_id);
  if (repliedTo) return replyAuthor(repliedTo);
  return "message";
}

function replyAuthor(message: Pick<TelegramMessage, "outgoing" | "sender">) {
  return message.outgoing ? "You" : message.sender;
}

function ruleTitle(chat: Pick<Config["telegram"]["work_allowed_chats"][number], "title" | "topic_title">) {
  return chat.topic_title ? `${chat.title} / ${chat.topic_title}` : chat.title;
}

function topicTitle(chat: Pick<TelegramChatRule, "topic_title" | "topic_id">) {
  return chat.topic_title ?? (chat.topic_id ? `Topic ${chat.topic_id}` : "All messages");
}

function messageElementId(messageId: string) {
  return `telegram-message-${messageId.replace(/[^a-zA-Z0-9_-]/g, "_")}`;
}

function isPreviewableMedia(media: NonNullable<TelegramMessage["media"]>) {
  return media.kind === "photo" || media.kind === "video";
}

function formatBytes(bytes: number) {
  if (!Number.isFinite(bytes) || bytes <= 0) return "";
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value >= 10 || unitIndex === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[unitIndex]}`;
}
