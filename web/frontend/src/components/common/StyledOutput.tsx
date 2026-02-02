type OutputBlockType = 'outgoing' | 'incoming' | 'error' | 'section' | 'iteration' | 'regular';

interface OutputBlock {
  type: OutputBlockType;
  label?: string;
  content: string;
}

function parseOutput(output: string): OutputBlock[] {
  const blocks: OutputBlock[] = [];
  const lines = output.split('\n');
  let currentBlock: OutputBlock | null = null;
  let contentLines: string[] = [];

  const flushBlock = () => {
    if (currentBlock) {
      currentBlock.content = contentLines.join('\n').trim();
      if (currentBlock.content || currentBlock.type !== 'regular') {
        blocks.push(currentBlock);
      }
    }
    contentLines = [];
  };

  for (const line of lines) {
    if (line.startsWith('>>> ')) {
      flushBlock();
      const label = line.slice(4).replace(/:$/, '');
      currentBlock = { type: 'outgoing', label, content: '' };
    } else if (line.startsWith('<<< ')) {
      flushBlock();
      const label = line.slice(4).replace(/:$/, '');
      currentBlock = { type: 'incoming', label, content: '' };
    } else if (line.startsWith('!!! ')) {
      flushBlock();
      currentBlock = { type: 'error', content: line.slice(4) };
    } else if (line.startsWith('=== ')) {
      flushBlock();
      currentBlock = { type: 'section', content: line.replace(/===/g, '').trim() };
    } else if (line.startsWith('--- ')) {
      flushBlock();
      currentBlock = { type: 'iteration', content: line.replace(/---/g, '').trim() };
    } else if (currentBlock) {
      contentLines.push(line);
    } else {
      //
      // Start a regular block.
      //
      currentBlock = { type: 'regular', content: '' };
      contentLines.push(line);
    }
  }
  flushBlock();

  return blocks;
}

export function StyledOutput({ output }: { output: string }) {
  const blocks = parseOutput(output);

  return (
    <div className="space-y-3">
      {blocks.map((block, idx) => {
        switch (block.type) {
          case 'outgoing':
            return (
              <div key={idx} className="border-l-2 border-[var(--accent-info)] pl-3">
                <div className="text-xs text-[var(--accent-info)] font-medium mb-1 flex items-center gap-1">
                  <span>→</span> {block.label}
                </div>
                <pre className="text-sm whitespace-pre-wrap font-mono text-muted">{block.content}</pre>
              </div>
            );
          case 'incoming': {
            const isToolResult = block.label?.startsWith('Tool result');
            const accentColor = isToolResult ? 'var(--accent-success)' : 'var(--accent-purple)';
            return (
              <div key={idx} className={`border-l-2 pl-3`} style={{ borderColor: accentColor }}>
                <div className="text-xs font-medium mb-1 flex items-center gap-1" style={{ color: accentColor }}>
                  <span>←</span> {block.label}
                </div>
                <pre className="text-sm whitespace-pre-wrap font-mono text-[var(--text-primary)]">{block.content}</pre>
              </div>
            );
          }
          case 'error':
            return (
              <div key={idx} className="border-l-2 border-[var(--accent-error)] pl-3 py-1 bg-[var(--accent-error)]/5 rounded-r">
                <pre className="text-sm whitespace-pre-wrap font-mono text-[var(--accent-error)]">{block.content}</pre>
              </div>
            );
          case 'section':
            return (
              <div key={idx} className="text-center py-2">
                <span className="text-xs font-semibold uppercase tracking-wider text-[var(--accent-warning)] bg-[var(--accent-warning)]/10 px-3 py-1 rounded-full">
                  {block.content}
                </span>
              </div>
            );
          case 'iteration':
            return (
              <div key={idx} className="text-center py-1">
                <span className="text-xs text-muted">— {block.content} —</span>
              </div>
            );
          default:
            return block.content ? (
              <pre key={idx} className="text-sm whitespace-pre-wrap font-mono text-muted">{block.content}</pre>
            ) : null;
        }
      })}
    </div>
  );
}
