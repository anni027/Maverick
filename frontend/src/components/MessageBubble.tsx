import { useState } from 'react';
import { Message } from '../types';
import MarkdownRenderer from './MarkdownRenderer';
import ThinkingTimeline from './ThinkingTimeline';
import { ApertureTile } from './ApertureLogo';
import { ChevronIcon, CopyIcon, CheckIcon } from './icons';

/// One transcript row. Purely presentational: it renders whatever `Message`
/// it is handed and reports copy actions to itself only. Run/ownership state
/// stays in Chat.tsx — `isStreaming` is passed down, never derived here.

interface MessageBubbleProps {
  message: Message;
  compact?: boolean;
  /** True for the single live assistant message while a run is in flight. */
  isStreaming?: boolean;
}

export default function MessageBubble({ message, compact = false, isStreaming = false }: MessageBubbleProps) {
  const [copied, setCopied] = useState(false);
  const isUser = message.role === 'user';
  const isTool = message.role === 'tool';
  const isError = message.content.startsWith('Error:') || message.content.startsWith('Send failed:');

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(message.content);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {}
  };

  if (isTool) {
    const hasResult = !!message.toolResult;
    return (
      <details
        open={!hasResult}
        className={`fold overflow-hidden rounded-[14px] border border-[var(--line)] bg-[var(--panel)] ${compact ? 'my-1' : 'my-[10px]'}`}
      >
        <summary className="flex cursor-pointer list-none items-center gap-[9px] px-3 py-[9px] text-[12px] text-[var(--muted)]">
          <span className="relative inline-flex h-2 w-2 shrink-0 items-center justify-center">
            {hasResult ? (
              <span className="h-2 w-2 rounded-full bg-[var(--accent-2)]" />
            ) : (
              <>
                <span className="ping-soft absolute h-2 w-2 rounded-full bg-[var(--accent)] opacity-75" />
                <span className="h-2 w-2 rounded-full bg-[var(--accent)]" />
              </>
            )}
          </span>
          <span className={`mono font-semibold ${hasResult ? 'text-[var(--text)]' : 'text-[var(--muted)]'}`}>
            {message.content}
          </span>
          <span
            className={`mono rounded-full border px-[7px] py-px text-[10px] ${
              hasResult ? 'border-[var(--accent-border)] text-[var(--accent-2)]' : 'border-[var(--line)] text-[var(--muted)]'
            }`}
          >
            {hasResult ? 'completed' : 'running…'}
          </span>
          {hasResult && message.durationMs != null && (
            <span className="mono text-[10px] text-[var(--faint)]">· {(message.durationMs / 1000).toFixed(1)}s</span>
          )}
          <span className="flex-1" />
          <ChevronIcon size={14} className="fold-chev" />
        </summary>
        <div className="border-t border-[var(--line)] px-3 pb-3 pt-[10px]">
          {message.toolCalls && (
            <div className="mono break-all rounded-[10px] border border-[var(--line)] bg-[var(--panel-2)] px-[10px] py-2 text-[11px] text-[var(--faint)]">
              {message.toolCalls[0]?.arguments.slice(0, 600)}
            </div>
          )}
          {message.toolResult && (
            <div className="mono mt-2 max-h-[220px] overflow-y-auto whitespace-pre-wrap rounded-[12px] border border-[var(--line)] bg-[var(--panel-2)] px-3 py-[10px] text-[12px] leading-[1.5]">
              {message.toolResult.content.slice(0, 2000)}
            </div>
          )}
        </div>
      </details>
    );
  }

  if (isUser) {
    return (
      <div className={`flex justify-end ${compact ? 'py-1' : 'py-[10px]'}`}>
        <div className="max-w-[80%] rounded-[18px] border border-[var(--line-2)] bg-[var(--bubble-user)] px-4 py-[10px] text-[var(--text)]">
          <div className="whitespace-pre-wrap break-words text-[14.5px] leading-[1.6]">{message.content}</div>
        </div>
      </div>
    );
  }

  return (
    <div className={`flex items-start gap-[14px] ${compact ? 'py-[6px]' : 'py-[14px]'}`}>
      <ApertureTile size={28} />
      <div className="min-w-0 flex-1 pt-px">
        {message.thinking && <ThinkingTimeline steps={message.thinking.steps} open={message.thinking.open} />}

        {isError ? (
          <div className="whitespace-pre-wrap rounded-[14px] border border-[var(--error-border)] bg-[var(--error-bg)] px-[14px] py-3 text-[var(--error)]">
            {message.content}
          </div>
        ) : (
          <div className={isStreaming ? 'markdown-streaming' : undefined}>
            <MarkdownRenderer content={message.content} />
          </div>
        )}

        <div className="mono mt-2 flex items-center gap-3 text-[11px] text-[var(--faint)]">
          <span>{message.timestamp.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}</span>
          {!isError && (
            <button
              onClick={handleCopy}
              className="btn-ghost inline-flex items-center gap-1 rounded-[4px] px-[7px] py-[2px] text-[11px]"
              style={{ color: copied ? 'var(--accent-2)' : 'var(--muted)' }}
              aria-label="Copy message"
            >
              {copied ? <CheckIcon size={11} /> : <CopyIcon size={11} />}
              <span>{copied ? 'Copied' : 'Copy'}</span>
            </button>
          )}
          {isError && <span className="text-[var(--error)]">• needs attention</span>}
        </div>
      </div>
    </div>
  );
}
