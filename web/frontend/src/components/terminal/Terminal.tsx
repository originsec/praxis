import { useEffect, useRef } from 'react';
import { Terminal as XTerm, type ITheme } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import { useApp } from '../../context/AppContext';
import { useTheme } from '../../context/ThemeContext';

//
// Terminal color schemes for each app theme.
//

const TERMINAL_THEMES: Record<string, ITheme> = {
  //
  // Praxis Dark - green phosphor terminal aesthetic.
  //
  praxis_dark: {
    background: '#030712',
    foreground: '#9ee675',
    cursor: '#9ee675',
    cursorAccent: '#030712',
    black: '#030712',
    red: '#f87171',
    green: '#9ee675',
    yellow: '#ffd700',
    blue: '#00ffff',
    magenta: '#cc66ff',
    cyan: '#5c9c66',
    white: '#9ee675',
    brightBlack: '#4a5d52',
    brightRed: '#ff9b9b',
    brightGreen: '#b4ff8f',
    brightYellow: '#ffe066',
    brightBlue: '#66ffff',
    brightMagenta: '#dd99ff',
    brightCyan: '#7bbd7b',
    brightWhite: '#f2ffd5',
    selectionBackground: '#1f3229',
  },

  //
  // Origin Light - warm stone/bone tones.
  // Note: ANSI "white" must be dark and "black" light for visibility.
  //
  origin_light: {
    background: '#f6f5f2',
    foreground: '#181612',
    cursor: '#476955',
    cursorAccent: '#f6f5f2',
    black: '#e6e5e0',
    red: '#a83232',
    green: '#3a6a3a',
    yellow: '#8a7a34',
    blue: '#3a5a8a',
    magenta: '#6a4a7a',
    cyan: '#2a5a4a',
    white: '#181612',
    brightBlack: '#94938c',
    brightRed: '#c04040',
    brightGreen: '#4a8a4a',
    brightYellow: '#a08a44',
    brightBlue: '#4a6a9a',
    brightMagenta: '#7a5a8a',
    brightCyan: '#3a6a5a',
    brightWhite: '#2d2c26',
    selectionBackground: '#c5c4bf',
  },
};

interface TerminalProps {
  nodeId: string;
  terminalId: string;
}

export function Terminal({ nodeId, terminalId }: TerminalProps) {
  const termRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XTerm | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const { registerTerminalHandler, sendTerminalInput, sendCommand } = useApp();
  const { theme } = useTheme();

  useEffect(() => {
    if (!termRef.current) return;

    //
    // Get theme-appropriate terminal colors.
    //
    const terminalTheme = TERMINAL_THEMES[theme] || TERMINAL_THEMES.praxis_dark;

    //
    // Create terminal.
    //
    const xterm = new XTerm({
      cursorBlink: true,
      fontSize: 12,
      fontFamily: 'JetBrains Mono, Consolas, monospace',
      theme: terminalTheme,
    });

    const fitAddon = new FitAddon();
    xterm.loadAddon(fitAddon);

    xterm.open(termRef.current);
    fitAddon.fit();

    xtermRef.current = xterm;
    fitAddonRef.current = fitAddon;

    //
    // Handle input.
    //
    xterm.onData((data) => {
      const bytes = Array.from(new TextEncoder().encode(data));
      sendTerminalInput(nodeId, terminalId, bytes);
    });

    //
    // Handle resize.
    //
    const handleResize = () => {
      fitAddon.fit();
      sendCommand(nodeId, {
        Terminal: { Resize: { rows: xterm.rows, cols: xterm.cols } },
      });
    };

    xterm.onResize(handleResize);
    window.addEventListener('resize', () => fitAddon.fit());

    //
    // Observe container size changes (e.g. floating panel resize).
    //
    const resizeObserver = new ResizeObserver(() => {
      fitAddon.fit();
    });
    resizeObserver.observe(termRef.current);

    //
    // Register output handler.
    //
    const unregister = registerTerminalHandler(nodeId, terminalId, (output) => {
      const text = new TextDecoder().decode(new Uint8Array(output.data));
      xterm.write(text);
    });

    //
    // Initial resize notification, scrollback replay, and focus.
    //
    setTimeout(async () => {
      fitAddon.fit();
      sendCommand(nodeId, {
        Terminal: { Resize: { rows: xterm.rows, cols: xterm.cols } },
      });

      //
      // Request scrollback replay to restore previous terminal content.
      //
      try {
        const response = await sendCommand(nodeId, { Terminal: 'Replay' });
        if (response?.result && 'Terminal' in response.result) {
          const termResult = response.result.Terminal;
          if (typeof termResult === 'object' && 'Replay' in termResult && termResult.Replay.data?.length > 0) {
            const text = new TextDecoder().decode(new Uint8Array(termResult.Replay.data));
            xterm.write(text);
          }
        }
      } catch {
        // Replay not critical — continue without it.
      }

      xterm.focus();
    }, 100);

    return () => {
      unregister();
      resizeObserver.disconnect();
      xterm.dispose();
      window.removeEventListener('resize', () => fitAddon.fit());
    };
  }, [nodeId, terminalId, registerTerminalHandler, sendTerminalInput, sendCommand, theme]);

  //
  // Get background color from terminal theme for container.
  //
  const terminalTheme = TERMINAL_THEMES[theme] || TERMINAL_THEMES.praxis_dark;

  return (
    <div
      className="flex-1 min-h-0 flex flex-col p-1"
      style={{ backgroundColor: terminalTheme.background }}
    >
      <div ref={termRef} className="flex-1 min-h-0" />
    </div>
  );
}
