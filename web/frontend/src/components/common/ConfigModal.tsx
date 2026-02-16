import { useState, type ReactNode } from 'react';
import { Modal } from './Modal';
import { ChevronRight, Loader2, Save, Circle, CircleCheck } from 'lucide-react';

export type FieldType = 'text' | 'textarea' | 'select' | 'number' | 'toggle';

export interface SelectOption {
  value: string;
  label: string;
}

export interface FieldConfig {
  name: string;
  label: string;
  type: FieldType;
  placeholder?: string;
  required?: boolean;
  disabled?: boolean;
  rows?: number;
  options?: SelectOption[];
  help?: string;
  subtext?: string;
  span?: 'full' | 'half';
}

export interface SectionConfig {
  type: 'section';
  title?: string;
  collapsible?: boolean;
  fields: FieldConfig[];
}

export interface DividerConfig {
  type: 'divider';
}

export interface CustomConfig {
  type: 'custom';
  render: () => ReactNode;
}

export type ConfigItem = SectionConfig | DividerConfig | CustomConfig;

interface ConfigModalProps {
  isOpen: boolean;
  onClose: () => void;
  title: string;
  config: ConfigItem[];
  values: Record<string, any>;
  onChange: (name: string, value: any) => void;
  onSubmit: () => void;
  submitLabel?: string;
  submitIcon?: ReactNode;
  submitVariant?: 'default' | 'success' | 'warning' | 'info' | 'purple';
  isSubmitting?: boolean;
  submitDisabled?: boolean;
  error?: string | null;
  size?: 'sm' | 'md' | 'lg' | 'xl';
}

