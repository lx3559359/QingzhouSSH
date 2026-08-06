import { Package, ShieldCheck, X } from '@phosphor-icons/react';

import type { TaskRemediationPreview } from '../../../api/contracts';

interface TaskRemediationDialogProps {
  preview: TaskRemediationPreview;
  serverName: string;
  taskTitle: string;
  busy: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}

export function TaskRemediationDialog({
  preview,
  serverName,
  taskTitle,
  busy,
  onCancel,
  onConfirm,
}: TaskRemediationDialogProps) {
  return (
    <div className="dialog-backdrop" role="presentation">
      <section
        className="silver-card modal-card remediation-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="remediation-dialog-title"
      >
        <header className="remediation-dialog__header">
          <div className="feature-icon feature-icon--green"><Package weight="duotone" /></div>
          <div>
            <span className="eyebrow">白名单组件 · 安装前确认</span>
            <h2 id="remediation-dialog-title">确认补齐组件</h2>
            <p>为“{taskTitle}”补齐服务器当前缺少的命令。</p>
          </div>
          <button className="icon-button" type="button" aria-label="关闭补齐组件窗口" disabled={busy} onClick={onCancel}><X /></button>
        </header>

        <dl className="remediation-dialog__facts">
          <div><dt>目标服务器</dt><dd>{serverName}</dd></div>
          <div><dt>软件包管理器</dt><dd>{preview.packageManager}</dd></div>
          <div><dt>缺少的命令</dt><dd>{preview.missingCommands.join('、')}</dd></div>
          <div><dt>准备安装</dt><dd>{preview.packages.join('、')}</dd></div>
        </dl>

        <div className="remediation-dialog__safety">
          <ShieldCheck weight="fill" />
          <div>
            <strong>安全边界</strong>
            <p>仅安装上方列出的白名单软件包，不会询问 sudo 密码。</p>
            <p>安装完成后只会重新检测兼容性，不会自动运行原任务。</p>
          </div>
        </div>

        <details className="technical-details">
          <summary>查看安装说明</summary>
          <code>{preview.commandSummary}</code>
        </details>

        <div className="modal-actions">
          <button className="secondary-button" type="button" disabled={busy} onClick={onCancel}>取消</button>
          <button className="success-button" type="button" disabled={busy} onClick={onConfirm}>{busy ? '正在安装…' : '确认安装组件'}</button>
        </div>
      </section>
    </div>
  );
}
