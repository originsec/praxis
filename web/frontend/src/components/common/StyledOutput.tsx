import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

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
    <div className="space-y-2">
      {blocks.map((block, idx) => {
        switch (block.type) {
          case 'outgoing':
            return (
              <div key={idx} className="border-l border-[var(--text-secondary)] pl-2">
                <div className="text-[10px] text-[var(--text-secondary)] font-medium mb-0.5 flex items-center gap-1">
                  <span>→</span> {block.label}
                </div>
                <pre className="text-[11px] whitespace-pre-wrap font-mono text-muted">{block.content}</pre>
              </div>
            );
          case 'incoming': {
            const isToolResult = block.label?.startsWith('Tool result');
            const borderColor = isToolResult ? 'var(--accent-info)' : 'var(--text-secondary)';

            //
            // For AI Response blocks, strip out tool call JSON (starts with {"tool": or
            // {"complete":). These are internal signals, not user-facing content.
            // Also strip code blocks containing these JSON patterns, and empty code blocks.
            //
            let displayContent = block.content;
            if (block.label === 'AI Response') {
              displayContent = displayContent
                .replace(/```[a-z]*\s*\n?\s*\{"\s*tool\s*"[\s\S]*?```/gi, '')
                .replace(/```[a-z]*\s*\n?\s*\{"\s*complete\s*"[\s\S]*?```/gi, '')
                .replace(/\{"tool":\s*"[^"]+",\s*"args":\s*\{[^}]*\}\}/g, '')
                .replace(/\{"complete":\s*(true|false)(,\s*"summary":\s*"[^"]*")?(,\s*"result":\s*"[^"]*")?\}/g, '');

              //
              // Strip empty code blocks (any code block containing only whitespace).
              //
              displayContent = displayContent.replace(/```[a-z]*\n[\s\n]*```/gi, '').trim();
            }

            if (!displayContent) return null;

            return (
              <div key={idx} className="border-l pl-2" style={{ borderColor }}>
                <div className="text-[10px] font-medium mb-0.5 flex items-center gap-1 text-[var(--text-secondary)]">
                  <span>←</span> {block.label}
                </div>
                <div className="prose prose-xs prose-invert max-w-none text-[11px] [&_table]:text-[10px] [&_th]:p-0.5 [&_td]:p-0.5 [&_p]:my-0.5 [&_ul]:my-0.5 [&_li]:my-0 [&_h3]:text-xs [&_h3]:my-1">
                  <ReactMarkdown remarkPlugins={[remarkGfm]}>{displayContent}</ReactMarkdown>
                </div>
              </div>
            );
          }
          case 'error':
            return (
              <div key={idx} className="border-l border-[var(--accent-error)] pl-2 py-0.5">
                <pre className="text-[11px] whitespace-pre-wrap font-mono text-[var(--accent-error)]">{block.content}</pre>
              </div>
            );
          case 'section':
            return (
              <div key={idx} className="text-center py-1">
                <span className="text-[10px] font-semibold uppercase tracking-wider text-muted">
                  {block.content}
                </span>
              </div>
            );
          case 'iteration':
            return (
              <div key={idx} className="text-center py-0.5">
                <span className="text-[10px] text-muted">— {block.content} —</span>
              </div>
            );
          default:
            return block.content ? (
              <div key={idx} className="prose prose-xs prose-invert max-w-none text-[11px] [&_table]:text-[10px] [&_th]:p-0.5 [&_td]:p-0.5 [&_p]:my-0.5 [&_ul]:my-0.5 [&_li]:my-0 [&_h3]:text-xs [&_h3]:my-1">
                <ReactMarkdown remarkPlugins={[remarkGfm]}>{block.content}</ReactMarkdown>
              </div>
            ) : null;
        }
      })}
    </div>
  );
}
