import { useState } from 'react';
import type { PendingBatch, PendingQuestion } from '../types';

interface QuestionCardProps {
  batch: PendingBatch;
  /** Submit in flight — disables the card. */
  answering: boolean;
  /**
   * Answer the whole batch; entries align with the batch's questions and
   * empty strings are per-question skips. All-empty = skip all.
   */
  onAnswer: (id: string, answers: string[]) => void;
}

interface StepProps {
  question: PendingQuestion;
  /** Previously recorded answer (restored when stepping back). */
  initial: string | null;
  disabled: boolean;
  onChange: (answer: string | null) => void;
  onAdvance: () => void;
}

/** One step of the batch: radios, optional free text. Untouched = skipped. */
function QuestionStep({ question, initial, disabled, onChange, onAdvance }: StepProps) {
  const optMatch =
    initial !== null && question.options.some(o => o.label === initial);
  const [picked, setPicked] = useState<string | null>(() =>
    optMatch ? initial : null,
  );
  const [custom, setCustom] = useState(() =>
    initial !== null && !optMatch ? initial : '',
  );
  const [useCustom, setUseCustom] = useState(() => initial !== null && !optMatch);

  const commitPick = (label: string) => {
    setPicked(label);
    setUseCustom(false);
    onChange(label);
  };
  const commitCustom = (text: string) => {
    setCustom(text);
    setUseCustom(true);
    onChange(text.trim() || null);
  };

  return (
    <>
      <div
        style={{ display: 'flex', flexDirection: 'column', gap: '2px', maxHeight: '260px', overflowY: 'auto' }}
        role="radiogroup"
      >
        {question.options.map(o => {
          const active = !useCustom && picked === o.label;
          return (
            <button
              key={o.label}
              role="radio"
              aria-checked={active}
              disabled={disabled}
              onClick={() => commitPick(o.label)}
              style={{
                textAlign: 'left',
                display: 'flex',
                gap: '10px',
                alignItems: 'flex-start',
                padding: '7px 10px',
                borderRadius: '10px',
                border: 'none',
                background: active ? 'var(--control-hover)' : 'transparent',
                cursor: disabled ? 'default' : 'pointer',
              }}
            >
              <span
                style={{
                  marginTop: '2px',
                  width: '13px',
                  height: '13px',
                  borderRadius: '50%',
                  border: '1px solid var(--muted)',
                  background: active ? 'var(--text)' : 'transparent',
                  boxShadow: active ? 'inset 0 0 0 2.5px var(--panel)' : 'none',
                  flexShrink: 0,
                }}
              />
              <span style={{ minWidth: 0 }}>
                <div style={{ fontSize: '12.5px', fontWeight: active ? 600 : 400 }}>{o.label}</div>
                {o.description && (
                  <div className="mono" style={{ fontSize: '11px', color: 'var(--muted)', marginTop: '1px' }}>
                    {o.description}
                  </div>
                )}
              </span>
            </button>
          );
        })}
      </div>

      {question.allow_custom && (
        <input
          value={custom}
          disabled={disabled}
          onChange={e => commitCustom(e.target.value)}
          onFocus={() => {
            if (!useCustom) {
              setUseCustom(true);
              onChange(custom.trim() || null);
            }
          }}
          onKeyDown={e => {
            if (e.key === 'Enter') onAdvance();
          }}
          placeholder="Something else…"
          style={{ width: '100%', fontSize: '12.5px' }}
        />
      )}
    </>
  );
}

/**
 * Batched clarifying-question card above the prompt bar: steps through the
 * batch Claude-style ("1 of N"), preserves per-question answers while
 * stepping, skips unanswered ones. A batch of one renders as a plain card.
 */
