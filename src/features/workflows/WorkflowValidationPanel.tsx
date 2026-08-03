import { CheckCircle, WarningCircle } from '@phosphor-icons/react';

import type { WorkflowValidationReport } from '../../api/contracts';

export function WorkflowValidationPanel({
  report,
  onLocate,
}: {
  report: WorkflowValidationReport | null;
  onLocate: (nodeId: string) => void;
}) {
  if (!report) return null;
  return (
    <section
      className={`workflow-validation ${report.valid ? 'workflow-validation--valid' : 'workflow-validation--invalid'}`}
      aria-label="校验结果"
    >
      <header>
        {report.valid ? <CheckCircle weight="fill" aria-hidden="true" /> : <WarningCircle weight="fill" aria-hidden="true" />}
        <div>
          <strong>{report.valid ? '工作流校验通过' : `${report.diagnostics.length} 项需要处理`}</strong>
          <small>{report.valid ? '保存或运行前仍会由 Rust 再次校验。' : '点击问题可定位对应节点。'}</small>
        </div>
      </header>
      {!report.valid && (
        <ul>
          {report.diagnostics.map((diagnostic, index) => (
            <li key={`${diagnostic.code}-${diagnostic.nodeId}-${index}`}>
              {diagnostic.nodeId ? (
                <button type="button" aria-label={`定位：${diagnostic.message}`} onClick={() => onLocate(diagnostic.nodeId!)}>
                  <span>{diagnostic.message}</span><small>{diagnostic.code}</small>
                </button>
              ) : (
                <div><span>{diagnostic.message}</span><small>{diagnostic.code}</small></div>
              )}
            </li>
          ))}
        </ul>
      )}
    </section>
  );
}
