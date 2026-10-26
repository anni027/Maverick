/**
 * Maverick generative-UI component library for OpenUI Lang.
 *
 * The agent emits these components inside ```openui-lang fenced blocks
 * (inline mode). `splitOpenUiSegments` carves a chat message into
 * prose segments (rendered by MarkdownRenderer) and UI segments
 * (rendered by OpenUI's <Renderer> with this library).
 *
 * Styling uses the app's CSS variables exclusively, so light/dark
 * theming follows automatically.
 */
import {
  ActionEvent,
  FormNameContext,
  Library,
  Renderer,
  createLibrary,
  defineComponent,
  useFormName,
  useIsStreaming,
  useSetFieldValue,
  useTriggerAction,
} from '@openuidev/react-lang';
import { z } from 'zod';
import React from 'react';
import MarkdownRenderer from '../components/MarkdownRenderer';
import { EMAIL_DRAFT_PROMPT_SECTION } from './email-drafts';

export type { ActionEvent };

/** Artifact-preview handler type shared with MarkdownRenderer. */
export type OpenArtifactFn = (a: { language: string; code: string; title: string }) => void;

/**
 * How prose reaches the `Markdown` component without going through the
 * language: the client renders the constant program `root = Markdown()` and
 * hands the actual text down this context.
 *
 * The model can't be trusted to escape a whole markdown document into a
 * string literal (token cost + corruption risk), and re-parsing a giant
 * escaped literal every stream tick would be slower than the memoized
 * markdown engine we already have. Context side-channels the text instead:
 * the program string never changes, so the parser is a no-op per tick and
 * only the markdown engine re-runs — exactly today's cost.
 */
export interface MarkdownTextValue {
  /** Message prose for this segment (empty for UI segments). */
  text: string;
  /** Streaming flag forwarded to MarkdownRenderer (skips KaTeX per tick). */
  streaming?: boolean;
  /** Open-artifact handler (functions can't cross the wire as props). */
  onOpenArtifact?: OpenArtifactFn;
}

export const MarkdownTextContext = React.createContext<MarkdownTextValue>({ text: '' });

const card: React.CSSProperties = {
  background: 'var(--panel)',
  border: '1px solid var(--line)',
  borderRadius: 'var(--radius)',
  padding: '12px 14px',
};

/* ------------------------------------------------------------------ */
/* Static components                                                   */
/* ------------------------------------------------------------------ */

const Callout = defineComponent({
  name: 'Callout',
  description:
    'A highlighted note box. kind: info (neutral), success (green), warning (amber), error (red). Use for tips, warnings, and important asides.',
  props: z.object({
    kind: z.enum(['info', 'success', 'warning', 'error']).describe('Visual tone of the box'),
    title: z.string().optional().describe('Optional bold heading'),
    text: z.string().describe('Body text (plain, no markdown)'),
  }),
  component: ({ props }) => {
    const accent =
      props.kind === 'success'
        ? 'var(--ok)'
        : props.kind === 'warning'
          ? 'var(--warn)'
          : props.kind === 'error'
            ? 'var(--error)'
            : 'var(--link)';
    return (
      <div style={{ ...card, borderLeft: `3px solid ${accent}` }}>
        {props.title ? (
          <div style={{ fontWeight: 600, marginBottom: 4, color: 'var(--text)' }}>{props.title}</div>
        ) : null}
        <div style={{ color: 'var(--text)', fontSize: 14, whiteSpace: 'pre-wrap' }}>{props.text}</div>
      </div>
    );
  },
});

