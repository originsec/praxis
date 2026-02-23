import {
  CheckCircle,
  Loader2,
  Circle,
  ListTodo,
} from 'lucide-react';
import type { OrchestratorPlan, PlanStep } from '../../api/types';

function PlanStepIcon({ status }: { status: PlanStep['status'] }) {
  switch (status) {
    case 'done':
      return <CheckCircle size={10} className="text-[var(--accent-success)]" />;
    case 'in_progress':
      return <Loader2 size={10} className="text-[var(--accent-warning)] animate-spin" />;
    case 'not_started':
    default:
      return <Circle size={10} className="text-muted" />;
  }
}

export function PlanDisplay({ plan }: { plan: OrchestratorPlan }) {
  const doneCount = plan.steps.filter((s) => s.status === 'done').length;
  const totalCount = plan.steps.length;
  const progressPercent = totalCount > 0 ? (doneCount / totalCount) * 100 : 0;

  return (
    <div className="bg-[var(--bg-tertiary)] p-3 mb-3 border border-subtle">
      <div className="flex items-center gap-2 mb-2">
        <ListTodo size={12} className="text-[var(--accent-purple)]" />
        <span className="font-medium text-xs">Plan</span>
        <span className="text-[10px] text-muted ml-auto">
          {doneCount}/{totalCount}
        </span>
      </div>

      <div className="h-0.5 bg-[var(--bg-secondary)] rounded-full mb-2 overflow-hidden">
        <div
          className="h-full bg-[var(--accent-purple)]/60 transition-all duration-300"
          style={{ width: `${progressPercent}%` }}
        />
      </div>

      {plan.current_step_description && (
        <div className="text-xs text-[var(--accent-warning)] mb-2 font-medium">
          {plan.current_step_description}
        </div>
      )}

      <div className="space-y-1">
        {plan.steps.map((step, idx) => (
          <div
            key={idx}
            className={`flex items-start gap-1.5 text-xs ${
              step.status === 'done'
                ? 'text-muted line-through'
                : step.status === 'in_progress'
                ? 'text-[var(--text-primary)]'
                : 'text-[var(--text-secondary)]'
            }`}
          >
            <div className="mt-0.5">
              <PlanStepIcon status={step.status} />
            </div>
            <span>{step.description}</span>
          </div>
        ))}
      </div>

      {plan.summary && (
        <div className="mt-2 pt-2 border-t border-subtle text-xs text-[var(--text-highlight)]/50 italic">
          {plan.summary}
        </div>
      )}
    </div>
  );
}
