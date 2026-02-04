import { useEffect, useRef } from 'react';
import { Terminal as XTerm } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import { useApp } from '../../context/AppContext';

interface TerminalProps {
  nodeId: string;
  terminalId: string;
}

export function Terminal({ nodeId, terminalId }: TerminalProps) {
  const termRef = useRef<HTMLDivElement>(null);
  const xtermRef = useRef<XTerm | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const { registerTerminalHandler, sendTerminalInput, sendCommand } = useApp();

  useEffect(() => {
    if (!termRef.current) return;

    //
    // Create terminal.
    //
    const xterm = new XTerm({
      cursorBlink: true,
      fontSize: 12,
      fontFamily: 'JetBrains Mono, Consolas, monospace',
      theme: {
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
    // Register output handler.
    //
    const unregister = registerTerminalHandler(nodeId, terminalId, (output) => {
      const text = new TextDecoder().decode(new Uint8Array(output.data));
      xterm.write(text);
    });

    //
    // Initial resize notification and focus.
    //
    setTimeout(() => {
      fitAddon.fit();
      sendCommand(nodeId, {
        Terminal: { Resize: { rows: xterm.rows, cols: xterm.cols } },
      });
      xterm.focus();
    }, 100);

    return () => {
      unregister();
      xterm.dispose();
      window.removeEventListener('resize', () => fitAddon.fit());
    };
  }, [nodeId, terminalId, registerTerminalHandler, sendTerminalInput, sendCommand]);

  return (
    <div className="flex-1 min-h-0 flex flex-col bg-[var(--bg-primary)]" style={{ padding: '8px', paddingBottom: '24px' }}>
      <div ref={termRef} className="flex-1 min-h-0" />
    </div>
  );
}
