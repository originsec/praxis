import type { AgentSessionMessage, OrchestratorMessage } from '../context/AppContext';
import type { SemanticOpUpdate, ChainExecutionUpdate, ElementExecutionStatus } from '../api/types';

//
// Helper to format a date nicely.
//
function formatDate(date: Date | string): string {
  const d = typeof date === 'string' ? new Date(date) : date;
  return d.toLocaleString();
}

//
// Helper to format duration.
//
function formatDuration(start: string, end: string | null): string {
  const startTime = new Date(start).getTime();
  const endTime = end ? new Date(end).getTime() : Date.now();
  const diffMs = endTime - startTime;
  const diffSecs = Math.floor(diffMs / 1000);
  const mins = Math.floor(diffSecs / 60);
  const secs = diffSecs % 60;
  return mins > 0 ? `${mins}m ${secs}s` : `${secs}s`;
}

//
// Get output from element status.
//
function getElementOutput(status: ElementExecutionStatus): string | null {
  if (typeof status === 'object' && 'Completed' in status) {
    return status.Completed.output;
  }
  if (typeof status === 'object' && 'Failed' in status) {
    return `Error: ${status.Failed.error}`;
  }
  return null;
}

//
// Get status string from element status.
//
function getElementStatusString(status: ElementExecutionStatus): string {
  if (typeof status === 'string') return status;
  if ('Completed' in status) return 'Completed';
  if ('Failed' in status) return 'Failed';
  return 'Unknown';
}

//
// Export agent session to Markdown.
//
export function exportAgentSession(
  messages: AgentSessionMessage[],
  agentName: string,
  nodeName: string
): string {
  const lines: string[] = [];

  lines.push('# Agent Session Transcript');
  lines.push('');
  lines.push(`**Agent:** ${agentName}`);
  lines.push(`**Node:** ${nodeName}`);
  if (messages.length > 0) {
    lines.push(`**Started:** ${formatDate(messages[0].timestamp)}`);
    lines.push(`**Ended:** ${formatDate(messages[messages.length - 1].timestamp)}`);
  }
  lines.push(`**Total Messages:** ${messages.length}`);
  lines.push('');
  lines.push('---');
  lines.push('');

  for (const msg of messages) {
    const role = msg.role === 'user' ? '**User**' : '**Agent**';
    lines.push(`### ${role} - ${formatDate(msg.timestamp)}`);
    lines.push('');
    lines.push(msg.content);
    lines.push('');
  }

  return lines.join('\n');
}

//
// Export Orchestrator session to Markdown.
//
export function exportOrchestratorSession(
  messages: OrchestratorMessage[],
  tokenUsage?: { promptTokens: number; completionTokens: number; totalTokens: number } | null
): string {
  const lines: string[] = [];

  lines.push('# Orchestrator Session Transcript');
  lines.push('');
  if (messages.length > 0) {
    lines.push(`**Started:** ${formatDate(messages[0].timestamp)}`);
    lines.push(`**Ended:** ${formatDate(messages[messages.length - 1].timestamp)}`);
  }
  lines.push(`**Total Messages:** ${messages.length}`);
  if (tokenUsage) {
    lines.push(`**Token Usage:** ${tokenUsage.totalTokens.toLocaleString()} total (${tokenUsage.promptTokens.toLocaleString()} prompt, ${tokenUsage.completionTokens.toLocaleString()} completion)`);
  }
  lines.push('');
  lines.push('---');
  lines.push('');

  for (const msg of messages) {
    const role = msg.role === 'user' ? '**User**' : msg.role === 'assistant' ? '**Orchestrator**' : '**System**';
    lines.push(`### ${role} - ${formatDate(msg.timestamp)}`);
    lines.push('');

    //
    // Show tool executions if present.
    //
    if (msg.toolExecutions && msg.toolExecutions.length > 0) {
      lines.push('<details>');
      lines.push(`<summary>Tool Calls (${msg.toolExecutions.length})</summary>`);
      lines.push('');
      lines.push('| Tool | Result | Status |');
      lines.push('|------|--------|--------|');
      for (const tool of msg.toolExecutions) {
        const status = tool.success ? '✓' : '✗';
        lines.push(`| \`${tool.name}\` | ${tool.display} | ${status} |`);
      }
      lines.push('');
      lines.push('</details>');
      lines.push('');
    }

    if (msg.content) {
      lines.push(msg.content);
    }
    lines.push('');
  }

  return lines.join('\n');
}

