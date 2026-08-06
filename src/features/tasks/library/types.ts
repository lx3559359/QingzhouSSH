import type {
  PersonalScriptSummary,
  RiskLevel,
  TaskAvailability,
  TaskAvailabilityState,
  ToolLibraryCategory,
  ToolSource,
} from '../../../api/contracts';

export type UnifiedToolSource = ToolSource | 'personal_script';
export type UnifiedToolCategory = ToolLibraryCategory | 'my_scripts';

interface ToolLibraryItemBase {
  id: string;
  title: string;
  description: string;
  source: UnifiedToolSource;
  category: UnifiedToolCategory;
  risk: RiskLevel;
  state: TaskAvailabilityState;
  favorite: boolean;
  searchText: string;
}

export type ToolLibraryItem =
  | (ToolLibraryItemBase & {
      source: ToolSource;
      availability: TaskAvailability;
      script?: never;
    })
  | (ToolLibraryItemBase & {
      source: 'personal_script';
      script: PersonalScriptSummary;
      availability?: never;
    });

export interface ToolLibraryFilters {
  query?: string;
  categories?: UnifiedToolCategory[];
  sources?: UnifiedToolSource[];
  risks?: RiskLevel[];
  states?: TaskAvailabilityState[];
  favoritesOnly?: boolean;
  recentOnly?: boolean;
  recentIds?: string[];
}

export type ToolLibraryGroupCounts = Record<UnifiedToolCategory, number>;
