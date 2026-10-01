import ReasoningStep from './ReasoningStep';
import { ChevronIcon } from './icons';

/// Collapsible reasoning trace for one assistant turn. Backed by the
/// `ThinkingStarted` / `ThinkingStep` agent events: `open` tracks whether the
/// turn is still reasoning, so the shell stays expanded live and the browser
/// owns the collapse gesture afterwards (native `<details>`).

interface ThinkingTimelineProps {
  steps: string[];
  open: boolean;
}

export default function ThinkingTimeline({ steps, open }: ThinkingTimelineProps) {
  if (steps.length === 0) return null;
  const lastIdx = steps.length - 1;

  return (
    <details
      open={open}
      className="fold mb-[10px] overflow-hidden rounded-[14px] border border-[var(--line)] bg-[var(--panel)]"
    >
      <summary className="flex cursor-pointer list-none items-center gap-[9px] px-3 py-[9px] text-[12px] text-[var(--muted)]">
        <span className="relative inline-flex h-2 w-2 shrink-0 items-center justify-center">
          <span className="ping-soft absolute h-2 w-2 rounded-full bg-[var(--accent)] opacity-75" />
          <span className="h-2 w-2 rounded-full bg-[var(--accent)]" />
        </span>
        <span className="mono font-semibold text-[var(--text)]">
          Thought for {steps.length} step{steps.length === 1 ? '' : 's'}
        </span>
        <span className="mono ml-auto text-[10px] text-[var(--faint)]">
          {open ? 'thinking…' : 'done'}
        </span>
        <ChevronIcon size={14} className="fold-chev" />
      </summary>

      <div className="relative flex flex-col gap-2 border-t border-[var(--line)] px-3 pb-3 pt-[10px]">
        <div className="absolute bottom-[14px] left-[21px] top-[14px] w-px bg-[var(--line)]" />
        {steps.map((s, i) => (
          <ReasoningStep key={i} text={s} streaming={open && i === lastIdx} />
        ))}
      </div>
    </details>
  );
}
