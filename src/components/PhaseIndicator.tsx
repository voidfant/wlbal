import { TimerState } from "../hooks/useTimer";

export function PhaseIndicator({
  state,
  enforcementEnabled,
}: {
  state: TimerState;
  enforcementEnabled: boolean;
}) {
  const label = state.paused ? "Paused" : enforcementEnabled ? state.phase : "Disarmed";

  return (
    <div className="flex items-center gap-2 border border-line px-3 py-2 font-mono text-xs uppercase text-[#cfcfcf]">
      <span
        className={`h-2 w-2 ${
          state.paused || !enforcementEnabled ? "bg-muted" : state.phase === "Work" ? "bg-work" : "bg-leisure"
        }`}
      />
      <span>{label}</span>
    </div>
  );
}
