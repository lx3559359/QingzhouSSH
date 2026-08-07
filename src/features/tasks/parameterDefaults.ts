import type { SystemCapabilities, TaskDefinition } from '../../api/contracts';

export function buildInitialParameters(
  definition: TaskDefinition,
  capabilities: SystemCapabilities | null,
): Record<string, unknown> {
  const values: Record<string, unknown> = {};
  for (const parameter of definition.parameters) {
    if (parameter.kind.type === 'managedId') {
      values[parameter.name] = crypto.randomUUID();
    } else if (parameter.defaultValue !== null) {
      values[parameter.name] = parameter.defaultValue;
    }
  }
  if (!capabilities) return values;

  const defaultInterface = capabilities.interfaces.find((item) => item.isDefault)
    ?? capabilities.interfaces.find((item) => item.isUp && item.name !== 'lo')
    ?? capabilities.interfaces.find((item) => item.name !== 'lo');
  const interfaceParameter = definition.parameters.find((item) => item.kind.type === 'interfaceName');
  if (interfaceParameter && defaultInterface) {
    values[interfaceParameter.name] = defaultInterface.name;
  }

  const timezoneParameter = definition.parameters.find((item) => item.kind.type === 'timezone');
  if (timezoneParameter && capabilities.currentTimezone) {
    values[timezoneParameter.name] = capabilities.currentTimezone;
  }

  if (definition.id === 'network.ip_change' && defaultInterface?.gateway4) {
    values.gateway = defaultInterface.gateway4;
  }
  if (definition.id === 'system.time_sync_change' && capabilities.ntpEnabled !== null) {
    values.enabled = capabilities.ntpEnabled;
  }
  return values;
}

export function updateDependentParameters(
  taskId: string,
  current: Record<string, unknown>,
  name: string,
  value: unknown,
  capabilities: SystemCapabilities | null,
): Record<string, unknown> {
  const next = { ...current, [name]: value };
  if (taskId !== 'network.ip_change' || name !== 'interface' || typeof value !== 'string') {
    return next;
  }
  const selected = capabilities?.interfaces.find((item) => item.name === value);
  if (selected?.gateway4) next.gateway = selected.gateway4;
  else delete next.gateway;
  return next;
}
