import { ArrowClockwise, ShieldWarning } from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import type { ExecutionDetails, ExecutionEvent, SystemCapabilities } from '../../../api/contracts';
import { ExecutionDrawer } from '../ExecutionDrawer';
import { ParameterForm } from '../ParameterForm';
import type { ToolLibraryItem } from './types';

interface ToolDetailPaneProps {
  item: ToolLibraryItem | null;
  parameters: Record<string, unknown>;
  events: ExecutionEvent[];
  details: ExecutionDetails | null;
  running: boolean;
  capabilities: SystemCapabilities | null;
  capabilitiesDetectedAt: number | null;
  refreshingCapabilities: boolean;
  onParameterChange: (name: string, value: unknown) => void;
  onRefreshCapabilities: () => void;
  onRun: () => void;
  onCancel: () => void;
  onRemediate: () => void;
}

export function ToolDetailPane({
  item,
  parameters,
  events,
  details,
  running,
  capabilities,
  capabilitiesDetectedAt,
  refreshingCapabilities,
  onParameterChange,
  onRefreshCapabilities,
  onRun,
  onCancel,
  onRemediate,
}: ToolDetailPaneProps) {
  const [tab, setTab] = useState<'parameters' | 'results'>('parameters');
  useEffect(() => {
    if (running || events.length > 0 || details) setTab('results');
  }, [details, events.length, running]);
  return (
    <aside className="silver-card tool-detail-pane" aria-label="工具详情">
      {!item ? (
        <div className="tool-empty-state">请选择一个工具查看说明和参数。</div>
      ) : (
        <>
          <header className="tool-detail-pane__header">
            <div>
              <span className="eyebrow">{item.source === 'personal_script' ? '我的脚本' : '工具详情'}</span>
              <h2>{item.title}</h2>
              <p>{item.description}</p>
            </div>
            {item.risk === 'dangerous' && <ShieldWarning className="danger-icon" weight="fill" />}
          </header>
          <div className="tool-detail-tabs" role="tablist" aria-label="详情内容">
            <button type="button" role="tab" aria-selected={tab === 'parameters'} className={tab === 'parameters' ? 'is-active' : ''} onClick={() => setTab('parameters')}>参数与说明</button>
            <button type="button" role="tab" aria-selected={tab === 'results'} className={tab === 'results' ? 'is-active' : ''} onClick={() => setTab('results')}>运行结果</button>
          </div>
          {tab === 'results' ? (
            <ExecutionDrawer events={events} details={details} running={running} onCancel={onCancel} />
          ) : item.source === 'personal_script' ? (
            <div className="tool-detail-body">
              <p className="tool-status-note">脚本已经加入统一工具库。运行前仍会显示完整安全预演和二次确认。</p>
              <button className="danger-button" type="button" disabled={running} onClick={onRun}>检查并运行脚本</button>
            </div>
          ) : (
            <div className="tool-detail-body">
              <div className={`tool-status-note tool-status-note--${item.state}`}>
                <strong>{item.availability.summary}</strong>
                {item.availability.missingCommands.length > 0 && <span>缺少：{item.availability.missingCommands.join('、')}</span>}
              </div>
              {item.availability.definition.parameters.length > 0 && (
                <div className="task-capability-status">
                  <div>
                    <strong>已自动识别服务器参数</strong>
                    <span>
                      {capabilitySummary(item, capabilities)}
                      {capabilitiesDetectedAt ? ` · ${new Date(capabilitiesDetectedAt).toLocaleTimeString()}` : ''}
                    </span>
                  </div>
                  <button
                    className="secondary-button compact-button"
                    type="button"
                    disabled={refreshingCapabilities}
                    onClick={onRefreshCapabilities}
                    aria-label="重新检测服务器参数"
                  >
                    <ArrowClockwise className={refreshingCapabilities ? 'spin' : ''} />
                    {refreshingCapabilities ? '检测中' : '重新检测'}
                  </button>
                </div>
              )}
              <ParameterForm
                definitions={item.availability.definition.parameters}
                values={parameters}
                capabilities={capabilities}
                onChange={onParameterChange}
              />
              {item.state === 'remediable' ? (
                <button className="success-button" type="button" onClick={onRemediate}>查看并补齐组件</button>
              ) : (
                <button
                  className={item.risk === 'dangerous' ? 'danger-button' : 'success-button'}
                  type="button"
                  disabled={running || item.state !== 'ready'}
                  onClick={onRun}
                >
                  运行任务
                </button>
              )}
            </div>
          )}
        </>
      )}
    </aside>
  );
}

function capabilitySummary(
  item: ToolLibraryItem,
  capabilities: SystemCapabilities | null,
): string {
  if (!capabilities) return '尚未取得识别结果';
  if (item.id === 'network.ip_change') {
    const network = capabilities.interfaces.find((candidate) => candidate.isDefault)
      ?? capabilities.interfaces.find((candidate) => candidate.isUp && candidate.name !== 'lo');
    return network
      ? `默认网卡 ${network.name} · 当前 IP ${network.addresses.join('、') || '无'} · 网关 ${network.gateway4 ?? network.gateway6 ?? '未识别'}`
      : '未识别到可用网卡';
  }
  if (item.id === 'system.timezone_change' || item.id === 'system.time_sync_change') {
    const ntp = capabilities.ntpEnabled === null
      ? '自动校时未知'
      : capabilities.ntpEnabled
        ? capabilities.ntpSynchronized ? '自动校时已同步' : '自动校时已开启，等待同步'
        : '自动校时未开启';
    return `${capabilities.currentTime ?? '当前时间未知'} · ${capabilities.currentTimezone ?? '时区未知'} · ${ntp}`;
  }
  return `${capabilities.interfaces.length} 个网卡 · ${capabilities.services.length} 个服务 · ${capabilities.containers.length} 个容器`;
}