export default function QuestionCard({ batch, answering, onAnswer }: QuestionCardProps) {
  const total = batch.questions.length;
  const [step, setStep] = useState(0);
  const [answers, setAnswers] = useState<(string | null)[]>(() =>
    batch.questions.map(() => null),
  );
  const current = batch.questions[step];
  const last = step === total - 1;
  const answeredCount = answers.filter(a => a !== null).length;

  const record = (index: number, answer: string | null) => {
    setAnswers(prev => {
      if (prev[index] === answer) return prev;
      const next = [...prev];
      next[index] = answer;
      return next;
    });
  };
  const submitAll = (skipAll: boolean) => {
    if (answering) return;
    onAnswer(
      batch.id,
      batch.questions.map((_, i) => (skipAll ? '' : (answers[i] ?? ''))),
    );
  };
  const skipAll = () => submitAll(true);

  return (
    <div
      onKeyDown={e => {
        if (e.key === 'Escape') skipAll();
      }}
      style={{
        width: '100%',
        border: '1px solid var(--line-2, var(--line))',
        borderRadius: '16px',
        background: 'var(--panel)',
        boxShadow: '0 12px 32px rgba(0,0,0,0.35)',
        padding: '14px 16px 10px',
        display: 'flex',
        flexDirection: 'column',
        gap: '10px',
      }}
    >
      <div style={{ display: 'flex', alignItems: 'flex-start', gap: '10px' }}>
        <div style={{ fontSize: '14px', fontWeight: 600, lineHeight: 1.4, flex: 1, minWidth: 0 }}>
          {current.question}
        </div>
        {total > 1 && (
          <div className="mono" style={{ fontSize: '11px', color: 'var(--faint)', whiteSpace: 'nowrap', paddingTop: '2px' }}>
            {step + 1} of {total}
          </div>
        )}
        <button
          className="btn-ghost btn-ico"
          onClick={skipAll}
          disabled={answering}
          aria-label="Skip all questions"
          title="Skip all (Esc)"
          style={{ borderRadius: '999px', flexShrink: 0 }}
        >
          <svg width="11" height="11" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.5"><path d="M2 2 L10 10 M10 2 L2 10" /></svg>
        </button>
      </div>

      <QuestionStep
        key={`${batch.id}-${step}`}
        question={current}
        initial={answers[step]}
        disabled={answering}
        onChange={a => record(step, a)}
        onAdvance={() => {
          if (!answering && !last) setStep(step + 1);
          else if (!answering && last) submitAll(false);
        }}
      />

      <div
        style={{
          display: 'flex',
          gap: '8px',
          alignItems: 'center',
          borderTop: '1px solid var(--line)',
          paddingTop: '10px',
        }}
      >
        {total > 1 && (
          <button
            className="btn-ghost"
            disabled={answering || step === 0}
            onClick={() => setStep(s => Math.max(0, s - 1))}
            style={{ borderRadius: 999 }}
          >
            Back
          </button>
        )}
        <span className="mono" style={{ fontSize: '10.5px', color: 'var(--faint)', flex: 1 }}>
          {total > 1 ? `${answeredCount} of ${total} answered` : (answers[0] ? `selected: ${answers[0]}` : 'nothing selected')}
        </span>
        <button
          className="btn-ghost"
          disabled={answering}
          onClick={skipAll}
          style={{ borderRadius: 999 }}
          title="Let the agent proceed with its best judgment"
        >
          Skip all
        </button>
        {last ? (
          <button
            className="btn-accent"
            disabled={answering}
            onClick={() => submitAll(false)}
            style={{ borderRadius: 999 }}
          >
            {answering ? 'Sending…' : total > 1 ? 'Send answers' : 'Send answer'}
          </button>
        ) : (
          <button
            className="btn-accent"
            disabled={answering}
            onClick={() => setStep(step + 1)}
            style={{ borderRadius: 999 }}
          >
            Next
          </button>
        )}
      </div>
      <div className="mono" style={{ fontSize: '10px', color: 'var(--faint)', textAlign: 'center' }}>
        Enter to continue · Esc to skip all
      </div>
    </div>
  );
}
