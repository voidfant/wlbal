import { invoke } from "@tauri-apps/api/core";
import { CalendarDays, RefreshCw } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import { TimerState } from "../hooks/useTimer";

type Phase = "Work" | "Leisure";

type StatusEvent = {
  timestamp: string;
  phase: Phase;
  paused: boolean;
  override_active: boolean;
  active: boolean;
};

type Totals = {
  workMs: number;
  leisureMs: number;
  workActiveMs: number;
  leisureActiveMs: number;
  workPauseMs: number;
  leisurePauseMs: number;
  observedMs: number;
};

type DayBucket = Totals & {
  date: Date;
  label: string;
  weekday: number;
  dominant: "work" | "leisure" | "workPause" | "leisurePause" | "none";
};

type Preset = "day" | "week" | "month" | "custom";

const emptyTotals: Totals = {
  workMs: 0,
  leisureMs: 0,
  workActiveMs: 0,
  leisureActiveMs: 0,
  workPauseMs: 0,
  leisurePauseMs: 0,
  observedMs: 0,
};

const statusContinuityLimitMs = 6 * 60 * 1000;

export function Stats({ state }: { state: TimerState }) {
  const todayValue = dateInputValue(new Date());
  const [startDate, setStartDate] = useState(todayValue);
  const [endDate, setEndDate] = useState(todayValue);
  const [activePreset, setActivePreset] = useState<Preset>("day");
  const [events, setEvents] = useState<StatusEvent[]>([]);
  const [selectedDay, setSelectedDay] = useState(todayValue);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [nowMs, setNowMs] = useState(() => Date.now());
  const [reloadKey, setReloadKey] = useState(0);

  const range = useMemo(() => {
    const start = parseDateInput(startDate, "start");
    const end = parseDateInput(endDate, "end");
    if (end.getTime() <= start.getTime()) {
      end.setDate(start.getDate() + 1);
    }
    return { start, end };
  }, [endDate, startDate]);

  useEffect(() => {
    const id = window.setInterval(() => setNowMs(Date.now()), 15_000);
    return () => window.clearInterval(id);
  }, []);

  useEffect(() => {
    let mounted = true;
    setLoading(true);
    invoke<StatusEvent[]>("get_status_events", {
      start: range.start.toISOString(),
      end: range.end.toISOString(),
    })
      .then((next) => {
        if (!mounted) return;
        setEvents(next);
        setError(null);
      })
      .catch((err) => {
        if (!mounted) return;
        setError(String(err));
      })
      .finally(() => {
        if (mounted) setLoading(false);
      });
    return () => {
      mounted = false;
    };
  }, [range.end, range.start, reloadKey, state.override_active, state.paused, state.phase, state.started_at]);

  const cappedEnd = useMemo(() => {
    const now = new Date(nowMs);
    return range.end.getTime() > now.getTime() ? now : range.end;
  }, [nowMs, range.end]);

  const totals = useMemo(
    () => computeTotals(events, range.start, cappedEnd),
    [cappedEnd, events, range.start],
  );

  const days = useMemo(
    () => buildDays(events, range.start, cappedEnd),
    [cappedEnd, events, range.start],
  );

  const selected = useMemo(
    () => days.find((day) => day.label === selectedDay) ?? days[days.length - 1],
    [days, selectedDay],
  );

  const weekColumns = Math.max(1, Math.ceil((days.length + (days[0]?.weekday ?? 0)) / 7));
  const intervalLabel = `${formatShortDate(range.start)} - ${formatShortDate(new Date(cappedEnd.getTime() - 1))}`;

  function applyPreset(preset: Preset) {
    const today = new Date();
    let start = startOfDay(today);
    if (preset === "week") {
      start = startOfWeek(today);
    }
    if (preset === "month") {
      start = new Date(today.getFullYear(), today.getMonth(), 1);
    }
    const nextStart = dateInputValue(start);
    const nextEnd = dateInputValue(today);
    setStartDate(nextStart);
    setEndDate(nextEnd);
    setSelectedDay(nextEnd);
    setActivePreset(preset);
  }

  function setCustomStart(value: string) {
    setActivePreset("custom");
    setStartDate(value);
    if (value > endDate) {
      setEndDate(value);
    }
  }

  function setCustomEnd(value: string) {
    setActivePreset("custom");
    setEndDate(value);
    setSelectedDay(value);
  }

  return (
    <section className="stats-surface animate-enter">
      <header className="stats-header">
        <div>
          <div className="stats-kicker">Status stats</div>
          <h1>Phase history</h1>
          <p>{intervalLabel}</p>
        </div>
        <div className="stats-controls" data-no-drag>
          <div className="stats-presets">
            <button className={activePreset === "day" ? "active" : ""} onClick={() => applyPreset("day")}>Day</button>
            <button className={activePreset === "week" ? "active" : ""} onClick={() => applyPreset("week")}>Week</button>
            <button className={activePreset === "month" ? "active" : ""} onClick={() => applyPreset("month")}>Month</button>
          </div>
          <label className="stats-date-field">
            <span>From</span>
            <input type="date" value={startDate} onChange={(event) => setCustomStart(event.target.value)} />
          </label>
          <label className="stats-date-field">
            <span>To</span>
            <input type="date" value={endDate} onChange={(event) => setCustomEnd(event.target.value)} />
          </label>
          <button
            className="icon-button"
            onClick={() => {
              setNowMs(Date.now());
              setReloadKey((value) => value + 1);
            }}
            title="Refresh stats"
          >
            <RefreshCw size={15} />
          </button>
        </div>
      </header>

      <div className="stats-total-grid">
        <StatBlock label="Work phase" value={formatDuration(totals.workMs)} detail="Active work time" tone="work" />
        <StatBlock label="Leisure phase" value={formatDuration(totals.leisureMs)} detail="Active leisure time" tone="leisure" />
        <StatBlock label="Pause in work" value={formatDuration(totals.workPauseMs)} detail={percent(totals.workPauseMs, totals.observedMs)} tone="workPause" />
        <StatBlock label="Pause in leisure" value={formatDuration(totals.leisurePauseMs)} detail={percent(totals.leisurePauseMs, totals.observedMs)} tone="leisurePause" />
      </div>

      <div className="stats-body">
        <section className="stats-matrix-section">
          <div className="stats-section-heading">
            <CalendarDays size={16} />
            <span>Daily matrix</span>
            {loading && <small>Loading</small>}
            {error && <small>{error}</small>}
          </div>
          <div className="stats-weekdays">
            <span>Mon</span>
            <span>Tue</span>
            <span>Wed</span>
            <span>Thu</span>
            <span>Fri</span>
            <span>Sat</span>
            <span>Sun</span>
          </div>
          <div
            className="stats-heatmap"
            style={{ gridTemplateColumns: `repeat(${weekColumns}, 15px)` }}
          >
            {Array.from({ length: days[0]?.weekday ?? 0 }).map((_, index) => (
              <span key={`pad-${index}`} className="stats-cell stats-cell-empty" />
            ))}
            {days.map((day) => (
              <button
                key={day.label}
                className={`stats-cell stats-cell-${day.dominant} ${selected?.label === day.label ? "stats-cell-selected" : ""}`}
                style={{ "--cell-strength": cellStrength(day), "--cell-bg": cellBackground(day) } as React.CSSProperties}
                onClick={() => setSelectedDay(day.label)}
                title={`${formatShortDate(day.date)} | Work ${formatDuration(day.workMs)} | Leisure ${formatDuration(day.leisureMs)} | Paused ${formatDuration(day.workPauseMs + day.leisurePauseMs)}`}
              />
            ))}
          </div>
          <div className="stats-legend">
            <span>Less</span>
            <i style={{ background: "rgba(46, 196, 182, .24)" }} />
            <i style={{ background: "rgba(46, 196, 182, .42)" }} />
            <i style={{ background: "rgba(230, 57, 70, .55)" }} />
            <i style={{ background: "rgba(230, 57, 70, .8)" }} />
            <span>More</span>
          </div>
        </section>

        <aside className="stats-day-detail">
          <div className="stats-section-heading">
            <span>{selected ? formatShortDate(selected.date) : "No day selected"}</span>
          </div>
          {selected ? (
            <>
              <div className="stats-day-bars">
                <StatusBar label="Work" value={selected.workMs} total={selected.observedMs} tone="work" />
                <StatusBar label="Leisure" value={selected.leisureMs} total={selected.observedMs} tone="leisure" />
                <StatusBar label="Work pause" value={selected.workPauseMs} total={selected.observedMs} tone="workPause" />
                <StatusBar label="Leisure pause" value={selected.leisurePauseMs} total={selected.observedMs} tone="leisurePause" />
              </div>
              <div className="stats-day-total">
                <span>Observed</span>
                <strong>{formatDuration(selected.observedMs)}</strong>
              </div>
            </>
          ) : (
            <p className="stats-empty">No status events in this interval yet.</p>
          )}
        </aside>
      </div>
    </section>
  );
}

