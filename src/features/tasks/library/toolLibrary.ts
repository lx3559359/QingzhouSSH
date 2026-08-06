import type {
  PersonalScriptSummary,
  TaskAvailability,
  TaskAvailabilityState,
} from '../../../api/contracts';
import type {
  ToolLibraryFilters,
  ToolLibraryGroupCounts,
  ToolLibraryItem,
  UnifiedToolCategory,
} from './types';

const CATEGORIES: UnifiedToolCategory[] = [
  'recommended_recent',
  'daily_inspection',
  'performance',
  'storage',
  'network',
  'web_service',
  'security_login',
  'service_management',
  'container',
  'system_settings',
  'my_scripts',
];

export function buildToolLibrary(
  tasks: TaskAvailability[],
  scripts: PersonalScriptSummary[],
): ToolLibraryItem[] {
  const taskItems: ToolLibraryItem[] = tasks.map((availability) => {
    const { definition, library } = availability;
    return {
      id: definition.id,
      title: definition.title,
      description: definition.description,
      source: library.source,
      category: library.primaryCategory,
      risk: definition.riskLevel,
      state: availability.state,
      favorite: false,
      availability,
      searchText: normalizeSearchText([
        definition.id,
        definition.title,
        definition.description,
        definition.category,
        ...library.keywords,
        ...library.noviceAliases,
      ]),
    };
  });
  const scriptItems: ToolLibraryItem[] = scripts.map((script) => ({
    id: script.id,
    title: script.title,
    description: `${script.category} · ${script.tags.join(' · ')}`,
    source: 'personal_script',
    category: 'my_scripts',
    risk: 'dangerous',
    state: 'ready',
    favorite: script.isFavorite,
    script,
    searchText: normalizeSearchText([
      script.id,
      script.title,
      script.category,
      ...script.tags,
      '我的脚本',
      '个人脚本',
    ]),
  }));
  return [...taskItems, ...scriptItems];
}

export function filterToolLibrary(
  items: ToolLibraryItem[],
  filters: ToolLibraryFilters,
): ToolLibraryItem[] {
  const states: TaskAvailabilityState[] = filters.states?.length
    ? filters.states
    : ['ready', 'remediable'];
  const recentOrder = new Map((filters.recentIds ?? []).map((id, index) => [id, index]));
  const queryTokens = normalizeSearchText([filters.query ?? '']).split(/\s+/).filter(Boolean);

  return items
    .filter((item) => states.includes(item.state))
    .filter((item) => !filters.categories?.length || filters.categories.includes(item.category))
    .filter((item) => !filters.sources?.length || filters.sources.includes(item.source))
    .filter((item) => !filters.risks?.length || filters.risks.includes(item.risk))
    .filter((item) => !filters.favoritesOnly || item.favorite)
    .filter((item) => !filters.recentOnly || recentOrder.has(item.id))
    .filter((item) => queryTokens.every((token) => item.searchText.includes(token)))
    .sort((left, right) => {
      const leftRecent = recentOrder.get(left.id) ?? Number.MAX_SAFE_INTEGER;
      const rightRecent = recentOrder.get(right.id) ?? Number.MAX_SAFE_INTEGER;
      return leftRecent - rightRecent;
    });
}

export function groupCounts(items: ToolLibraryItem[]): ToolLibraryGroupCounts {
  const counts = Object.fromEntries(
    CATEGORIES.map((category) => [category, 0]),
  ) as ToolLibraryGroupCounts;
  for (const item of items) counts[item.category] += 1;
  return counts;
}

function normalizeSearchText(parts: string[]): string {
  return parts.join(' ').normalize('NFKC').toLocaleLowerCase('zh-CN');
}
