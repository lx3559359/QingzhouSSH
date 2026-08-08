import {
  Key,
  LockKey,
  ShieldCheck,
  X,
} from '@phosphor-icons/react';
import { type FormEvent, useState } from 'react';

import type { CreateServerRequest } from '../../api/contracts';

interface AddServerDialogProps {
  onSubmit: (request: CreateServerRequest) => Promise<void> | void;
  onCancel: () => void;
}

type AuthMode = 'password' | 'private_key';

export function AddServerDialog({ onSubmit, onCancel }: AddServerDialogProps) {
  const [name, setName] = useState('');
  const [host, setHost] = useState('');
  const [port, setPort] = useState('22');
  const [username, setUsername] = useState('');
  const [authMode, setAuthMode] = useState<AuthMode>('password');
  const [password, setPassword] = useState('');
  const [privateKey, setPrivateKey] = useState('');
  const [passphrase, setPassphrase] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);

  function clearSecrets() {
    setPassword('');
    setPrivateKey('');
    setPassphrase('');
  }

  function cancel() {
    clearSecrets();
    setError('');
    onCancel();
  }

  async function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setError('');
    const parsedPort = Number(port);
    if (!Number.isInteger(parsedPort) || parsedPort < 1 || parsedPort > 65535) {
      setError('端口必须在 1 到 65535 之间');
      return;
    }
    if (!name.trim() || !host.trim() || !username.trim()) {
      setError('名称、服务器地址和用户名不能为空');
      return;
    }
    if (authMode === 'password' && !password) {
      setError('请输入密码');
      return;
    }
    if (authMode === 'private_key' && !privateKey) {
      setError('请输入私钥内容');
      return;
    }

    const request: CreateServerRequest = {
      name: name.trim(),
      host: host.trim(),
      port: parsedPort,
      username: username.trim(),
      credential:
        authMode === 'password'
          ? { kind: 'password', password }
          : {
              kind: 'private_key',
              privateKey,
              passphrase: passphrase || null,
            },
    };

    setBusy(true);
    try {
      await onSubmit(request);
      clearSecrets();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="dialog-backdrop" role="presentation">
      <section
        className="silver-card modal-card add-server-dialog"
        role="dialog"
        aria-modal="true"
        aria-labelledby="add-server-title"
      >
        <header className="modal-header">
          <div>
            <span className="eyebrow">服务器配置</span>
            <h2 id="add-server-title">添加 Linux 服务器</h2>
          </div>
          <button className="icon-button" type="button" onClick={cancel} aria-label="取消">
            <X weight="bold" aria-hidden="true" />
          </button>
        </header>

        <form className="server-form" noValidate onSubmit={submit}>
          <div className="form-grid form-grid--two">
            <label>
              <span>名称</span>
              <input value={name} onChange={(event) => setName(event.target.value)} />
            </label>
            <label>
              <span>服务器地址</span>
              <input value={host} onChange={(event) => setHost(event.target.value)} />
            </label>
            <label>
              <span>端口</span>
              <input
                type="number"
                inputMode="numeric"
                min="1"
                max="65535"
                value={port}
                onChange={(event) => setPort(event.target.value)}
              />
            </label>
            <label>
              <span>用户名</span>
              <input value={username} onChange={(event) => setUsername(event.target.value)} />
            </label>
          </div>

          <fieldset className="auth-fieldset">
            <legend>认证方式</legend>
            <div className="segmented-control">
              <label className={authMode === 'password' ? 'is-selected' : ''}>
                <input
                  type="radio"
                  name="auth-mode"
                  value="password"
                  checked={authMode === 'password'}
                  onChange={() => setAuthMode('password')}
                />
                <LockKey weight="duotone" aria-hidden="true" />
                密码
              </label>
              <label className={authMode === 'private_key' ? 'is-selected' : ''}>
                <input
                  type="radio"
                  name="auth-mode"
                  value="private_key"
                  checked={authMode === 'private_key'}
                  onChange={() => setAuthMode('private_key')}
                />
                <Key weight="duotone" aria-hidden="true" />
                私钥
              </label>
            </div>
          </fieldset>

          {authMode === 'password' ? (
            <label>
              <span>密码</span>
              <input
                type="password"
                autoComplete="new-password"
                value={password}
                onChange={(event) => setPassword(event.target.value)}
              />
            </label>
          ) : (
            <div className="form-stack">
              <label>
                <span>私钥内容</span>
                <textarea
                  rows={6}
                  value={privateKey}
                  onChange={(event) => setPrivateKey(event.target.value)}
                />
              </label>
              <label>
                <span>私钥口令（可选）</span>
                <input
                  type="password"
                  autoComplete="new-password"
                  value={passphrase}
                  onChange={(event) => setPassphrase(event.target.value)}
                />
              </label>
            </div>
          )}

          <p className="security-caption">
            <ShieldCheck weight="fill" aria-hidden="true" />
            凭据只写入当前用户的系统安全存储；应用不会保存明文凭据。
          </p>
          {error && (
            <p className="inline-message inline-message--error" role="alert">
              {error}
            </p>
          )}

          <footer className="modal-actions">
            <button className="secondary-button" type="button" onClick={cancel}>
              取消
            </button>
            <button className="primary-button" type="submit" disabled={busy}>
              <ShieldCheck weight="bold" aria-hidden="true" />
              {busy ? '正在保存…' : '保存并检查身份'}
            </button>
          </footer>
        </form>
      </section>
    </div>
  );
}