function StatBlock({
  label,
  value,
  detail,
  tone,
}: {
  label: string;
  value: string;
  detail: string;
  tone: "work" | "leisure" | "workPause" | "leisurePause";
}) {
  return (
    <div className={`stats-total stats-tone-${tone}`}>
      <span>{label}</span>
      <strong>{value}</strong>
      <small>{detail}</small>
    </div>
  );
}

function StatusBar({
  label,
  value,
  total,
  tone,
}: {
  label: string;
  value: number;
  total: number;
  tone: "work" | "leisure" | "workPause" | "leisurePause";
}) {
  const width = total > 0 ? Math.max(3, (value / total) * 100) : 0;
  return (
    <div className="stats-bar-row">
      <div>
        <span>{label}</span>
        <strong>{formatDuration(value)}</strong>
      </div>
      <div className="stats-bar-track">
        <i className={`stats-bar stats-tone-${tone}`} style={{ width: `${width}%` }} />
      </div>
    </div>
  );
}

function buildDays(events: StatusEvent[], start: Date, end: Date): DayBucket[] {
  const days: DayBucket[] = [];
  let cursor = startOfDay(start);
  while (cursor < end) {
    const next = new Date(cursor);
    next.setDate(cursor.getDate() + 1);
    const bucketStart = cursor < start ? start : cursor;
    const bucketEnd = next > end ? end : next;
    const totals = computeTotals(events, bucketStart, bucketEnd);
    days.push({
      ...totals,
      date: new Date(cursor),
      label: dateInputValue(cursor),
      weekday: mondayIndex(cursor),
      dominant: dominantStatus(totals),
    });
    cursor = next;
  }
  return days;
}

