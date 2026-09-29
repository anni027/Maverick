import React, { useState } from 'react';

declare global {
  interface Window {
    katex?: {
      renderToString: (tex: string, options?: { displayMode?: boolean; throwOnError?: boolean }) => string;
    };
  }
}

interface MarkdownRendererProps {
  content: string;
}

export default function MarkdownRenderer({ content }: MarkdownRendererProps) {
  if (!content) return null;
  const blocks = parseBlocks(content);

  return (
    <div className="markdown-body" style={{ lineHeight: 1.7, fontSize: '14.5px', color: 'var(--text)' }}>
      {blocks.map((block, idx) => (
        <BlockView key={idx} block={block} />
      ))}
    </div>
  );
}

// -----------------------------------------------------------------------------
// Block Types & Parser
// -----------------------------------------------------------------------------

type Block =
  | { type: 'heading'; level: number; text: string }
  | { type: 'code_block'; language: string; code: string }
  | { type: 'math_block'; tex: string }
  | { type: 'table'; headers: string[]; rows: string[][] }
  | { type: 'blockquote'; text: string }
  | { type: 'unordered_list'; items: string[] }
  | { type: 'ordered_list'; items: string[] }
  | { type: 'hr' }
  | { type: 'paragraph'; text: string };

function parseBlocks(raw: string): Block[] {
  const lines = raw.replace(/\r\n/g, '\n').split('\n');
  const blocks: Block[] = [];
  let i = 0;

  while (i < lines.length) {
    const line = lines[i];

    // 1. Math Block ($$...$$ or \[...\])
    if (line.trim().startsWith('$$') || line.trim().startsWith('\\[')) {
      const isBracket = line.trim().startsWith('\\[');
      const endMarker = isBracket ? '\\]' : '$$';
      const firstLine = line.trim().slice(2);
      if (firstLine.endsWith(endMarker) && firstLine.length > 2) {
        blocks.push({ type: 'math_block', tex: firstLine.slice(0, -endMarker.length).trim() });
        i++;
        continue;
      }
      let mathContent = firstLine ? [firstLine] : [];
      i++;
      while (i < lines.length && !lines[i].trim().endsWith(endMarker) && !lines[i].trim().startsWith(endMarker)) {
        mathContent.push(lines[i]);
        i++;
      }
      if (i < lines.length) {
        const lastLine = lines[i].trim().replace(new RegExp(`${endMarker.replace(/\\/g, '\\\\')}$`), '').trim();
        if (lastLine) mathContent.push(lastLine);
        i++;
      }
      blocks.push({ type: 'math_block', tex: mathContent.join('\n').trim() });
      continue;
    }

    // 2. Code Block (```lang ... ```)
    if (line.trim().startsWith('```')) {
      const language = line.trim().slice(3).trim();
      const codeLines: string[] = [];
      i++;
      while (i < lines.length && !lines[i].trim().startsWith('```')) {
        codeLines.push(lines[i]);
        i++;
      }
      if (i < lines.length) i++; // consume closing ```
      blocks.push({ type: 'code_block', language, code: codeLines.join('\n') });
      continue;
    }

    // 3. Headings (# H1, ## H2, etc.)
    const headingMatch = line.match(/^(#{1,6})\s+(.*)$/);
    if (headingMatch) {
      blocks.push({
        type: 'heading',
        level: headingMatch[1].length,
        text: headingMatch[2],
      });
      i++;
      continue;
    }

    // 4. Horizontal Rule (---, ***, ___)
    if (/^(\*{3,}|-{3,}|_{3,})$/.test(line.trim())) {
      blocks.push({ type: 'hr' });
      i++;
      continue;
    }

    // 5. Blockquote (> quote)
    if (line.startsWith('>')) {
      const quoteLines: string[] = [];
      while (i < lines.length && lines[i].startsWith('>')) {
        quoteLines.push(lines[i].replace(/^>\s?/, ''));
        i++;
      }
      blocks.push({ type: 'blockquote', text: quoteLines.join('\n') });
      continue;
    }

    // 6. Tables (| col 1 | col 2 |)
    if (line.includes('|') && i + 1 < lines.length && /^\s*\|?\s*[-:]+[-| :]*\s*\|?\s*$/.test(lines[i + 1])) {
      const parseRow = (r: string) =>
        r
          .trim()
          .replace(/^\|/, '')
          .replace(/\|$/, '')
          .split('|')
          .map(c => c.trim());

      const headers = parseRow(line);
      i += 2; // skip header and separator
      const rows: string[][] = [];
      while (i < lines.length && lines[i].includes('|') && lines[i].trim() !== '') {
        rows.push(parseRow(lines[i]));
        i++;
      }
      blocks.push({ type: 'table', headers, rows });
      continue;
    }

    // 7. Unordered List (- item or * item)
    if (/^\s*[-*+]\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*[-*+]\s+/.test(lines[i])) {
        items.push(lines[i].replace(/^\s*[-*+]\s+/, ''));
        i++;
      }
      blocks.push({ type: 'unordered_list', items });
      continue;
    }

    // 8. Ordered List (1. item)
    if (/^\s*\d+\.\s+/.test(line)) {
      const items: string[] = [];
      while (i < lines.length && /^\s*\d+\.\s+/.test(lines[i])) {
        items.push(lines[i].replace(/^\s*\d+\.\s+/, ''));
        i++;
      }
      blocks.push({ type: 'ordered_list', items });
      continue;
    }

    // 9. Blank line
    if (line.trim() === '') {
      i++;
      continue;
    }

    // 10. Paragraph (collect consecutive lines until empty or other block)
    const pLines: string[] = [];
    while (
      i < lines.length &&
      lines[i].trim() !== '' &&
      !lines[i].trim().startsWith('```') &&
      !lines[i].trim().startsWith('$$') &&
      !lines[i].trim().startsWith('\\[') &&
      !lines[i].match(/^#{1,6}\s+/) &&
      !/^\s*[-*+]\s+/.test(lines[i]) &&
      !/^\s*\d+\.\s+/.test(lines[i]) &&
      !lines[i].startsWith('>') &&
      !(lines[i].includes('|') && i + 1 < lines.length && /^\s*\|?\s*[-:]+[-| :]*\s*\|?\s*$/.test(lines[i + 1]))
    ) {
      pLines.push(lines[i]);
      i++;
    }
    if (pLines.length > 0) {
      blocks.push({ type: 'paragraph', text: pLines.join(' ') });
    }
  }

  return blocks;
}

// -----------------------------------------------------------------------------
// Component Renderers
// -----------------------------------------------------------------------------

function BlockView({ block }: { block: Block }) {
  switch (block.type) {
    case 'heading': {
      const Tag = (`h${Math.min(block.level, 6)}` as keyof JSX.IntrinsicElements) || 'h4';
      const fontSizes = ['24px', '20px', '17px', '15px', '14px', '13px'];
      const margins = ['20px 0 10px', '18px 0 8px', '14px 0 6px', '12px 0 4px', '10px 0 4px', '8px 0 4px'];
      return (
        <Tag
          style={{
            fontSize: fontSizes[block.level - 1] || '15px',
            fontWeight: block.level <= 2 ? 700 : 650,
            letterSpacing: block.level <= 2 ? '-0.025em' : '-0.015em',
            margin: margins[block.level - 1] || '12px 0 6px',
            lineHeight: 1.3,
            color: 'var(--text)',
          }}
        >
          <InlineContent text={block.text} />
        </Tag>
      );
    }

    case 'code_block':
      return <CodeBlock language={block.language} code={block.code} />;

    case 'math_block':
      return <MathBlock tex={block.tex} />;

    case 'blockquote':
      return (
        <blockquote
          style={{
            margin: '12px 0',
            padding: '10px 16px',
            borderLeft: '3px solid var(--accent)',
            background: 'transparent',
            borderRadius: '0 8px 8px 0',
            color: 'var(--text)',
            opacity: 0.95,
          }}
        >
          <InlineContent text={block.text} />
        </blockquote>
      );

    case 'table':
      return (
        <div style={{ margin: '14px 0', overflowX: 'auto', borderRadius: '10px', border: '1px solid var(--line)' }}>
          <table style={{ width: '100%', borderCollapse: 'collapse', fontSize: '13.5px', textAlign: 'left' }}>
            <thead>
              <tr style={{ background: 'transparent', borderBottom: '1px solid var(--line)' }}>
                {block.headers.map((h, i) => (
                  <th key={i} style={{ padding: '10px 14px', fontWeight: 650, color: 'var(--text)' }}>
                    <InlineContent text={h} />
                  </th>
                ))}
              </tr>
            </thead>
            <tbody>
              {block.rows.map((row, ri) => (
                <tr
                  key={ri}
                  style={{
                    borderBottom: ri === block.rows.length - 1 ? 'none' : '1px solid var(--line-2)',
                    background: 'transparent',
                  }}
                >
                  {row.map((cell, ci) => (
                    <td key={ci} style={{ padding: '9px 14px', color: 'var(--text)' }}>
                      <InlineContent text={cell} />
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      );

    case 'unordered_list':
      return (
        <ul style={{ margin: '8px 0', paddingLeft: '22px', display: 'flex', flexDirection: 'column', gap: '4px' }}>
          {block.items.map((item, i) => (
            <li key={i} style={{ listStyleType: 'disc' }}>
              <InlineContent text={item} />
            </li>
          ))}
        </ul>
      );

    case 'ordered_list':
      return (
        <ol style={{ margin: '8px 0', paddingLeft: '22px', display: 'flex', flexDirection: 'column', gap: '4px' }}>
          {block.items.map((item, i) => (
            <li key={i}>
              <InlineContent text={item} />
            </li>
          ))}
        </ol>
      );

    case 'hr':
      return <hr style={{ border: 'none', borderTop: '1px solid var(--line)', margin: '18px 0' }} />;

    case 'paragraph':
      return (
        <p style={{ margin: '8px 0', lineHeight: 1.7, color: 'var(--text)' }}>
          <InlineContent text={block.text} />
        </p>
      );

    default:
      return null;
  }
}

// -----------------------------------------------------------------------------
// Code Block with Copy Button
// -----------------------------------------------------------------------------

function CodeBlock({ language, code }: { language: string; code: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = async () => {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      // ignore
    }
  };

  return (
    <div
      style={{
        margin: '14px 0',
        borderRadius: '12px',
        overflow: 'hidden',
        border: '1px solid var(--line)',
        background: 'var(--code-surface)',
      }}
    >
      {/* Top bar */}
      <div
        style={{
          display: 'flex',
          alignItems: 'center',
          justifyContent: 'space-between',
          padding: '8px 14px',
          background: 'var(--code-head)',
          borderBottom: '1px solid var(--line)',
        }}
      >
        <span className="mono" style={{ fontSize: '11px', color: 'var(--muted)', textTransform: 'lowercase' }}>
          {language || 'code'}
        </span>
        <button
          onClick={handleCopy}
          className="btn-ghost"
          style={{
            padding: '3px 8px',
            fontSize: '11px',
            borderRadius: '6px',
            display: 'inline-flex',
            alignItems: 'center',
            gap: '5px',
            color: copied ? 'var(--ok)' : 'var(--muted)',
            cursor: 'pointer',
          }}
          aria-label="Copy code"
        >
          {copied ? (
            <>
              <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.6">
                <path d="M2.5 6.5 L4.5 8.5 L9.5 3.5" />
              </svg>
              <span>Copied!</span>
            </>
          ) : (
            <>
              <svg width="12" height="12" viewBox="0 0 12 12" fill="none" stroke="currentColor" strokeWidth="1.3">
                <rect x="4" y="4" width="6" height="6" rx="1" />
                <path d="M3 8 H2.5 A1 1 0 0 1 1.5 7 V2.5 A1 1 0 0 1 2.5 1.5 H7 A1 1 0 0 1 8 2.5 V3" />
              </svg>
              <span>Copy code</span>
            </>
          )}
        </button>
      </div>

      {/* Code contents */}
      <pre
        className="mono"
        style={{
          margin: 0,
          padding: '14px 16px',
          fontSize: '13px',
          lineHeight: 1.55,
          overflowX: 'auto',
          color: 'var(--code-fg)',
          background: 'var(--code-body)',
          fontFamily: 'JetBrains Mono, Menlo, Consolas, monospace',
        }}
      >
        <code>{code}</code>
      </pre>
    </div>
  );
}

// -----------------------------------------------------------------------------
// Math Block ($$...$$)
// -----------------------------------------------------------------------------

function MathBlock({ tex }: { tex: string }) {
  if (typeof window !== 'undefined' && window.katex) {
    try {
      const html = window.katex.renderToString(tex, { displayMode: true, throwOnError: false });
      return (
        <div
          style={{ margin: '14px 0', padding: '12px 16px', overflowX: 'auto', textAlign: 'center' }}
          dangerouslySetInnerHTML={{ __html: html }}
        />
      );
    } catch {
      // fallback below
    }
  }

  // Fallback math display
  return (
    <div
      style={{
        margin: '14px 0',
        padding: '14px 20px',
        background: 'transparent',
        border: '1px solid var(--line)',
        borderRadius: '10px',
        textAlign: 'center',
        fontFamily: 'KaTeX_Main, Cambria Math, Times New Roman, serif',
        fontSize: '16px',
        letterSpacing: '0.04em',
        color: 'var(--code-fg-strong)',
        overflowX: 'auto',
      }}
    >
      {formatFallbackMath(tex)}
    </div>
  );
}

// -----------------------------------------------------------------------------
// Inline Markdown & Math Parser ($...$, `code`, **bold**, *italic*, [link](url))
// -----------------------------------------------------------------------------

function InlineContent({ text }: { text: string }) {
  if (!text) return null;
  const elements = parseInline(text);
  return <>{elements}</>;
}

function parseInline(text: string): React.ReactNode[] {
  const parts: React.ReactNode[] = [];
  // Tokenize regex matching inline code, inline math, links, bold, italic, strikethrough
  const pattern = /(`[^`]+`|\$[^$\n]+\$|!*\[[^\]]+\]\([^)]+\)|\*\*[^*]+\*\*|__[^_]+__|\*[^*]+\*|_[^_]+_|~~[^~]+~~)/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;

  while ((match = pattern.exec(text)) !== null) {
    if (match.index > lastIndex) {
      parts.push(text.substring(lastIndex, match.index));
    }
    const token = match[0];
    const key = `inline-${match.index}`;

    if (token.startsWith('`') && token.endsWith('`')) {
      // Inline code
      parts.push(
        <code
          key={key}
          className="mono"
          style={{
            background: 'transparent',
            border: '1px solid var(--line)',
            padding: '2px 5px',
            borderRadius: '5px',
            fontSize: '12.5px',
            color: 'var(--inline-code-fg)',
          }}
        >
          {token.slice(1, -1)}
        </code>
      );
    } else if (token.startsWith('$') && token.endsWith('$') && token.length > 2) {
      // Inline math
      const mathTex = token.slice(1, -1);
      if (typeof window !== 'undefined' && window.katex) {
        try {
          const html = window.katex.renderToString(mathTex, { displayMode: false, throwOnError: false });
          parts.push(<span key={key} dangerouslySetInnerHTML={{ __html: html }} />);
        } catch {
          parts.push(
            <span
              key={key}
              style={{
                fontFamily: 'KaTeX_Main, Cambria Math, Times New Roman, serif',
                fontStyle: 'italic',
                padding: '0 3px',
                color: 'var(--code-fg-strong)',
              }}
            >
              {formatFallbackMath(mathTex)}
            </span>
          );
        }
      } else {
        parts.push(
          <span
            key={key}
            style={{
              fontFamily: 'KaTeX_Main, Cambria Math, Times New Roman, serif',
              fontStyle: 'italic',
              padding: '0 3px',
              color: 'var(--code-fg-strong)',
            }}
          >
            {formatFallbackMath(mathTex)}
          </span>
        );
      }
    } else if (token.startsWith('**') && token.endsWith('**')) {
      // Bold
      parts.push(<strong key={key} style={{ fontWeight: 650, color: 'var(--text)' }}>{token.slice(2, -2)}</strong>);
    } else if (token.startsWith('__') && token.endsWith('__')) {
      // Bold
      parts.push(<strong key={key} style={{ fontWeight: 650, color: 'var(--text)' }}>{token.slice(2, -2)}</strong>);
    } else if (token.startsWith('~~') && token.endsWith('~~')) {
      // Strikethrough
      parts.push(<del key={key} style={{ opacity: 0.7 }}>{token.slice(2, -2)}</del>);
    } else if ((token.startsWith('*') && token.endsWith('*')) || (token.startsWith('_') && token.endsWith('_'))) {
      // Italic
      parts.push(<em key={key} style={{ fontStyle: 'italic' }}>{token.slice(1, -1)}</em>);
    } else if (token.startsWith('[') && token.includes('](')) {
      // Link
      const linkMatch = token.match(/\[([^\]]+)\]\(([^)]+)\)/);
      if (linkMatch) {
        parts.push(
          <a
            key={key}
            href={linkMatch[2]}
            target="_blank"
            rel="noopener noreferrer"
            style={{ color: 'var(--link)', textDecoration: 'underline', textUnderlineOffset: '3px' }}
          >
            {linkMatch[1]}
          </a>
        );
      } else {
        parts.push(token);
      }
    } else {
      parts.push(token);
    }
    lastIndex = pattern.lastIndex;
  }

  if (lastIndex < text.length) {
    parts.push(text.substring(lastIndex));
  }

  return parts;
}

// -----------------------------------------------------------------------------
// Fallback Math Formatter (converts common LaTeX commands to symbols)
// -----------------------------------------------------------------------------

function formatFallbackMath(tex: string): string {
  let s = tex;
  const replacements: [RegExp, string][] = [
    [/\\alpha/g, 'α'],
    [/\\beta/g, 'β'],
    [/\\gamma/g, 'γ'],
    [/\\delta/g, 'δ'],
    [/\\epsilon/g, 'ε'],
    [/\\theta/g, 'θ'],
    [/\\lambda/g, 'λ'],
    [/\\mu/g, 'μ'],
    [/\\pi/g, 'π'],
    [/\\sigma/g, 'σ'],
    [/\\tau/g, 'τ'],
    [/\\phi/g, 'φ'],
    [/\\omega/g, 'ω'],
    [/\\infty/g, '∞'],
    [/\\times/g, '×'],
    [/\\pm/g, '±'],
    [/\\neq/g, '≠'],
    [/\\leq/g, '≤'],
    [/\\geq/g, '≥'],
    [/\\approx/g, '≈'],
    [/\\rightarrow/g, '→'],
    [/\\cdot/g, '·'],
    [/\\sum/g, '∑'],
    [/\\int/g, '∫'],
    [/\\sqrt\{([^}]+)\}/g, '√($1)'],
    [/\\frac\{([^}]+)\}\{([^}]+)\}/g, '($1 / $2)'],
    [/\^2/g, '²'],
    [/\^3/g, '³'],
    [/\^([0-9a-zA-Z])/g, '^$1'],
  ];

  for (const [r, repl] of replacements) {
    s = s.replace(r, repl);
  }
  return s;
}