//
// Export operation result to Markdown.
//
export function exportOperationResult(op: SemanticOpUpdate): string {
  const lines: string[] = [];

  lines.push(`# Operation: ${op.spec.name}`);
  lines.push('');
  lines.push('## Summary');
  lines.push('');
  lines.push(`| Field | Value |`);
  lines.push(`|-------|-------|`);
  lines.push(`| **Operation ID** | \`${op.operation_id}\` |`);
  lines.push(`| **Status** | ${op.status} |`);
  lines.push(`| **Agent** | ${op.agent_short_name} |`);
  lines.push(`| **Node** | ${op.node_id} |`);
  lines.push(`| **Mode** | ${op.spec.mode} |`);
  lines.push(`| **Started** | ${formatDate(op.start_time)} |`);
  if (op.end_time) {
    lines.push(`| **Ended** | ${formatDate(op.end_time)} |`);
  }
  lines.push(`| **Duration** | ${formatDuration(op.start_time, op.end_time)} |`);
  lines.push('');

  lines.push('## Description');
  lines.push('');
  lines.push(op.spec.description);
  lines.push('');

  lines.push('## Prompt');
  lines.push('');
  lines.push('```');
  lines.push(op.spec.operation_prompt);
  lines.push('```');
  lines.push('');

  if (op.output) {
    lines.push('## Output');
    lines.push('');
    lines.push(op.output);
    lines.push('');
  }

  if (op.result) {
    lines.push('## Result');
    lines.push('');
    lines.push('```');
    lines.push(op.result);
    lines.push('```');
    lines.push('');
  }

  return lines.join('\n');
}

//
// Export chain execution to Markdown.
//
export function exportChainExecution(exec: ChainExecutionUpdate): string {
  const lines: string[] = [];

  lines.push(`# Chain Execution: ${exec.chain_name}`);
  lines.push('');
  lines.push('## Summary');
  lines.push('');
  lines.push(`| Field | Value |`);
  lines.push(`|-------|-------|`);
  lines.push(`| **Execution ID** | \`${exec.execution_id}\` |`);
  lines.push(`| **Chain ID** | \`${exec.chain_id}\` |`);
  lines.push(`| **Status** | ${exec.status} |`);
  lines.push(`| **Agent** | ${exec.agent_short_name} |`);
  lines.push(`| **Node** | ${exec.node_id} |`);
  lines.push(`| **Started** | ${formatDate(exec.started_at)} |`);
  if (exec.ended_at) {
    lines.push(`| **Ended** | ${formatDate(exec.ended_at)} |`);
  }
  lines.push(`| **Duration** | ${formatDuration(exec.started_at, exec.ended_at)} |`);
  lines.push('');

  //
  // Sort elements by started_at for chronological order.
  //
  const sortedElements = Object.entries(exec.elements)
    //
    // Only include elements that started.
    //
    .filter(([_, el]) => el.started_at)
    .sort((a, b) => {
      const aTime = a[1].started_at ? new Date(a[1].started_at).getTime() : 0;
      const bTime = b[1].started_at ? new Date(b[1].started_at).getTime() : 0;
      return aTime - bTime;
    });

  if (sortedElements.length > 0) {
    lines.push('## Element Executions');
    lines.push('');

    for (const [elementId, element] of sortedElements) {
      const statusStr = getElementStatusString(element.status);
      const statusEmoji = statusStr === 'Completed' ? '✓' : statusStr === 'Failed' ? '✗' : '○';

      lines.push(`### ${statusEmoji} ${elementId}`);
      lines.push('');

      if (element.config) {
        if (element.config.type === 'Operation') {
          lines.push(`**Type:** Operation (\`${element.config.operation_name}\`)`);
        } else if (element.config.type === 'Transform') {
          lines.push(`**Type:** Transform`);
          lines.push('');
          lines.push('**Prompt:**');
          lines.push('```');
          lines.push(element.config.prompt);
          lines.push('```');
        } else if (element.config.type === 'GenericPrompt') {
          lines.push(`**Type:** Generic Prompt`);
          lines.push('');
          lines.push('**Prompt:**');
          lines.push('```');
          lines.push(element.config.prompt);
          lines.push('```');
        } else if (element.config.type === 'SemanticOutput') {
          lines.push(`**Type:** Semantic Output`);
          lines.push('');
          lines.push('**Prompt:**');
          lines.push('```');
          lines.push(element.config.prompt);
          lines.push('```');
        } else {
          lines.push(`**Type:** ${element.config.type}`);
        }
        lines.push('');
      }

      if (element.context?.input) {
        lines.push('<details>');
        lines.push('<summary>Input</summary>');
        lines.push('');
        lines.push('```');
        lines.push(element.context.input);
        lines.push('```');
        lines.push('');
        lines.push('</details>');
        lines.push('');
      }

      if (element.started_at) {
        lines.push(`**Started:** ${formatDate(element.started_at)}`);
      }
      if (element.completed_at) {
        lines.push(`**Completed:** ${formatDate(element.completed_at)}`);
      }
      lines.push('');

      const output = getElementOutput(element.status);
      if (output) {
        lines.push('**Output:**');
        lines.push('');
        lines.push(output);
        lines.push('');
      }

      lines.push('---');
      lines.push('');
    }
  }

  //
  // Final outputs.
  //
  const outputs = Object.entries(exec.outputs);
  if (outputs.length > 0) {
    lines.push('## Final Outputs');
    lines.push('');

    for (const [label, output] of outputs) {
      lines.push(`### ${label}`);
      lines.push('');
      lines.push(output);
      lines.push('');
    }
  }

  return lines.join('\n');
}

