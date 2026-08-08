import { describe, expect, it } from 'vitest';

import { analyzeScript } from './scriptAnalysis';

describe('analyzeScript', () => {
  it('uses shell-specific rules and preserves line numbers', () => {
    expect(analyzeScript('posix_sh', 'echo ok\nsudo reboot')).toEqual([
      expect.objectContaining({ code: 'interactive_sudo', lineNumber: 2 }),
    ]);
    expect(analyzeScript('powershell', "Write-Output ok\nRead-Host 'name'")).toEqual([
      expect.objectContaining({ code: 'interactive_input', lineNumber: 2 }),
    ]);
  });

  it('does not apply PowerShell syntax rules to POSIX scripts', () => {
    expect(analyzeScript('posix_sh', "Read-Host 'name'"))
      .toEqual([]);
  });
});
