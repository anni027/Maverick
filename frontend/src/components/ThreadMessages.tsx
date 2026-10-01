import MessageBubble from './MessageBubble';
import type { Message } from '../types';

/// Message list for the thread. Each row is wrapped in `.thread-render-unit`
/// so offscreen rows skip layout and paint (see the `content-visibility` rule
/// in index.css). Keys come from the stable `Message.id`, which is what keeps
/// that skipping from remounting rows while a run streams.

interface ThreadMessagesProps {
  messages: Message[];
  compact?: boolean;
  /** id of the live assistant message, or null when no run is in flight. */
  streamingId?: string | null;
}

export default function ThreadMessages({ messages, compact = false, streamingId = null }: ThreadMessagesProps) {
  return (
    <div className="flex flex-col pb-6">
      {messages.map(msg => (
        <div key={msg.id} className="thread-render-unit">
          <MessageBubble message={msg} compact={compact} isStreaming={msg.id === streamingId} />
        </div>
      ))}
    </div>
  );
}
