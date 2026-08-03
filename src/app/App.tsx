import { Database, SpinnerGap } from '@phosphor-icons/react';
import { useCallback, useEffect, useState } from 'react';

import appIcon from '../../assets/app-icon.svg';
import type { BootstrapStatus } from '../api/contracts';
import { api } from '../api/tauri';
import { DataRootGate } from '../features/bootstrap/DataRootGate';
import { ServerListPage } from '../features/servers/ServerListPage';
import '../styles/theme.css';

function displayError(cause: unknown) {
  return cause instanceof Error ? cause.message : String(cause);
}

export function App() {
  const [bootstrap, setBootstrap] = useState<BootstrapStatus | null>(null);
  const [error, setError] = useState('');

  const loadBootstrap = useCallback(async () => {
    setBootstrap(null);
    setError('');
    try {
      setBootstrap(await api.bootstrapStatus());
    } catch (cause) {
      setError(displayError(cause));
    }
  }, []);

  useEffect(() => {
    void loadBootstrap();
  }, [loadBootstrap]);

  return (
    <main className="app-shell">
      <section className="app-window">
        <header className="app-topbar">
          <div className="brand-lockup">
            <img className="brand-icon" src={appIcon} alt="" />
            <div>
              <h1>轻舟 SSH</h1>
              <p>安全地完成 Linux 操作</p>
            </div>
          </div>
          {bootstrap?.state === 'ready' && (
            <div className="data-root-badge" title={bootstrap.dataRoot}>
              <Database weight="duotone" aria-hidden="true" />
              <span>{bootstrap.dataRoot}</span>
            </div>
          )}
        </header>

        <div className="app-content">
          {!bootstrap ? (
            <section className="loading-stage" aria-live="polite">
              <div className="silver-card loading-card" role="status">
                <SpinnerGap className="spin" weight="bold" aria-hidden="true" />
                <div>
                  <strong>正在准备本地环境…</strong>
                  <span>检查数据目录与安全存储</span>
                </div>
              </div>
              {error && (
                <div className="bootstrap-error" role="alert">
                  <span>{error}</span>
                  <button className="secondary-button" type="button" onClick={loadBootstrap}>
                    重试
                  </button>
                </div>
              )}
            </section>
          ) : bootstrap.state === 'needs_selection' ? (
            <DataRootGate status={bootstrap} onReady={setBootstrap} />
          ) : (
            <ServerListPage />
          )}
        </div>
      </section>
    </main>
  );
}
