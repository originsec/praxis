import { useRef, useCallback } from 'react';
import { Highlight, Prism } from 'prism-react-renderer';
import type { PrismTheme } from 'prism-react-renderer';

//
// Register Lua grammar with Prism since it's not built-in.
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

//
// Theme using CSS variables so it adapts to the active theme.
//

const editorTheme: PrismTheme = {
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
    { types: ['property'], style: { color: 'var(--text-highlight)' } },
    { types: ['tag'], style: { color: 'var(--accent-purple)' } },
    { types: ['attr-name'], style: { color: 'var(--accent-warning)' } },
    { types: ['attr-value'], style: { color: 'var(--accent-info)' } },
    { types: ['selector'], style: { color: 'var(--accent-purple)' } },
    { types: ['title'], style: { color: 'var(--text-highlight)' } },
  ],
};

interface CodeEditorProps {
  value: string;
  onChange?: (value: string) => void;
  readOnly?: boolean;
  language: string;
}

//
// Derive Prism language identifier from a language string. For languages not
// bundled in prism-react-renderer, falls back to plaintext rendering.
//

function resolvePrismLanguage(lang: string): string {
  const prismLangs = (Prism as unknown as { languages: Record<string, unknown> }).languages;
  if (prismLangs[lang]) return lang;
  return 'plaintext';
}

//
// Map file extensions to language identifiers.
//

const EXT_MAP: Record<string, string> = {
  json: 'json',
  md: 'markdown',
  lua: 'lua',
  yaml: 'yaml',
  yml: 'yaml',
  toml: 'toml',
  xml: 'xml',
  sh: 'bash',
  bash: 'bash',
};

export function languageFromPath(path: string): string {
  const ext = path.split('.').pop()?.toLowerCase() ?? '';
  return EXT_MAP[ext] ?? 'plaintext';
}

//
// Code editor with syntax highlighting. Uses a transparent textarea overlaid
// on a highlighted pre block for editable syntax coloring.
//

export function CodeEditor({ value, onChange, readOnly = false, language }: CodeEditorProps) {
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const preRef = useRef<HTMLPreElement>(null);

  const handleScroll = useCallback(() => {
    if (textareaRef.current && preRef.current) {
      preRef.current.scrollTop = textareaRef.current.scrollTop;
      preRef.current.scrollLeft = textareaRef.current.scrollLeft;
    }
  }, []);

  const prismLanguage = resolvePrismLanguage(language);

  return (
    <div className="relative flex-1 overflow-hidden" style={{ minHeight: 0 }}>
      <Highlight
        prism={Prism}
        theme={editorTheme}
        code={value}
        language={prismLanguage}
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
