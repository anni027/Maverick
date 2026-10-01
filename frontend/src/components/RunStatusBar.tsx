import { useEffect, useState } from 'react';

/// Live run state, mirrored from backend `agent-event`s (no polling).
export interface RunStatusData {
  segment: number;
  maxSegments: number;
  turn: number;
  startedAt: number;
  lastActivityAt: number;
  totalTokens: number;
  costUsd: number;
  lastTool: string | null;
  capUsd: number | null;
  finished: boolean;
}

function fmtElapsed(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  const m = Math.floor(s / 60);
  return `${String(m).padStart(2, '0')}:${String(s % 60).padStart(2, '0')}`;
}

function fmtTokens(n: number): string {
  return n >= 1000 ? `${(n / 1000).toFixed(1)}k` : `${n}`;
}

function fmtUsd(n: number): string {
  return `$${n.toFixed(n < 1 ? 3 : 2)}`;
}

export default function RunStatusBar({
  status,
  summary,
}: {
  status: RunStatusData | null;
  summary: string | null;
}) {
  const [, setNow] = useState(Date.now());
  const active = !!status && !status.finished;

  // Tick the elapsed clock (and the "thinking… Ns" readout) once a second
  // while the run is live. Frozen once finished.
  useEffect(() => {
    if (!active) return;
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [active]);

  if (!status) return null;

  const now = Date.now();
  const elapsed = (status.finished ? status.lastActivityAt : now) - status.startedAt;
  const silentSec = Math.floor((now - status.lastActivityAt) / 1000);
  const unbounded = status.capUsd != null;
  const segLabel =
    unbounded && status.segment > status.maxSegments
      ? `Seg ${status.segment}+ · unbounded`
      : `Seg ${status.segment}/${status.maxSegments}`;
  const activity =
    !status.finished && silentSec >= 5
      ? `thinking… ${silentSec}s`
      : status.lastTool
        ? `last: ${status.lastTool}`
        : 'working';

  return (
    <div
      className="mono"
      style={{
        display: 'flex',
        alignItems: 'center',
        gap: '8px',
        flexWrap: 'wrap',
        fontSize: '11px',
        color: 'var(--muted)',
        background: 'var(--panel)',
        border: '1px solid var(--line)',
        borderRadius: '10px',
        padding: '6px 12px',
      }}
      aria-live="polite"
    >
      <span
        style={{
          width: '6px',
          height: '6px',
          borderRadius: '50%',
          flexShrink: 0,
          background: status.finished ? 'var(--ok)' : 'var(--accent)',
          opacity: status.finished ? 1 : 0.85,
        }}
      />
      <span style={{ color: 'var(--text)', fontWeight: 600 }}>{segLabel}</span>
      <span style={{ color: 'var(--faint)' }}>·</span>
      <span>Turn {status.turn}</span>
      <span style={{ color: 'var(--faint)' }}>·</span>
      <span>{fmtElapsed(elapsed)}</span>
      <span style={{ color: 'var(--faint)' }}>·</span>
      <span>
        {fmtTokens(status.totalTokens)} tokens · {fmtUsd(status.costUsd)}
        {status.capUsd != null ? ` / ${fmtUsd(status.capUsd)} cap` : ''}
      </span>
      <span style={{ color: 'var(--faint)', marginLeft: 'auto' }}>{summary ?? activity}</span>
    </div>
  );
}

