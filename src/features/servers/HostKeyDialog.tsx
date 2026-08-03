import {
  ShieldCheck,
  ShieldWarning,
  WarningOctagon,
} from '@phosphor-icons/react';
import { useState } from 'react';

import type { HostKeyCheck } from '../../api/contracts';

interface HostKeyDialogProps {
  check: HostKeyCheck;
  onApprove: () => Promise<void> | void;
  onContinue: () => Promise<void> | void;
  onClose: () => void;
}

export function HostKeyDialog({
  check,
  onApprove,
  onContinue,
  onClose,
}: HostKeyDialogProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');

  async function run(action: () => Promise<void> | void) {
    setBusy(true);
    setError('');
    try {
      await action();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  }

  const changed = check.decision === 'changed';
  const trusted = check.decision === 'trusted';

  return (
    <div className="dialog-backdrop" role="presentation">
      <section
        className={`silver-card modal-card host-key-dialog host-key-dialog--${check.decision}`}
        role="dialog"
        aria-modal="true"
        aria-labelledby="host-key-title"
      >
        <div
          className={`feature-icon ${
            changed
              ? 'feature-icon--red'
              : trusted
                ? 'feature-icon--green'
                : 'feature-icon--orange'
          }`}
          aria-hidden="true"
        >
          {changed ? (
            <WarningOctagon weight="duotone" />
          ) : trusted ? (
            <ShieldCheck weight="duotone" />
          ) : (
            <ShieldWarning weight="duotone" />
          )}
        </div>

        <div className="card-heading">
          <span className="eyebrow">SSH 主机身份</span>
          <h2 id="host-key-title">
            {changed
              ? '主机身份发生变化'
              : trusted
                ? '身份已验证'
                : '确认服务器身份'}
          </h2>
          <p>
            {changed
              ? '为避免连接到被冒充的服务器，本次连接已阻止。'
              : '请核对服务器提供的 SHA-256 指纹，再决定是否继续。'}
          </p>
        </div>

        <dl className="fingerprint-list">
          <div>
            <dt>算法</dt>
            <dd>{check.observed.algorithm}</dd>
          </div>
          {changed && check.trusted && (
            <div>
              <dt>原指纹</dt>
              <dd>{check.trusted.fingerprintSha256}</dd>
            </div>
          )}
          <div>
            <dt>{changed ? '新指纹' : 'SHA-256 指纹'}</dt>
            <dd>{check.observed.fingerprintSha256}</dd>
          </div>
        </dl>

        {error && (
          <p className="inline-message inline-message--error" role="alert">
            {error}
          </p>
        )}

        <footer className="modal-actions">
          {changed ? (
            <button className="danger-button" type="button" onClick={onClose}>
              关闭
            </button>
          ) : (
            <>
              <button className="secondary-button" type="button" onClick={onClose}>
                取消
              </button>
              <button
                className={trusted ? 'success-button' : 'primary-button'}
                type="button"
                disabled={busy}
                onClick={() => run(trusted ? onContinue : onApprove)}
              >
                <ShieldCheck weight="bold" aria-hidden="true" />
                {trusted ? '继续' : '信任并继续'}
              </button>
            </>
          )}
        </footer>
      </section>
    </div>
  );
}