function computeTotals(events: StatusEvent[], start: Date, end: Date): Totals {
  if (end <= start) return { ...emptyTotals };
  const totals = { ...emptyTotals };
  const sorted = [...events].sort((a, b) => Date.parse(a.timestamp) - Date.parse(b.timestamp));
  let active: StatusEvent | null = null;
  let activeTimestamp = 0;
  let cursor = start.getTime();
  const startMs = start.getTime();
  const endMs = end.getTime();

  for (const event of sorted) {
    const timestamp = Date.parse(event.timestamp);
    if (!Number.isFinite(timestamp)) continue;
    if (timestamp <= startMs) {
      active = event;
      activeTimestamp = timestamp;
      continue;
    }
    if (timestamp >= endMs) break;

    if (active && timestamp > cursor) {
      addContinuousDuration(totals, active, cursor, timestamp, activeTimestamp);
    }
    active = event;
    activeTimestamp = timestamp;
    cursor = Math.max(cursor, timestamp);
  }

  if (active && endMs > cursor) {
    addContinuousDuration(totals, active, cursor, endMs, activeTimestamp);
  }

  return totals;
}

function addContinuousDuration(totals: Totals, event: StatusEvent, startMs: number, endMs: number, eventMs: number) {
  const continuousEnd = Math.min(endMs, eventMs + statusContinuityLimitMs);
  if (continuousEnd > startMs) {
    addDuration(totals, event, continuousEnd - startMs);
  }
}

function addDuration(totals: Totals, event: StatusEvent, duration: number) {
  if (!event.active) return;
  totals.observedMs += duration;
  if (event.phase === "Work") {
    if (event.paused) {
      totals.workPauseMs += duration;
    } else {
      totals.workMs += duration;
      totals.workActiveMs += duration;
    }
  } else {
    if (event.paused) {
      totals.leisurePauseMs += duration;
    } else {
      totals.leisureMs += duration;
      totals.leisureActiveMs += duration;
    }
  }
}

function dominantStatus(totals: Totals): DayBucket["dominant"] {
  const values = [
    ["work", totals.workActiveMs] as const,
    ["leisure", totals.leisureActiveMs] as const,
    ["workPause", totals.workPauseMs] as const,
    ["leisurePause", totals.leisurePauseMs] as const,
  ].sort((a, b) => b[1] - a[1]);
  return values[0][1] > 0 ? values[0][0] : "none";
}

function cellStrength(day: DayBucket) {
  const active = day.workActiveMs + day.leisureActiveMs;
  const ratio = day.observedMs > 0 ? active / Math.max(day.observedMs, 60 * 60 * 1000) : 0;
  return String(Math.max(0.18, Math.min(1, ratio)));
}

function cellBackground(day: DayBucket) {
  if (day.observedMs === 0) return "#151515";
  const strength = Number(cellStrength(day));
  const alpha = 0.22 + strength * 0.62;
  if (day.dominant === "work") return `rgba(230, 57, 70, ${alpha})`;
  if (day.dominant === "leisure") return `rgba(46, 196, 182, ${alpha})`;
  if (day.dominant === "workPause") return `rgba(246, 200, 95, ${alpha})`;
  if (day.dominant === "leisurePause") return `rgba(103, 126, 234, ${alpha})`;
  return "#151515";
}

function parseDateInput(value: string, edge: "start" | "end") {
  const [year, month, day] = value.split("-").map(Number);
  const date = new Date(year, month - 1, day);
  if (edge === "end") {
    date.setDate(date.getDate() + 1);
  }
  return date;
}

function dateInputValue(date: Date) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

function startOfDay(date: Date) {
  return new Date(date.getFullYear(), date.getMonth(), date.getDate());
}

function startOfWeek(date: Date) {
  const start = startOfDay(date);
  const day = mondayIndex(start);
  start.setDate(start.getDate() - day);
  return start;
}

function mondayIndex(date: Date) {
  return (date.getDay() + 6) % 7;
}

function formatShortDate(date: Date) {
  return new Intl.DateTimeFormat(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
  }).format(date);
}

function formatDuration(ms: number) {
  const totalMinutes = Math.floor(ms / 60000);
  const hours = Math.floor(totalMinutes / 60);
  const minutes = totalMinutes % 60;
  if (hours > 0) return `${hours}h ${String(minutes).padStart(2, "0")}m`;
  if (minutes > 0) return `${minutes}m`;
  const seconds = Math.floor(ms / 1000);
  return `${seconds}s`;
}

function percent(value: number, total: number) {
  if (total <= 0 || value <= 0) return "0% of observed";
  return `${Math.round((value / total) * 100)}% of observed`;
}
