import type { ParameterDefinition, SystemCapabilities } from '../../api/contracts';

interface ParameterFormProps {
  definitions: ParameterDefinition[];
  values: Record<string, unknown>;
  capabilities?: SystemCapabilities | null;
  onChange: (name: string, value: unknown) => void;
}

export function ParameterForm({ definitions, values, capabilities, onChange }: ParameterFormProps) {
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
        if (
          definition.kind.type === 'interfaceName' ||
          definition.kind.type === 'serviceName' ||
          definition.kind.type === 'containerName' ||
          definition.kind.type === 'timezone'
        ) {
          const options = discoveredOptions(definition, capabilities ?? null);
          return (
            <label key={definition.name}>
              <span>{definition.label}</span>
              <select
                aria-label={definition.label}
                value={String(value)}
                required={definition.required}
                onChange={(event) => onChange(definition.name, event.target.value)}
              >
                <option value="">{options.length > 0 ? '请选择自动识别结果' : '未识别到可选项，请重新检测'}</option>
                {options.map((option) => (
                  <option key={option.value} value={option.value}>{option.label}</option>
                ))}
              </select>
              <small>{definition.description}</small>
            </label>
          );
        }
        if (definition.kind.type === 'serviceMultiSelect') {
          const selected = Array.isArray(value) ? value.map(String) : [];
          const services = capabilities?.services ?? [];
          const maxItems = definition.kind.maxItems;
          return (
            <fieldset className="parameter-choice-list" key={definition.name}>
              <legend>{definition.label}</legend>
              {services.length === 0 ? (
                <small>未识别到服务，请重新检测服务器参数。</small>
              ) : services.map((service) => (
                <label className="checkbox-field" key={service}>
                  <input
                    type="checkbox"
                    checked={selected.includes(service)}
                    disabled={!selected.includes(service) && selected.length >= maxItems}
                    onChange={(event) => onChange(
                      definition.name,
                      event.target.checked
                        ? [...selected, service]
                        : selected.filter((item) => item !== service),
                    )}
                  />
                  <span>{service}</span>
                </label>
              ))}
              <small>{definition.description}</small>
            </fieldset>
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
        if (definition.kind.type === 'managedId') {
          return (
            <label key={definition.name}>
              <span>{definition.label}</span>
              <input
                aria-label={definition.label}
                type="text"
                value={String(value)}
                required
                readOnly
              />
              <small>{definition.description}</small>
            </label>
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

function discoveredOptions(
  definition: ParameterDefinition,
  capabilities: SystemCapabilities | null,
): Array<{ value: string; label: string }> {
  if (!capabilities) return [];
  switch (definition.kind.type) {
    case 'interfaceName':
      return capabilities.interfaces.map((item) => {
        const details = [
          item.isDefault ? '默认' : null,
          item.isUp ? '已启用' : '未启用',
          item.addresses.join('、') || '暂无地址',
        ].filter(Boolean).join(' · ');
        return { value: item.name, label: `${item.name}（${details}）` };
      });
    case 'serviceName':
      return capabilities.services.map((service) => ({ value: service, label: service }));
    case 'containerName':
      return capabilities.containers.map((container) => ({ value: container, label: container }));
    case 'timezone':
      return capabilities.timezones.map((timezone) => ({
        value: timezone,
        label: timezone === capabilities.currentTimezone ? `${timezone}（当前）` : timezone,
      }));
    default:
      return [];
  }
}
