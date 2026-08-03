import {
  ArrowDown,
  ArrowUp,
  BracketsCurly,
  DownloadSimple,
  FileMagnifyingGlass,
  FlagCheckered,
  GitBranch,
  Lightning,
  UploadSimple,
} from '@phosphor-icons/react';

import type { WorkflowNodeConfig } from '../../api/contracts';

export type WorkflowStepType = WorkflowNodeConfig['type'];

const groups = [
  {
    title: '流程控制',
    items: [
      { type: 'start' as const, label: '开始', description: '唯一入口节点', icon: FlagCheckered, tone: 'green' },
      { type: 'condition' as const, label: '条件判断', description: '按结果选择分支', icon: GitBranch, tone: 'purple' },
      { type: 'stop' as const, label: '停止并提示', description: '结束流程并说明', icon: ArrowDown, tone: 'orange' },
    ],
  },
  {
    title: '快捷操作',
    items: [
      { type: 'task' as const, label: '快捷任务', description: '运行受控任务', icon: Lightning, tone: 'blue' },
      { type: 'custom' as const, label: '自定义命令', description: '高级命令或脚本', icon: BracketsCurly, tone: 'purple' },
      { type: 'upload' as const, label: '上传文件', description: '上传并可建恢复点', icon: UploadSimple, tone: 'orange' },
      { type: 'download' as const, label: '下载文件', description: '下载到数据目录', icon: DownloadSimple, tone: 'green' },
      { type: 'logSearch' as const, label: '检索日志', description: '搜索并保存结果', icon: FileMagnifyingGlass, tone: 'blue' },
    ],
  },
];

export function WorkflowLibrary({ onAdd }: { onAdd: (type: WorkflowStepType) => void }) {
  return (
    <aside className="silver-card workflow-library" aria-label="步骤库">
      <header>
        <span className="workflow-panel-kicker">步骤库</span>
        <strong>添加一个节点</strong>
        <small>点击即可放入画布，随后可拖动调整位置。</small>
      </header>
      {groups.map((group) => (
        <section key={group.title}>
          <h3>{group.title}</h3>
          <div className="workflow-library__items">
            {group.items.map(({ type, label, description, icon: Icon, tone }) => (
              <button type="button" key={type} aria-label={`添加${label}步骤`} onClick={() => onAdd(type)}>
                <span className={`workflow-step-icon workflow-step-icon--${tone}`}>
                  <Icon weight="duotone" aria-hidden="true" />
                </span>
                <span>
                  <strong>{label}</strong>
                  <small>{description}</small>
                </span>
                <ArrowUp className="workflow-library__add" aria-hidden="true" />
              </button>
            ))}
          </div>
        </section>
      ))}
    </aside>
  );
}
