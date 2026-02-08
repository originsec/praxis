import { useRef, useCallback } from 'react';
import { Highlight, Prism } from 'prism-react-renderer';
import type { PrismTheme } from 'prism-react-renderer';

//
// Custom theme using CSS variables so it adapts to the active theme.
//

const luaTheme: PrismTheme = {
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
// Register Lua language with Prism.
//

(Prism as unknown as { languages: Record<string, unknown> }).languages.lua = {
  'comment': /^#!.+|--(?:\[(=*)\[[\s\S]*?\]\1\]|.*)/m,
  'string': {
    pattern: /(["'])(?:(?!\1)[^\\\r\n]|\\z(?:\r\n|\s)|\\(?:\r\n|[^z]))*\1|\[(=*)\[[\s\S]*?\]\2\]/,
    greedy: true,
  },
  'number': /\b0x[a-f\d]+(?:\.[a-f\d]*)?(?:p[+-]?\d+)?\b|\b\d+(?:\.\B|(?:\.\d*)?(?:e[+-]?\d+)?\b)|\B\.\d+(?:e[+-]?\d+)?\b/i,
  'keyword': /\b(?:and|break|do|else|elseif|end|false|for|function|goto|if|in|local|nil|not|or|repeat|return|then|true|until|while)\b/,
  'function': /(?!\d)\w+(?=\s*(?:[({]))/,
  'operator': [
    /[-+*%^&|#]|\/\/?|<[<=]?|>[>=]?|[=~]=?/,
    { pattern: /(^|[^.])\.\.(?!\.)/, lookbehind: true },
  ],
  'punctuation': /[\[\](){},;]|\.+|:+/,
};

interface LuaCodeEditorProps {
  value: string;
  onChange?: (value: string) => void;
  readOnly?: boolean;
}

//
// Code editor with Lua syntax highlighting. Uses a transparent textarea
// overlaid on a highlighted pre block for editable syntax coloring.
//

export function LuaCodeEditor({ value, onChange, readOnly = false }: LuaCodeEditorProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const preRef = useRef<HTMLPreElement>(null);

  const handleScroll = useCallback(() => {
    if (textareaRef.current && preRef.current) {
      preRef.current.scrollTop = textareaRef.current.scrollTop;
      preRef.current.scrollLeft = textareaRef.current.scrollLeft;
    }
  }, []);

  return (
    <div className="relative flex-1 overflow-hidden" style={{ minHeight: 0 }}>
      <Highlight
        prism={Prism}
        theme={luaTheme}
        code={value}
        language="lua"
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
