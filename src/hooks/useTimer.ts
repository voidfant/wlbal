import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useState } from "react";

export type Phase = "Work" | "Leisure";

export type TimerState = {
  phase: Phase;
  remaining_secs: number;
  total_secs: number;
  session_count: number;
  paused: boolean;
  override_active: boolean;
  override_remaining_secs?: number | null;
  next_phase: Phase;
  next_duration_secs: number;
  started_at: string;
  updated_at: string;
};

const fallbackState: TimerState = {
  phase: "Work",
  remaining_secs: 25 * 60,
  total_secs: 25 * 60,
  session_count: 0,
  paused: false,
  override_active: false,
  override_remaining_secs: null,
  next_phase: "Leisure",
  next_duration_secs: 5 * 60,
  started_at: new Date().toISOString(),
  updated_at: new Date().toISOString(),
};

export function useTimer() {
  const [state, setState] = useState<TimerState>(fallbackState);
  const [now, setNow] = useState(() => Date.now());
  const [flashPhase, setFlashPhase] = useState<Phase | null>(null);

  useEffect(() => {
    let mounted = true;
    invoke<TimerState>("get_state")
      .then((next) => {
        if (mounted) setState(next);
      })
      .catch(() => undefined);

    const tick = listen<TimerState>("timer-tick", (event) => setState(event.payload));
    const changed = listen<{ new_phase: Phase }>("phase-changed", (event) => {
      setFlashPhase(event.payload.new_phase);
      window.setTimeout(() => setFlashPhase(null), 420);
    });

    return () => {
      mounted = false;
      tick.then((off) => off());
      changed.then((off) => off());
    };
  }, []);

  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 250);
    return () => window.clearInterval(id);
  }, []);

  const displayedState = useMemo(() => {
    if (state.paused) {
      return state;
    }

    const updatedAt = Date.parse(state.updated_at || state.started_at);
    if (!Number.isFinite(updatedAt)) {
      return state;
    }

    const elapsedSecs = Math.max(0, Math.floor((now - updatedAt) / 1000));
    const remaining = Math.max(0, state.remaining_secs - elapsedSecs);
    return {
      ...state,
      remaining_secs: remaining,
      override_remaining_secs: state.override_active
        ? Math.max(0, (state.override_remaining_secs ?? state.remaining_secs) - elapsedSecs)
        : state.override_remaining_secs,
    };
  }, [now, state]);

  return { state: displayedState, flashPhase };
}
