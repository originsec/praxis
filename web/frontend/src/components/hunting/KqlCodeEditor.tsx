import { useRef, useCallback } from 'react';
import { Highlight, Prism } from 'prism-react-renderer';
import type { PrismTheme } from 'prism-react-renderer';

const kqlTheme: PrismTheme = {
  plain: {
    color: 'var(--text-primary)',
    backgroundColor: 'var(--bg-primary)',
  },
  styles: [
    { types: ['comment'], style: { color: 'var(--text-muted)' } },
    { types: ['string', 'char'], style: { color: 'var(--accent-warning)' } },
    { types: ['number'], style: { color: 'var(--accent-info)' } },
    { types: ['keyword'], style: { color: 'var(--accent-purple)' } },
    { types: ['function'], style: { color: 'var(--text-highlight)' } },
    { types: ['operator'], style: { color: 'var(--text-secondary)' } },
    { types: ['punctuation'], style: { color: 'var(--text-secondary)' } },
    { types: ['boolean'], style: { color: 'var(--accent-info)' } },
  ],
};

//
// Register KQL language with Prism.
//

(Prism as unknown as { languages: Record<string, unknown> }).languages.kql = {
  'comment': /\/\/.*/,
  'string': {
    pattern: /(["'])(?:(?!\1)[^\\\r\n]|\\[\s\S])*\1/,
    greedy: true,
  },
  'number': /\b\d+(?:\.\d+)?(?:e[+-]?\d+)?\b/i,
  'keyword': /\b(?:where|project|sort|order|take|limit|extend|summarize|count|distinct|by|asc|desc|and|or|not|contains|startswith|endswith|has|ago|now|true|false|null|top|project_away|union|join|let|print|datatable|in|between|matches|regex)\b/i,
  'function': /(?!\d)\w+(?=\s*\()/,
  'operator': /[|=!<>+\-*/%]+/,
  'punctuation': /[[\](){},;.]/,
};

interface KqlCodeEditorProps {
  value: string;
  onChange?: (value: string) => void;
  onCtrlEnter?: () => void;
  readOnly?: boolean;
}

export function KqlCodeEditor({ value, onChange, onCtrlEnter, readOnly = false }: KqlCodeEditorProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const preRef = useRef<HTMLPreElement>(null);

  const handleScroll = useCallback(() => {
    if (textareaRef.current && preRef.current) {
      preRef.current.scrollTop = textareaRef.current.scrollTop;
      preRef.current.scrollLeft = textareaRef.current.scrollLeft;
    }
  }, []);

  const handleKeyDown = useCallback((e: React.KeyboardEvent) => {
    if ((e.ctrlKey || e.metaKey) && e.key === 'Enter') {
      e.preventDefault();
      onCtrlEnter?.();
    }
  }, [onCtrlEnter]);

  return (
    <div className="relative flex-1 overflow-hidden" style={{ minHeight: 0 }}>
      <Highlight
        prism={Prism}
        theme={kqlTheme}
        code={value}
        language="kql"
      >
        {({ tokens, getLineProps, getTokenProps }) => (
          <pre
            ref={preRef}
            className="absolute inset-0 m-0 overflow-hidden pointer-events-none"
            style={{
              padding: '12px',
              paddingLeft: '48px',
              fontFamily: '"JetBrains Mono", "Fira Code", "SF Mono", "Cascadia Code", Menlo, Monaco, "Courier New", monospace',
              fontSize: '11px',
              lineHeight: '1.5',
              background: 'var(--bg-primary)',
              whiteSpace: 'pre',
              minWidth: 'fit-content',
            }}
          >
            {tokens.map((line, i) => {
              const lineProps = getLineProps({ line, key: i });
              return (
                <div key={i} {...lineProps} style={{ ...lineProps.style, display: 'flex' }}>
                  <span
                    style={{
                      width: '36px',
                      marginLeft: '-36px',
                      display: 'inline-block',
                      textAlign: 'right',
                      paddingRight: '12px',
                      color: 'var(--text-muted)',
                      opacity: 0.4,
                      userSelect: 'none',
                      flexShrink: 0,
                    }}
                  >
                    {i + 1}
                  </span>
                  <span>
                    {line.map((token, key) => {
                      const tokenProps = getTokenProps({ token, key });
                      return <span key={key} {...tokenProps} />;
                    })}
                  </span>
                </div>
              );
            })}
          </pre>
        )}
      </Highlight>

      <textarea
        ref={textareaRef}
        value={value}
        onChange={(e) => onChange?.(e.target.value)}
        onScroll={handleScroll}
        onKeyDown={handleKeyDown}
        readOnly={readOnly}
        spellCheck={false}
        className="absolute inset-0 w-full h-full resize-none focus:outline-none"
        style={{
          padding: '12px',
          paddingLeft: '48px',
          fontFamily: '"JetBrains Mono", "Fira Code", "SF Mono", "Cascadia Code", Menlo, Monaco, "Courier New", monospace',
          fontSize: '11px',
          lineHeight: '1.5',
          background: 'transparent',
          color: 'transparent',
          caretColor: 'var(--text-highlight)',
          whiteSpace: 'pre',
          overflowWrap: 'normal',
          tabSize: 2,
        }}
      />
    </div>
  );
}