export function ConfigModal({
  isOpen,
  onClose,
  title,
  config,
  values,
  onChange,
  onSubmit,
  submitLabel = 'Save',
  submitIcon,
  submitVariant = 'default',
  isSubmitting = false,
  submitDisabled = false,
  error = null,
  size = 'lg',
}: ConfigModalProps) {
  const handleSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    onSubmit();
  };

  const getSubmitButtonClasses = () => {
    const base = 'inline-flex items-center gap-2 px-4 py-2 text-xs tracking-wider border border-dim transition-colors disabled:opacity-50';

    switch (submitVariant) {
      case 'success':
        return `${base} bg-[var(--accent-success)]/20 text-[var(--accent-success)] hover:border-[var(--accent-success)] hover:bg-[var(--accent-success)]/30`;
      case 'warning':
        return `${base} bg-[var(--accent-warning)]/20 text-[var(--accent-warning)] hover:border-[var(--accent-warning)] hover:bg-[var(--accent-warning)]/30`;
      case 'info':
        return `${base} bg-[var(--accent-info)]/20 text-[var(--accent-info)] hover:border-[var(--accent-info)] hover:bg-[var(--accent-info)]/30`;
      case 'purple':
        return `${base} bg-[var(--accent-purple)]/20 text-[var(--accent-purple)] hover:border-[var(--accent-purple)] hover:bg-[var(--accent-purple)]/30`;
      default:
        return `${base} bg-[var(--text-secondary)]/10 text-[var(--text-secondary)] hover:border-[var(--text-secondary)] hover:bg-[var(--text-secondary)]/20`;
    }
  };

  const renderField = (field: FieldConfig) => {
    const value = values[field.name] ?? '';
    const fieldClasses = "w-full bg-[var(--bg-primary)] border border-dim px-3 py-2 text-sm text-highlight focus:outline-none focus:border-subtle disabled:opacity-50 transition-colors";

    if (field.type === 'toggle') {
      const boolValue = !!value;
      return (
        <button
          key={field.name}
          type="button"
          onClick={() => onChange(field.name, !boolValue)}
          disabled={field.disabled || isSubmitting}
          className="flex items-center gap-2 disabled:opacity-50 hover:opacity-80 transition-opacity"
        >
          {boolValue ? (
            <CircleCheck size={16} className="text-[var(--accent-error)]" />
          ) : (
            <Circle size={16} className="text-[var(--text-secondary)]" />
          )}
          <span className={`text-xs tracking-wider ${boolValue ? 'text-[var(--accent-error)]' : 'text-[var(--text-secondary)]'}`}>
            {field.label}
          </span>
        </button>
      );
    }

    return (
      <div key={field.name} className={field.span === 'full' ? '' : ''}>
        <div className="flex items-baseline gap-2 mb-1.5">
          <label className="text-xs tracking-wider text-[var(--text-secondary)]">
            {field.label}
            {field.required && <span className="text-[var(--accent-error)]/70"> *</span>}
          </label>
          {field.subtext && (
            <span className="text-xs text-muted/60">{field.subtext}</span>
          )}
        </div>
        {field.type === 'text' && (
          <input
            type="text"
            value={value}
            onChange={(e) => onChange(field.name, e.target.value)}
            disabled={field.disabled || isSubmitting}
            className={fieldClasses}
            placeholder={field.placeholder}
            required={field.required}
          />
        )}
        {field.type === 'number' && (
          <input
            type="number"
            value={value}
            onChange={(e) => onChange(field.name, parseInt(e.target.value) || 0)}
            disabled={field.disabled || isSubmitting}
            className={fieldClasses}
            placeholder={field.placeholder}
            required={field.required}
          />
        )}
        {field.type === 'textarea' && (
          <textarea
            value={value}
            onChange={(e) => onChange(field.name, e.target.value)}
            disabled={field.disabled || isSubmitting}
            rows={field.rows || 4}
            className={`${fieldClasses} font-mono resize-none`}
            placeholder={field.placeholder}
            required={field.required}
          />
        )}
        {field.type === 'select' && (
          <select
            value={value}
            onChange={(e) => onChange(field.name, e.target.value)}
            disabled={field.disabled || isSubmitting}
            className={fieldClasses}
            required={field.required}
          >
            {field.options?.map((opt) => (
              <option key={opt.value} value={opt.value}>
                {opt.label}
              </option>
            ))}
          </select>
        )}
        {field.help && (
          <p className="text-xs mt-2 leading-relaxed" style={{ color: 'var(--text-muted)' }}>{field.help}</p>
        )}
      </div>
    );
  };

  //
  // Track collapsed state for collapsible sections by index.
  //
  const [collapsedSections, setCollapsedSections] = useState<Set<number>>(
    () => new Set(
      config
        .map((item, i) => (item.type === 'section' && (item as SectionConfig).collapsible ? i : -1))
        .filter(i => i >= 0)
    )
  );

  const toggleSection = (index: number) => {
    setCollapsedSections(prev => {
      const next = new Set(prev);
      if (next.has(index)) next.delete(index);
      else next.add(index);
      return next;
    });
  };

  const renderSectionFields = (section: SectionConfig) => {
    const fullSpanFields = section.fields.filter(f => f.span === 'full');
    const halfSpanFields = section.fields.filter(f => !f.span || f.span === 'half');
    const toggleFields = section.fields.filter(f => f.type === 'toggle');
    const nonToggleFullSpan = fullSpanFields.filter(f => f.type !== 'toggle');
    const nonToggleHalfSpan = halfSpanFields.filter(f => f.type !== 'toggle');

    return (
      <>
        {nonToggleFullSpan.map(renderField)}
        {nonToggleHalfSpan.length > 0 && (
          <div className="grid grid-cols-2 gap-3">
            {nonToggleHalfSpan.map(renderField)}
          </div>
        )}
        {toggleFields.length > 0 && (
          <div className="flex items-center gap-6">
            {toggleFields.map(renderField)}
          </div>
        )}
      </>
    );
  };

  const renderSection = (section: SectionConfig, index: number) => {
    const isCollapsible = section.collapsible;
    const isCollapsed = collapsedSections.has(index);

    return (
      <div key={`section-${index}`} className="bg-[var(--bg-secondary)]">
        {section.title && (
          <button
            type="button"
            onClick={isCollapsible ? () => toggleSection(index) : undefined}
            className={`flex items-center gap-1.5 px-2.5 pt-2.5 pb-1 text-[11px] tracking-widest text-[var(--text-secondary)] ${
              isCollapsible ? 'cursor-pointer hover:text-highlight transition-colors' : ''
            }`}
            style={{ letterSpacing: '0.08em' }}
            disabled={!isCollapsible}
          >
            {isCollapsible && (
              <ChevronRight
                size={12}
                className={`transition-transform ${isCollapsed ? '' : 'rotate-90'}`}
              />
            )}
            {section.title.toUpperCase()}
          </button>
        )}
        {!isCollapsed && (
          <div className="space-y-3 p-2.5">
            {renderSectionFields(section)}
          </div>
        )}
      </div>
    );
  };

  return (
    <Modal isOpen={isOpen} onClose={onClose} title={title} size={size}>
      <form onSubmit={handleSubmit} className="space-y-0">
        {config.map((item, index) => {
          if (item.type === 'section') {
            return renderSection(item as SectionConfig, index);
          }
          if (item.type === 'divider') {
            return <div key={`divider-${index}`} className="border-t border-dim" />;
          }
          if (item.type === 'custom') {
            return <div key={`custom-${index}`}>{(item as CustomConfig).render()}</div>;
          }
          return null;
        })}

        {error && (
          <div className="p-2.5 bg-[var(--bg-secondary)]">
            <div className="p-3 bg-[var(--accent-error)]/10 border border-[var(--accent-error)]/30 text-[var(--accent-error)] text-xs">
              {error}
            </div>
          </div>
        )}

        <div className="p-2.5 bg-[var(--bg-secondary)]">
          <div className="flex justify-end gap-2">
            <button
              type="button"
              onClick={onClose}
              disabled={isSubmitting}
              className="px-4 py-2 text-xs tracking-wider text-muted border border-dim hover:border-subtle hover:bg-[var(--highlight)] transition-colors disabled:opacity-50"
            >
              Cancel
            </button>
            <button
              type="submit"
              disabled={isSubmitting || submitDisabled}
              className={getSubmitButtonClasses()}
            >
              {isSubmitting && <Loader2 size={14} className="animate-spin" />}
              {!isSubmitting && (submitIcon || <Save size={14} />)}
              {submitLabel}
            </button>
          </div>
        </div>
      </form>
    </Modal>
  );
}
