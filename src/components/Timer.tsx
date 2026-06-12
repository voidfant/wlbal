import { Config } from "../App";
import { TimerState } from "../hooks/useTimer";

export function Timer({ state, config }: { state: TimerState; config: Config }) {
  const progress = state.total_secs > 0 ? 1 - state.remaining_secs / state.total_secs : 0;
  const sessionSlot = config.schedule.sessions_before_long_break || 4;
  const dotCount = Math.min(sessionSlot, 4);
  const filled = state.session_count % sessionSlot;
  const accent = state.phase === "Work" ? "bg-work" : "bg-leisure";

  return (
    <div className="flex h-full items-center justify-center px-10">
      <section className="w-full max-w-4xl animate-enter">
        <div className="mb-10 flex items-center justify-center gap-3">
          {Array.from({ length: dotCount }).map((_, index) => (
            <span
              key={index}
              className={`h-3 w-3 ${index < filled ? accent : "bg-line"}`}
            />
          ))}
        </div>

        <div className="text-center">
          <div className={`font-mono text-5xl font-bold uppercase ${state.phase === "Work" ? "text-work" : "text-leisure"}`}>
            {state.phase}
          </div>
          <div className="mt-4 font-mono text-[clamp(6rem,17vw,13rem)] font-semibold leading-none tracking-normal">
            {formatClock(state.remaining_secs)}
          </div>
          {state.override_active && (
            <div className="mx-auto mt-5 w-max border border-line bg-surface px-3 py-1 font-mono text-xs uppercase text-[#cfcfcf]">
              Override: {formatClock(state.override_remaining_secs ?? state.remaining_secs)} remaining
            </div>
          )}
        </div>

        <div className="mx-auto mt-10 h-4 w-full max-w-3xl bg-line">
          <div className={`h-full ${accent} transition-[width] duration-500`} style={{ width: `${Math.max(0, Math.min(100, progress * 100))}%` }} />
        </div>

        <div className="mx-auto mt-8 flex max-w-3xl justify-between font-mono text-sm uppercase text-[#9a9a9a]">
          <span>Session {state.session_count % sessionSlot || sessionSlot} of {sessionSlot}</span>
          <span>Next: {state.next_phase} ({Math.round(state.next_duration_secs / 60)}m)</span>
        </div>
      </section>
    </div>
  );
}

function formatClock(secs: number) {
  const hours = Math.floor(secs / 3600);
  const minutes = Math.floor((secs % 3600) / 60);
  const seconds = Math.floor(secs % 60);
  if (hours > 0) {
    return `${hours}:${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
  }
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}
