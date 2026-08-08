import { describe, expect, it, vi } from 'vitest';

import type { DirectoryListing } from '../../api/contracts';
import { DirectorySessionCache } from './directorySessionCache';

function listing(path: string): DirectoryListing {
  return {
    path,
    parent: path === '/' ? null : '/',
    entries: [],
  };
}

describe('DirectorySessionCache', () => {
  it('uses only fresh entries for cached loads', async () => {
    let now = 1_000;
    const clock = vi.fn(() => now);
    const cache = new DirectorySessionCache(128, clock, 5_000);
    const loader = vi
      .fn<() => Promise<DirectoryListing>>()
      .mockResolvedValueOnce(listing('/fresh'))
      .mockResolvedValueOnce(listing('/refreshed'));

    await expect(cache.loadRemote('server-1', '/', loader)).resolves.toEqual(listing('/fresh'));
    now = 5_999;
    expect(cache.freshRemote('server-1', '/')).toEqual(listing('/fresh'));
    await cache.loadRemote('server-1', '/', loader);
    expect(loader).toHaveBeenCalledTimes(1);

    now = 6_000;
    expect(cache.freshRemote('server-1', '/')).toBeNull();
    expect(cache.peekRemote('server-1', '/')).toEqual(listing('/fresh'));
    await expect(cache.loadRemote('server-1', '/', loader)).resolves.toEqual(listing('/refreshed'));
    expect(loader).toHaveBeenCalledTimes(2);
  });

  it('shows stale data while deduplicating its background refresh', async () => {
    let now = 1_000;
    const cache = new DirectorySessionCache(128, () => now, 5_000);
    await cache.loadRemote('server-1', '/', async () => listing('/old'));
    now = 6_000;
    let resolve!: (value: DirectoryListing) => void;
    const loader = vi.fn(() => new Promise<DirectoryListing>((done) => { resolve = done; }));

    expect(cache.peekRemote('server-1', '/')).toEqual(listing('/old'));
    const first = cache.refreshRemote('server-1', '/', loader);
    const second = cache.refreshRemote('server-1', '/', loader);
    expect(first).toBe(second);
    expect(loader).toHaveBeenCalledTimes(1);

    resolve(listing('/new'));
    await expect(first).resolves.toEqual(listing('/new'));
    expect(cache.peekRemote('server-1', '/')).toEqual(listing('/new'));
  });

  it('reuses one in-flight request and then serves the cached remote listing', async () => {
    const cache = new DirectorySessionCache(128);
    let resolve!: (value: DirectoryListing) => void;
    const loader = vi.fn(() => new Promise<DirectoryListing>((done) => { resolve = done; }));

    const first = cache.loadRemote('server-1', '/home', loader);
    const second = cache.loadRemote('server-1', '/home', loader);
    expect(loader).toHaveBeenCalledTimes(1);

    resolve(listing('/home'));
    await expect(Promise.all([first, second])).resolves.toEqual([
      listing('/home'),
      listing('/home'),
    ]);

    await expect(cache.loadRemote('server-1', '/home', loader)).resolves.toEqual(listing('/home'));
    expect(loader).toHaveBeenCalledTimes(1);
  });

  it('forces refresh, invalidates one directory, and isolates servers', async () => {
    const cache = new DirectorySessionCache(128);
    const loader = vi.fn(async () => listing('/home'));

    await cache.loadRemote('server-1', '/home', loader);
    await cache.loadRemote('server-2', '/home', loader);
    await cache.refreshRemote('server-1', '/home', loader);
    cache.invalidateRemote('server-1', '/home');
    await cache.loadRemote('server-1', '/home', loader);

    expect(loader).toHaveBeenCalledTimes(4);
    expect(cache.peekRemote('server-2', '/home')).toEqual(listing('/home'));
  });

  it('remembers paths per server and evicts the least recently used entry', async () => {
    const cache = new DirectorySessionCache(2);

    cache.rememberRemotePath('server-1', '/home');
    cache.rememberRemotePath('server-2', '/srv');
    await cache.loadRemote('server-1', '/', async () => listing('/'));
    await cache.loadRemote('server-1', '/home', async () => listing('/home'));
    expect(cache.peekRemote('server-1', '/')).toEqual(listing('/'));
    await cache.loadRemote('server-1', '/var', async () => listing('/var'));

    expect(cache.lastRemotePath('server-1')).toBe('/home');
    expect(cache.lastRemotePath('server-2')).toBe('/srv');
    expect(cache.lastRemotePath('server-3')).toBe('/');
    expect(cache.peekRemote('server-1', '/home')).toBeNull();
    expect(cache.peekRemote('server-1', '/')).toEqual(listing('/'));
    expect(cache.peekRemote('server-1', '/var')).toEqual(listing('/var'));
  });

  it('keeps local paths separate from remote paths', async () => {
    const cache = new DirectorySessionCache(128);
    const local = listing('D:\\project');

    await cache.loadLocal('D:\\project', async () => local);
    cache.invalidateLocal('D:\\project');

    expect(cache.peekLocal('D:\\project')).toBeNull();
    expect(cache.peekRemote('server-1', 'D:\\project')).toBeNull();
  });

  it('can clear one server or the whole session without touching unrelated entries', async () => {
    const cache = new DirectorySessionCache(128);
    await cache.loadRemote('server-1', '/home', async () => listing('/home'));
    await cache.loadRemote('server-2', '/srv', async () => listing('/srv'));
    await cache.loadLocal('D:\\project', async () => listing('D:\\project'));

    cache.clearServer('server-1');
    expect(cache.peekRemote('server-1', '/home')).toBeNull();
    expect(cache.peekRemote('server-2', '/srv')).toEqual(listing('/srv'));
    expect(cache.peekLocal('D:\\project')).toEqual(listing('D:\\project'));

    cache.clear();
    expect(cache.peekRemote('server-2', '/srv')).toBeNull();
    expect(cache.peekLocal('D:\\project')).toBeNull();
  });

  it('does not resurrect a server listing when an obsolete request finishes after clear', async () => {
    const cache = new DirectorySessionCache(128);
    let resolve!: (value: DirectoryListing) => void;
    const pending = cache.loadRemote(
      'server-1',
      '/slow',
      () => new Promise<DirectoryListing>((done) => { resolve = done; }),
    );
    cache.rememberRemotePath('server-1', '/slow');

    cache.clearServer('server-1');
    resolve(listing('/slow'));
    await pending;

    expect(cache.peekRemote('server-1', '/slow')).toBeNull();
    expect(cache.lastRemotePath('server-1')).toBe('/');
  });
});