const Stat = defineComponent({
  name: 'Stat',
  description: 'A single KPI tile: a label with a big value and an optional delta (e.g. "+12%").',
  props: z.object({
    label: z.string().describe('Small caption above the value'),
    value: z.string().describe('The headline value'),
    delta: z.string().optional().describe('Optional change indicator, e.g. "+12% vs last week"'),
  }),
  component: ({ props }) => (
    <div style={{ ...card, minWidth: 140 }}>
      <div style={{ fontSize: 12, color: 'var(--muted)', marginBottom: 2 }}>{props.label}</div>
      <div style={{ fontSize: 22, fontWeight: 650, color: 'var(--text)' }}>{props.value}</div>
      {props.delta ? <div style={{ fontSize: 12, color: 'var(--ok-text)', marginTop: 2 }}>{props.delta}</div> : null}
    </div>
  ),
});

const Table = defineComponent({
  name: 'Table',
  description: 'A data table. columns are headers; each row must have exactly one cell per column.',
  props: z.object({
    columns: z.array(z.string()).describe('Header labels'),
    rows: z.array(z.array(z.string())).describe('Data rows, one string per column'),
  }),
  component: ({ props }) => (
    <div style={{ ...card, padding: 0, overflowX: 'auto' }}>
      <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: 13 }}>
        <thead>
          <tr>
            {props.columns.map((c, i) => (
              <th
                key={i}
                style={{
                  textAlign: 'left',
                  padding: '8px 12px',
                  color: 'var(--muted)',
                  fontWeight: 600,
                  borderBottom: '1px solid var(--line)',
                  whiteSpace: 'nowrap',
                }}
              >
                {c}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {props.rows.map((row, i) => (
            <tr key={i}>
              {props.columns.map((_, j) => (
                <td
                  key={j}
                  style={{
                    padding: '8px 12px',
                    color: 'var(--text)',
                    borderBottom: i === props.rows.length - 1 ? 'none' : '1px solid var(--line)',
                  }}
                >
                  {row[j] ?? ''}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  ),
});

const BarChart = defineComponent({
  name: 'BarChart',
  description: 'A simple vertical bar chart. labels and values must have equal length.',
  props: z.object({
    title: z.string().optional().describe('Optional chart title'),
    labels: z.array(z.string()).describe('One label per bar'),
    values: z.array(z.number()).describe('One numeric value per bar'),
  }),
  component: ({ props }) => {
    const n = Math.max(props.labels.length, props.values.length, 1);
    const max = Math.max(1, ...props.values);
    const W = 560;
    const H = 220;
    const padL = 8;
    const padB = 28;
    const gap = 10;
    const bw = Math.max(8, (W - padL * 2 - gap * (n - 1)) / n);
    return (
      <div style={card}>
        {props.title ? <div style={{ fontWeight: 600, marginBottom: 8, color: 'var(--text)' }}>{props.title}</div> : null}
        <svg viewBox={`0 0 ${W} ${H}`} style={{ width: '100%', display: 'block' }} role="img">
          {Array.from({ length: n }, (_, i) => {
            const v = props.values[i] ?? 0;
            const h = Math.max(2, ((H - padB - 12) * v) / max);
            const x = padL + i * (bw + gap);
            const y = H - padB - h;
            return (
              <g key={i}>
                <rect x={x} y={y} width={bw} height={h} rx={4} fill="var(--accent)" opacity={0.85} />
                <text x={x + bw / 2} y={y - 4} textAnchor="middle" fontSize={11} fill="var(--muted)">
                  {v}
                </text>
                <text
                  x={x + bw / 2}
                  y={H - 10}
                  textAnchor="middle"
                  fontSize={11}
                  fill="var(--muted)"
                  style={{ maxWidth: bw + gap }}
                >
                  {(props.labels[i] ?? '').slice(0, 14)}
                </text>
              </g>
            );
          })}
        </svg>
      </div>
    );
  },
});

const Card = defineComponent({
  name: 'Card',
  description: 'A titled content card for a short summary, bio, or grouped facts.',
  props: z.object({
    title: z.string().describe('Card heading'),
    text: z.string().describe('Body text (plain, no markdown)'),
  }),
  component: ({ props }) => (
    <div style={card}>
      <div style={{ fontWeight: 600, marginBottom: 4, color: 'var(--text)' }}>{props.title}</div>
      <div style={{ fontSize: 14, color: 'var(--text)', whiteSpace: 'pre-wrap' }}>{props.text}</div>
    </div>
  ),
});

/* ------------------------------------------------------------------ */
/* Interactive components                                              */
/* ------------------------------------------------------------------ */

const FieldInput: React.FC<{
  field: { name: string; label: string; type: string; options?: string[]; required?: boolean };
}> = ({ field }) => {
  const formName = useFormName();
  const setFieldValue = useSetFieldValue();
  const push = (value: unknown) => setFieldValue(formName, field.type, field.name, value, false);
  const base: React.CSSProperties = {
    width: '100%',
    boxSizing: 'border-box',
    background: 'var(--surface)',
    border: '1px solid var(--line-2)',
    borderRadius: 8,
    padding: '7px 10px',
    fontSize: 14,
    color: 'var(--text)',
    fontFamily: 'var(--font-body)',
  };
  const input =
    field.type === 'textarea' ? (
      <textarea rows={3} required={field.required} onChange={(e) => push(e.target.value)} style={base} />
    ) : field.type === 'select' ? (
      <select required={field.required} defaultValue="" onChange={(e) => push(e.target.value)} style={base}>
        <option value="" disabled>
          Choose…
        </option>
        {(field.options ?? []).map((o) => (
          <option key={o} value={o}>
            {o}
          </option>
        ))}
      </select>
    ) : (
      <input
        type={field.type === 'number' ? 'number' : 'text'}
        required={field.required}
        onChange={(e) => push(field.type === 'number' ? Number(e.target.value) : e.target.value)}
        style={base}
      />
    );
  return (
    <label style={{ display: 'block', marginBottom: 10 }}>
      <span style={{ display: 'block', fontSize: 12, fontWeight: 600, color: 'var(--muted)', marginBottom: 4 }}>
        {field.label}
        {field.required ? ' *' : ''}
      </span>
      {input}
    </label>
  );
};

const Form = defineComponent({
  name: 'Form',
  description:
    'An interactive form. On submit the field values are sent back to the assistant as a new message. type is one of text, number, select, textarea; options is required when type is select.',
  props: z.object({
    name: z.string().describe('Unique form id, e.g. "contact"'),
    fields: z
      .array(
        z.object({
          name: z.string().describe('Field id, e.g. "email"'),
          label: z.string().describe('Human label shown above the input'),
          type: z.enum(['text', 'number', 'select', 'textarea']).describe('Input kind'),
          options: z.array(z.string()).optional().describe('Choices (required for select)'),
          required: z.boolean().optional().describe('Block submit until filled'),
        }),
      )
      .describe('Fields to render, in order'),
    submitLabel: z.string().optional().describe('Submit button text (default "Submit")'),
  }),
  component: ({ props }) => {
    const streaming = useIsStreaming();
    const triggerAction = useTriggerAction();
    return (
      <FormNameContext.Provider value={props.name}>
        <form
          style={card}
          onSubmit={(e) => {
            e.preventDefault();
            void triggerAction(props.submitLabel ?? `Submit ${props.name}`, props.name);
          }}
        >
          <div style={{ fontWeight: 600, marginBottom: 8, color: 'var(--text)', fontSize: 14 }}>
            {props.name}
          </div>
          {props.fields.map((f) => (
            <FieldInput key={f.name} field={f} />
          ))}
          <button
            type="submit"
            disabled={streaming}
            style={{
              background: 'var(--accent)',
              color: 'var(--accent-on)',
              border: 'none',
              borderRadius: 8,
              padding: '8px 16px',
              fontSize: 14,
              fontWeight: 600,
              cursor: streaming ? 'default' : 'pointer',
              opacity: streaming ? 0.5 : 1,
            }}
          >
            {props.submitLabel ?? 'Submit'}
          </button>
        </form>
      </FormNameContext.Provider>
    );
  },
});

const ActionButton = defineComponent({
  name: 'ActionButton',
  description:
    'A standalone button that sends a follow-up message to the assistant when clicked. Use for approve/deny, "show more", or drill-down actions.',
  props: z.object({
    label: z.string().describe('Button text'),
    message: z.string().describe('The message sent to the assistant on click'),
  }),
  component: ({ props }) => {
    const streaming = useIsStreaming();
    const triggerAction = useTriggerAction();
    return (
      <button
        disabled={streaming}
        onClick={() => void triggerAction(props.message)}
        style={{
          background: 'var(--accent)',
          color: 'var(--accent-on)',
          border: 'none',
          borderRadius: 8,
          padding: '8px 16px',
          fontSize: 14,
          fontWeight: 600,
          cursor: streaming ? 'default' : 'pointer',
          opacity: streaming ? 0.5 : 1,
        }}
      >
        {props.label}
      </button>
    );
  },
});

const Markdown = defineComponent({
  name: 'Markdown',
  description:
    'Rich markdown body text (headings, lists, code blocks, math, links). Use when prose must sit beside other components in a layout; message prose outside the fences already renders as markdown automatically.',
  props: z.object({
    content: z.string().optional().describe('Markdown text. Omit to render the surrounding message prose.'),
  }),
  component: ({ props }) => {
    const ctx = React.useContext(MarkdownTextContext);
    // Inline `Markdown("...")` wins (model-authored inside a fence);
    // otherwise fall back to the segment prose side-channelled via context.
    const text = props.content?.trim() ? props.content : ctx.text;
    if (!text) return null;
    return (
      <MarkdownRenderer content={text} onOpenArtifact={ctx.onOpenArtifact} streaming={ctx.streaming} />
    );
  },
});

const Column = defineComponent({
  name: 'Column',
  description: 'Vertical stack layout. Wrap multiple components in root = Column([...]) so all of them render.',
  props: z.object({
    children: z
      .array(
        z.union([
          Callout.ref,
          Stat.ref,
          Table.ref,
          BarChart.ref,
          Card.ref,
          Form.ref,
          ActionButton.ref,
          Markdown.ref,
        ]),
      )
      .describe('Components stacked top to bottom'),
  }),
  component: ({ props, renderNode }) => (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
      {(props.children as unknown[]).map((child, i) => (
        <React.Fragment key={i}>{renderNode(child)}</React.Fragment>
      ))}
    </div>
  ),
});

export const maverickLibrary: Library = createLibrary({
  components: [Callout, Stat, Table, BarChart, Card, Form, ActionButton, Markdown, Column],
  root: 'Column',
});

/* ------------------------------------------------------------------ */
/* Prompt + segmentation helpers                                       */
/* ------------------------------------------------------------------ */

const GENUI_EXAMPLES = [
  'Show two stats:\n```openui-lang\ns1 = Stat("Users", "12.4k", "+8%")\ns2 = Stat("Revenue", "$48k", "+3%")\nroot = Column([s1, s2])\n```',
  'Ask for an email address:\n```openui-lang\nroot = Column([Form("signup", [{name: "email", label: "Email", type: "text", required: true}], "Sign up")])\n```',
  'Summary text beside a stat:\n```openui-lang\nroot = Column([Markdown("## Weekly summary"), Stat("Revenue", "$48k", "+3%")])\n```',
];

/** System-prompt block teaching the model when and how to emit generative UI. */
export function generateGenUiPrompt(): string {
  return maverickLibrary.prompt({
    preamble:
      'You are a helpful assistant inside a desktop chat app. Your answers render as rich interactive UI inline in the chat.',
    additionalRules: [
      'Use UI components when the user asks for something visual or structured: tables, charts, dashboards, stats, summaries, or input forms.',
      'Ordinary answers — explanations, code, debugging, instructions — stay plain markdown outside the fences. Do NOT wrap normal text in components.',
      'Emit components inside ```openui-lang fenced blocks. Explanatory prose goes outside the fences as normal markdown.',
      'Markdown(text) renders markdown inside a component layout — use it when prose must sit alongside stats or other components.',
      'A single bare statement renders on its own; wrap two or more components in root = Column([...]). The statement named root is what renders.',
      'Form and ActionButton interactions arrive back as new user messages — continue the conversation from them naturally.',
      'Keep props short and factual. Never invent data: use values from the conversation or state clearly that figures are illustrative.',
    ],
    examples: GENUI_EXAMPLES,
    inlineMode: true,
    toolCalls: false,
    bindings: false,
  }) + '\n\n' + EMAIL_DRAFT_PROMPT_SECTION;
}

export interface OpenUiSegment {
  kind: 'text' | 'openui';
  /** For openui segments: the raw code inside the fences. */
  body: string;
  /** False while the closing fence is still missing (streaming). */
  complete: boolean;
}

const FENCE_RE = /```openui(?:-lang)?[^\n]*\n([\s\S]*?)(```|$)/gi;

/** Split a chat message into prose segments and ```openui-lang fenced UI segments. */
export function splitOpenUiSegments(text: string): OpenUiSegment[] {
  const segs: OpenUiSegment[] = [];
  FENCE_RE.lastIndex = 0;
  let last = 0;
  let m: RegExpExecArray | null;
  while ((m = FENCE_RE.exec(text)) !== null) {
    if (m.index > last) segs.push({ kind: 'text', body: text.slice(last, m.index), complete: true });
    segs.push({ kind: 'openui', body: m[1], complete: m[2] === '```' });
    last = m.index + m[0].length;
  }
  if (last < text.length) segs.push({ kind: 'text', body: text.slice(last), complete: true });
  return segs;
}

/** Parse errors: logged once per change, dev only (the free-tier model does
 *  emit malformed markup; the parser is deliberately permissive). */
const logOpenUiParseErrors = (result: { meta: { errors: unknown[] } } | null) => {
  if (import.meta.env.DEV && result && result.meta.errors.length > 0) {
    // eslint-disable-next-line no-console
    console.debug('[openui] parse errors', result.meta.errors);
  }
};

/** Thin wrapper so message code doesn't touch the Renderer directly. */
export const OpenUiBlock: React.FC<{
  code: string;
  isStreaming: boolean;
  onAction: (event: ActionEvent) => void;
  library?: Library;
}> = ({ code, isStreaming, onAction, library = maverickLibrary }) => (
  <Renderer
    response={'```openui-lang\n' + code + '\n```'}
    library={library}
    isStreaming={isStreaming}
    onAction={onAction}
    onParseResult={logOpenUiParseErrors}
  />
);

/** The whole prose rendering path: one constant program, never re-parsed
 *  (the response string is identical every tick) with the message text
 *  flowing through {@link MarkdownTextContext} instead of a string literal. */
const PROSE_RESPONSE = '```openui-lang\nroot = Markdown()\n```';

/** Render message prose through the OpenUI pipeline (Markdown component).
 *  This is the default path — plain `MarkdownRenderer` is only reachable
 *  with the generative-UI switch turned off. */
export const MarkdownProseBlock: React.FC<{
  text: string;
  isStreaming?: boolean;
  onOpenArtifact?: OpenArtifactFn;
}> = ({ text, isStreaming, onOpenArtifact }) => {
  const value = React.useMemo<MarkdownTextValue>(
    () => ({ text, streaming: isStreaming, onOpenArtifact }),
    [text, isStreaming, onOpenArtifact],
  );
  return (
    <MarkdownTextContext.Provider value={value}>
      <Renderer
        response={PROSE_RESPONSE}
        library={maverickLibrary}
        isStreaming={isStreaming}
        onParseResult={logOpenUiParseErrors}
      />
    </MarkdownTextContext.Provider>
  );
};
