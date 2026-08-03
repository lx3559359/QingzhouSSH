import type { ParameterDefinition } from '../../api/contracts';

interface ParameterFormProps {
  definitions: ParameterDefinition[];
  values: Record<string, unknown>;
  onChange: (name: string, value: unknown) => void;
}

export function ParameterForm({ definitions, values, onChange }: ParameterFormProps) {
  if (definitions.length === 0) {
    return <p className="parameter-empty">此任务不需要额外参数。</p>;
  }

  return (
    <div className="parameter-form">
      {definitions.map((definition) => {
        const value = values[definition.name] ?? '';
        if (definition.kind.type === 'boolean') {
          return (
            <label className="checkbox-field" key={definition.name}>
              <input
                type="checkbox"
                checked={Boolean(value)}
                onChange={(event) => onChange(definition.name, event.target.checked)}
              />
              <span><strong>{definition.label}</strong><small>{definition.description}</small></span>
            </label>
          );
        }
        if (definition.kind.type === 'enum') {
          return (
            <label key={definition.name}>
              <span>{definition.label}</span>
              <select
                aria-label={definition.label}
                value={String(value)}
                required={definition.required}
                onChange={(event) => onChange(definition.name, event.target.value)}
              >
                <option value="">请选择</option>
                {definition.kind.options.map((option) => <option key={option} value={option}>{option}</option>)}
              </select>
              <small>{definition.description}</small>
            </label>
          );
        }
        if (definition.kind.type === 'timeRange') {
          const range = (value && typeof value === 'object' ? value : {}) as { start?: string; end?: string };
          return (
            <fieldset className="time-range-field" key={definition.name}>
              <legend>{definition.label}</legend>
              <input aria-label={`${definition.label}开始`} type="datetime-local" value={range.start ?? ''} onChange={(event) => onChange(definition.name, { ...range, start: event.target.value })} />
              <input aria-label={`${definition.label}结束`} type="datetime-local" value={range.end ?? ''} onChange={(event) => onChange(definition.name, { ...range, end: event.target.value })} />
              <small>{definition.description}</small>
            </fieldset>
          );
        }
        const numberKind = definition.kind.type === 'integer' ? definition.kind : null;
        return (
          <label key={definition.name}>
            <span>{definition.label}</span>
            <input
              aria-label={definition.label}
              type={numberKind ? 'number' : 'text'}
              value={String(value)}
              required={definition.required}
              min={numberKind?.min}
              max={numberKind?.max}
              onChange={(event) =>
                onChange(
                  definition.name,
                  numberKind ? Number(event.target.value) : event.target.value,
                )
              }
            />
            <small>{definition.description}</small>
          </label>
        );
      })}
    </div>
  );
}
