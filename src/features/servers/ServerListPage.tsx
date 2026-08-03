import {
  ArrowRight,
  CheckCircle,
  Cpu,
  DesktopTower,
  GearSix,
  HardDrives,
  Key,
  Package,
  Plus,
  ShieldWarning,
  SpinnerGap,
  TerminalWindow,
  WifiHigh,
} from '@phosphor-icons/react';
import { useEffect, useState } from 'react';

import type {
  CreateServerRequest,
  HostKeyCheck,
  ServerProfile,
  SystemCapabilities,
} from '../../api/contracts';
import { api } from '../../api/tauri';
import { AddServerDialog } from './AddServerDialog';
import { HostKeyDialog } from './HostKeyDialog';

type ConnectionState = 'saved' | 'checking' | 'ready' | 'blocked' | 'failed';

interface VerificationState {
  server: ServerProfile;
  check: HostKeyCheck;
}

const connectionLabels: Record<ConnectionState, string> = {
  saved: '已保存',
  checking: '正在检查',
  ready: '连接正常',
  blocked: '身份已变化',
  failed: '连接失败',
};

function displayError(cause: unknown) {
  return cause instanceof Error ? cause.message : String(cause);
}

export function ServerListPage() {
  const [servers, setServers] = useState<ServerProfile[]>([]);
  const [connectionStates, setConnectionStates] = useState<
    Record<string, ConnectionState>
  >({});
  const [capabilities, setCapabilities] = useState<
    Record<string, SystemCapabilities>
  >({});
  const [verification, setVerification] = useState<VerificationState | null>(null);
  const [showAddServer, setShowAddServer] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');

  useEffect(() => {
    let active = true;
    api
      .listServers()
      .then((profiles) => {
        if (!active) return;
        setServers(profiles);
        setConnectionStates(
          Object.fromEntries(profiles.map((profile) => [profile.id, 'saved'])),
        );
      })
      .catch((cause) => active && setError(displayError(cause)))
      .finally(() => active && setLoading(false));
    return () => {
      active = false;
    };
  }, []);

  async function connect(server: ServerProfile) {
    setConnectionStates((current) => ({ ...current, [server.id]: 'checking' }));
    try {
      const result = await api.testConnection(server.id);
      setCapabilities((current) => ({ ...current, [server.id]: result }));
      setConnectionStates((current) => ({ ...current, [server.id]: 'ready' }));
      setVerification(null);
    } catch (cause) {
      setConnectionStates((current) => ({ ...current, [server.id]: 'failed' }));
      setError(displayError(cause));
      throw cause;
    }
  }

  async function inspect(server: ServerProfile) {
    setConnectionStates((current) => ({ ...current, [server.id]: 'checking' }));
    try {
      const check = await api.inspectHostKey(server.id);
      if (check.decision === 'trusted') {
        await connect(server);
        return;
      }
      setConnectionStates((current) => ({
        ...current,
        [server.id]: check.decision === 'changed' ? 'blocked' : 'saved',
      }));
      setVerification({ server, check });
    } catch (cause) {
      setConnectionStates((current) => ({ ...current, [server.id]: 'failed' }));
      setError(displayError(cause));
    }
  }

  async function createServer(request: CreateServerRequest) {
    setError('');
    const created = await api.createServer(request);
    setServers((current) => [...current, created]);
    setConnectionStates((current) => ({ ...current, [created.id]: 'saved' }));
    setShowAddServer(false);
    await inspect(created);
  }

  async function approveHostKey() {
    if (!verification) return;
    await api.trustHostKey(verification.server.id, verification.check.observed);
    await connect(verification.server);
  }

  return (
    <section className="server-page" aria-labelledby="servers-title">
      <header className="page-heading">
        <div>
          <span className="eyebrow">安全连接中心</span>
          <h1 id="servers-title">服务器</h1>
          <p>保存连接、核验服务器身份，并自动识别 Linux 运行环境。</p>
        </div>
        <button
          className="primary-button"
          type="button"
          onClick={() => setShowAddServer(true)}
        >
          <Plus weight="bold" aria-hidden="true" />
          添加服务器
        </button>
      </header>

      {error && (
        <p className="inline-message inline-message--error page-alert" role="alert">
          {error}
        </p>
      )}

      {loading ? (
        <div className="silver-card loading-card" role="status">
          <SpinnerGap className="spin" weight="bold" aria-hidden="true" />
          正在读取服务器…
        </div>
      ) : servers.length === 0 ? (
        <article className="silver-card empty-state">
          <div className="feature-icon feature-icon--blue" aria-hidden="true">
            <HardDrives weight="duotone" />
          </div>
          <div>
            <h2>还没有服务器</h2>
            <p>添加第一台 Linux 服务器，轻舟会先检查 SSH 主机指纹。</p>
          </div>
          <button
            className="secondary-button"
            type="button"
            onClick={() => setShowAddServer(true)}
          >
            开始添加
            <ArrowRight weight="bold" aria-hidden="true" />
          </button>
        </article>
      ) : (
        <div className="server-grid">
          {servers.map((server) => {
            const state = connectionStates[server.id] ?? 'saved';
            const detected = capabilities[server.id];
            return (
              <article className="silver-card server-card" key={server.id}>
                <header className="server-card__header">
                  <div className="server-identity">
                    <div className="feature-icon feature-icon--purple" aria-hidden="true">
                      <DesktopTower weight="duotone" />
                    </div>
                    <div>
                      <h2>{server.name}</h2>
                      <p>
                        {server.username}@{server.host}:{server.port}
                      </p>
                    </div>
                  </div>
                  <span className={`status-badge status-badge--${state}`}>
                    {state === 'checking' ? (
                      <SpinnerGap className="spin" weight="bold" aria-hidden="true" />
                    ) : state === 'ready' ? (
                      <CheckCircle weight="fill" aria-hidden="true" />
                    ) : state === 'blocked' || state === 'failed' ? (
                      <ShieldWarning weight="fill" aria-hidden="true" />
                    ) : (
                      <Key weight="fill" aria-hidden="true" />
                    )}
                    {connectionLabels[state]}
                  </span>
                </header>

                {detected ? (
                  <div className="capability-panel">
                    <div className="capability-title">
                      <WifiHigh weight="duotone" aria-hidden="true" />
                      <div>
                        <span>已识别系统</span>
                        <strong>
                          {detected.osId}
                          {detected.versionId ? ` ${detected.versionId}` : ''}
                        </strong>
                      </div>
                    </div>
                    <dl className="capability-grid">
                      <div>
                        <dt><DesktopTower aria-hidden="true" />系统家族</dt>
                        <dd>{detected.osFamily}</dd>
                      </div>
                      <div>
                        <dt><Package aria-hidden="true" />包管理器</dt>
                        <dd>{detected.packageManager ?? '未识别'}</dd>
                      </div>
                      <div>
                        <dt><GearSix aria-hidden="true" />服务管理器</dt>
                        <dd>{detected.serviceManager}</dd>
                      </div>
                      <div>
                        <dt><Cpu aria-hidden="true" />架构</dt>
                        <dd>{detected.architecture}</dd>
                      </div>
                      <div>
                        <dt><TerminalWindow aria-hidden="true" />Shell</dt>
                        <dd>{detected.shell}</dd>
                      </div>
                    </dl>
                  </div>
                ) : (
                  <button
                    className="server-card__action"
                    type="button"
                    disabled={state === 'checking' || state === 'blocked'}
                    onClick={() => inspect(server)}
                  >
                    <WifiHigh weight="bold" aria-hidden="true" />
                    检查并连接
                    <ArrowRight weight="bold" aria-hidden="true" />
                  </button>
                )}
              </article>
            );
          })}
        </div>
      )}

      {showAddServer && (
        <AddServerDialog
          onSubmit={createServer}
          onCancel={() => setShowAddServer(false)}
        />
      )}
      {verification && (
        <HostKeyDialog
          check={verification.check}
          onApprove={approveHostKey}
          onContinue={() => connect(verification.server)}
          onClose={() => setVerification(null)}
        />
      )}
    </section>
  );
}
