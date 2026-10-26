/**
 * Email drafting via `@openuidev/react-email` (44 prebuilt email blocks).
 *
 * Design notes:
 * - The full email prompt is ~32KB — far too big for the main system prompt.
 *   The model gets a compact mini-spec (`EMAIL_DRAFT_PROMPT_SECTION`, ~1KB)
 *   covering the handful of blocks used for drafts; the full catalog stays
 *   out of the context window.
 * - Email blocks live in their own library instance, so a fenced block is
 *   rendered with exactly one library. The taught pattern is TWO fenced
 *   blocks for "draft + send": an `EmailTemplate` block (this library) and a
 *   `Column([ActionButton("Send …", …)])` block (main library). The button's
 *   message becomes a user turn and the agent sends via its `send_email`
 *   tool — one send path, no duplicate MIME logic.
 * - Routing is name-based: any `Email[A-Z]…(` constructor belongs here. The
 *   `Email*` prefix is collision-free with the main library.
 */
import { emailLibrary } from '@openuidev/react-email';
import {
  ActionEvent,
  Library,
  Renderer,
  type ActionEvent as ActionEventT,
} from '@openuidev/react-lang';
import React from 'react';

export { emailLibrary };
export type { ActionEvent };

const EMAIL_NAMES: ReadonlySet<string> = new Set(Object.keys(emailLibrary.components));

const EMAIL_CTOR_RE = /\b(Email[A-Z][A-Za-z]*)\s*\(/;

/** True when the fenced code constructs any email-library component. */
export function isEmailDraftCode(code: string): boolean {
  const m = EMAIL_CTOR_RE.exec(code);
  return m !== null && EMAIL_NAMES.has(m[1]);
}

/** Compact drafting mini-spec appended to the main generative-UI prompt. */
export const EMAIL_DRAFT_PROMPT_SECTION = [
  'EMAIL DRAFTS: when the user asks to draft, write, or design an email, emit the draft as an EmailTemplate fenced block (NOT markdown):',
  'root = EmailTemplate("Subject line", "Preview snippet", [EmailHeading("Title", 1), EmailText("Body paragraph."), EmailButton("Read more", "https://example.com")])',
  'Useful blocks (positional args): EmailHeading(text, level?), EmailText(text), EmailMarkdown(content) for rich formatting, EmailButton(label, href), EmailImage(src, alt, width?), EmailDivider(), EmailLink(text, href), EmailSection([...]) to group.',
  'Always give EmailTemplate a subject and preview text. Keep drafts clean — no lorem ipsum.',
  'To offer sending, emit a SECOND fenced block with the main library: root = Column([ActionButton("Send this email", "Send this email to <address>: <one-line summary>")]). The button posts that message back as a user turn and the send_email tool delivers it.',
].join('\n');

/** Renderer for email-draft blocks (EmailTemplate root). */
export const EmailDraftBlock: React.FC<{
  code: string;
  isStreaming: boolean;
  onAction: (event: ActionEventT) => void;
}> = ({ code, isStreaming, onAction }) => (
  <Renderer
    response={'```openui-lang\n' + code + '\n```'}
    library={emailLibrary as unknown as Library}
    isStreaming={isStreaming}
    onAction={onAction}
    onParseResult={(result) => {
      if (import.meta.env.DEV && result && result.meta.errors.length > 0) {
        // eslint-disable-next-line no-console
        console.debug('[openui-email] parse errors', result.meta.errors);
      }
    }}
  />
);