//
// Download text as a file.
//
export function downloadTextFile(content: string, filename: string): void {
  const blob = new Blob([content], { type: 'text/markdown;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  URL.revokeObjectURL(url);
}

//
// Download JSON as a file.
//
export function downloadJsonFile(content: unknown, filename: string): void {
  const json = JSON.stringify(content, null, 2);
  const blob = new Blob([json], { type: 'application/json;charset=utf-8' });
  const url = URL.createObjectURL(blob);
  const link = document.createElement('a');
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  document.body.removeChild(link);
  URL.revokeObjectURL(url);
}

//
// Export chain definition to JSON (for import/export).
// Excludes id, created_at, updated_at as they will be regenerated on import.
//
export function exportChainDefinition(chain: {
  name: string;
  description: string;
  category: string;
  elements: unknown[];
  connections: unknown[];
  disabled?: boolean;
  timeout?: number;
}): object {
  return {
    name: chain.name,
    description: chain.description,
    category: chain.category,
    elements: chain.elements,
    connections: chain.connections,
    disabled: chain.disabled ?? false,
    timeout: chain.timeout,
  };
}

//
// Validate imported chain definition structure.
//
export function validateChainImport(data: unknown): { valid: boolean; error?: string } {
  if (!data || typeof data !== 'object') {
    return { valid: false, error: 'Invalid JSON: expected an object' };
  }

  const chain = data as Record<string, unknown>;

  if (typeof chain.name !== 'string' || !chain.name.trim()) {
    return { valid: false, error: 'Missing or invalid "name" field' };
  }

  if (!Array.isArray(chain.elements)) {
    return { valid: false, error: 'Missing or invalid "elements" field (expected array)' };
  }

  if (!Array.isArray(chain.connections)) {
    return { valid: false, error: 'Missing or invalid "connections" field (expected array)' };
  }

  //
  // Validate elements have required fields.
  //
  for (let i = 0; i < chain.elements.length; i++) {
    const elem = chain.elements[i] as Record<string, unknown>;
    if (!elem || typeof elem !== 'object') {
      return { valid: false, error: `Element ${i} is not an object` };
    }
    if (!elem.element_type) {
      return { valid: false, error: `Element ${i} missing "element_type"` };
    }
    if (!elem.id) {
      return { valid: false, error: `Element ${i} missing "id"` };
    }
  }

  //
  // Validate connections have required fields.
  //
  for (let i = 0; i < chain.connections.length; i++) {
    const conn = chain.connections[i] as Record<string, unknown>;
    if (!conn || typeof conn !== 'object') {
      return { valid: false, error: `Connection ${i} is not an object` };
    }
    if (!conn.from_element || !conn.to_element) {
      return { valid: false, error: `Connection ${i} missing "from_element" or "to_element"` };
    }
  }

  return { valid: true };
}
