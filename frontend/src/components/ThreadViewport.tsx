import { forwardRef, type ReactNode } from 'react';

/// Scroll container for the thread. `.thread-viewport` opens a named size
/// container so descendants can adapt on width without viewport media queries.
/// The scroll ref is forwarded rather than owned here — auto-scroll decisions
/// stay with Chat.tsx, which is what decides when to follow the transcript.

interface ThreadViewportProps {
  children: ReactNode;
  className?: string;
}

const ThreadViewport = forwardRef<HTMLDivElement, ThreadViewportProps>(function ThreadViewport(
  { children, className = '' },
  ref,
) {
  return (
    <div
      ref={ref}
      className={`thread-viewport flex-1 overflow-y-auto overflow-x-hidden ${className}`.trim()}
    >
      {children}
    </div>
  );
});

export default ThreadViewport;
