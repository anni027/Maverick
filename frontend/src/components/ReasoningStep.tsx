/// One node on the reasoning timeline: an accent dot plus the step text.
/// The live (latest, still-streaming) step renders through the ported
/// `streaming-text-sheen` treatment, which animates a highlight clipped to
/// the glyphs; settled steps render as plain muted text.

interface ReasoningStepProps {
  text: string;
  streaming?: boolean;
}

export default function ReasoningStep({ text, streaming = false }: ReasoningStepProps) {
  return (
    <div className="relative z-[1] flex items-start gap-2">
      <span className="mt-[5px] h-1.5 w-1.5 shrink-0 rounded-full bg-[var(--accent-2)]" />
      {streaming ? (
        <span
          className="streaming-text-sheen mono min-w-0 break-words text-[11.5px] leading-[1.55]"
          data-sheen-text={text}
        >
          {text}
        </span>
      ) : (
        <span className="mono min-w-0 break-words text-[11.5px] leading-[1.55] text-[var(--muted)]">
          {text}
        </span>
      )}
    </div>
  );
}
