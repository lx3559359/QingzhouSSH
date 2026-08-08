import { describe, expect, it } from 'vitest';

import type { PersonalScriptSummary, TaskAvailability } from '../../../api/contracts';
import { buildToolLibrary, filterToolLibrary, groupCounts } from './toolLibrary';

function task(
  id: string,
  title: string,
  state: TaskAvailability['state'],
  alias: string,
): TaskAvailability {
  return {
    state,
    summary: state === 'ready' ? '可以直接运行' : '当前不可直接运行',
    missingCommands: state === 'remediable' ? ['tcpdump'] : [],
    remediation: state === 'remediable'
      ? { packageManager: 'apt', missingCommands: ['tcpdump'], packages: ['tcpdump'] }
      : null,
    library: {
      source: 'reviewed_command',
      primaryCategory: id.includes('network') ? 'network' : 'daily_inspection',
      keywords: [title, '检查'],
      noviceAliases: [alias],
    },
    definition: {
      id,
      version: 2,
      category: id.includes('network') ? 'network' : 'system',
      title,
      description: `${title}说明`,
      riskLevel: 'safe',
      estimatedSeconds: 30,
      privilege: 'current_user',
      scope: 'read_only_batch',
      parameters: [],
      implementations: [],
      outputKind: 'text',
    },
  };
}

const script: PersonalScriptSummary = {
  id: 'script-1',
  title: '清理应用缓存',
  category: '日常维护',
  tags: ['缓存', '应用'],
  isFavorite: true,
  isEnabled: true,
  activeVersionId: 'version-1',
  activeVersionNumber: 1,
  bodySha256: 'abc',
  shell: 'posix_sh',
  compatibility: { osFamilies: ['linux', 'bsd'], requiredCommands: ['sh'] },
  updatedAt: 10,
};

describe('unified tool library projection', () => {
  const tasks = [
    task('system.overview', '系统概览', 'ready', '服务器很慢'),
    task('network.packet_capture', '限时抓包摘要', 'remediable', '网络丢包'),
    task('network.udp', 'UDP 探测', 'permission_blocked', 'UDP 不通'),
    task('network.ip_change', '修改 IP 地址', 'unsupported', '修改地址'),
  ];

  it('projects built-ins and personal scripts into one searchable collection', () => {
    const items = buildToolLibrary(tasks, [script]);
    expect(items.map((item) => item.source)).toEqual([
      'reviewed_command',
      'reviewed_command',
      'reviewed_command',
      'reviewed_command',
      'personal_script',
    ]);
    expect(filterToolLibrary(items, { query: '服务器 很慢' }).map((item) => item.id))
      .toEqual(['system.overview']);
    expect(filterToolLibrary(items, { query: '缓存 应用' }).map((item) => item.id))
      .toEqual(['script-1']);
  });

  it('hides blocked states by default and applies every active filter with AND logic', () => {
    const items = buildToolLibrary(tasks, [script]);
    expect(filterToolLibrary(items, {}).map((item) => item.id)).toEqual([
      'system.overview',
      'network.packet_capture',
      'script-1',
    ]);
    expect(filterToolLibrary(items, {
      categories: ['network'],
      sources: ['reviewed_command'],
      risks: ['safe'],
      states: ['remediable'],
      query: '抓包 网络',
    }).map((item) => item.id)).toEqual(['network.packet_capture']);
    expect(filterToolLibrary(items, { states: ['permission_blocked', 'unsupported'] })
      .map((item) => item.id)).toEqual(['network.udp', 'network.ip_change']);
  });

  it('supports favorites, recent ordering and category counts', () => {
    const items = buildToolLibrary(tasks, [script]);
    expect(filterToolLibrary(items, { favoritesOnly: true }).map((item) => item.id))
      .toEqual(['script-1']);
    expect(filterToolLibrary(items, { recentIds: ['network.packet_capture', 'system.overview'] })
      .slice(0, 2).map((item) => item.id))
      .toEqual(['network.packet_capture', 'system.overview']);
    expect(groupCounts(items).network).toBe(3);
    expect(groupCounts(items).my_scripts).toBe(1);
  });
});
