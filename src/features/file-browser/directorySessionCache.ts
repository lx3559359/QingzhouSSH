import type { DirectoryListing } from '../../api/contracts';

type DirectoryLoader = () => Promise<DirectoryListing>;

const REMOTE_PREFIX = 'remote\u0000';
const LOCAL_PREFIX = 'local\u0000';

export class DirectorySessionCache {
  private readonly listings = new Map<string, DirectoryListing>();
  private readonly inFlight = new Map<string, Promise<DirectoryListing>>();
  private readonly remotePaths = new Map<string, string>();

  constructor(private readonly maximumEntries = 128) {
    if (!Number.isInteger(maximumEntries) || maximumEntries < 1) {
      throw new Error('目录缓存容量必须大于 0');
    }
  }

  peekRemote(serverId: string, path: string) {
    return this.peek(this.remoteKey(serverId, path));
  }

  peekLocal(path: string | null) {
    return this.peek(this.localKey(path));
  }

  loadRemote(serverId: string, path: string, loader: DirectoryLoader) {
    return this.load(this.remoteKey(serverId, path), loader, false);
  }

  refreshRemote(serverId: string, path: string, loader: DirectoryLoader) {
    return this.load(this.remoteKey(serverId, path), loader, true);
  }

  loadLocal(path: string | null, loader: DirectoryLoader) {
    return this.load(this.localKey(path), loader, false);
  }

  refreshLocal(path: string | null, loader: DirectoryLoader) {
    return this.load(this.localKey(path), loader, true);
  }

  invalidateRemote(serverId: string, path: string) {
    this.listings.delete(this.remoteKey(serverId, path));
  }

  invalidateLocal(path: string | null) {
    this.listings.delete(this.localKey(path));
  }

  rememberRemotePath(serverId: string, path: string) {
    this.remotePaths.set(serverId, path);
  }

  lastRemotePath(serverId: string) {
    return this.remotePaths.get(serverId) ?? '/';
  }

  clearServer(serverId: string) {
    const prefix = `${REMOTE_PREFIX}${serverId}\u0000`;
    for (const key of this.listings.keys()) {
      if (key.startsWith(prefix)) this.listings.delete(key);
    }
    for (const key of this.inFlight.keys()) {
      if (key.startsWith(prefix)) this.inFlight.delete(key);
    }
    this.remotePaths.delete(serverId);
  }

  clear() {
    this.listings.clear();
    this.inFlight.clear();
    this.remotePaths.clear();
  }

  private remoteKey(serverId: string, path: string) {
    return `${REMOTE_PREFIX}${serverId}\u0000${path}`;
  }

  private localKey(path: string | null) {
    return `${LOCAL_PREFIX}${path ?? '<data-root>'}`;
  }

  private peek(key: string) {
    const listing = this.listings.get(key) ?? null;
    if (listing) {
      this.listings.delete(key);
      this.listings.set(key, listing);
    }
    return listing;
  }

  private load(key: string, loader: DirectoryLoader, force: boolean) {
    if (!force) {
      const cached = this.peek(key);
      if (cached) return Promise.resolve(cached);
    }

    const pending = this.inFlight.get(key);
    if (pending) return pending;

    const request = loader()
      .then((listing) => {
        this.listings.delete(key);
        this.listings.set(key, listing);
        this.evictOverflow();
        return listing;
      })
      .finally(() => {
        if (this.inFlight.get(key) === request) this.inFlight.delete(key);
      });
    this.inFlight.set(key, request);
    return request;
  }

  private evictOverflow() {
    while (this.listings.size > this.maximumEntries) {
      const oldest = this.listings.keys().next().value;
      if (typeof oldest !== 'string') return;
      this.listings.delete(oldest);
    }
  }
}

export const directorySessionCache = new DirectorySessionCache();
