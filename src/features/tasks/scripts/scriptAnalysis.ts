import type { ScriptScanWarning, ScriptShell } from '../../../api/contracts';

const sharedRules: Array<[string, RegExp, string]> = [
  ['embedded_secret', /(?:password|token|api_key|secret)\s*=\s*['"][^$'"]+['"]/i, '疑似包含明文凭据，请改为敏感运行参数'],
];

const posixRules: Array<[string, RegExp, string]> = [
  ['recursive_delete', /\brm\s+-(?:rf|fr)\b/i, '检测到递归强制删除，请核对目标路径'],
  ['interactive_sudo', /\bsudo\s+(?!-n\b)/i, 'sudo 未使用 -n，执行时可能等待密码并最终超时'],
  ['interactive_input', /\bread\s+-p\b|\btail\s+-f\b/i, '检测到需要持续输入或前台等待的命令'],
];

const powershellRules: Array<[string, RegExp, string]> = [
  ['recursive_delete', /remove-item\b(?=.*-recurse)(?=.*-force)/i, '检测到 PowerShell 递归强制删除，请核对目标路径'],
  ['disk_write', /\b(?:format-volume|clear-disk)\b/i, '检测到磁盘格式化或清理操作'],
  ['dynamic_execution', /\b(?:invoke-expression|iex)\b/i, '动态代码执行难以静态审查'],
  ['interactive_input', /\bread-host\b/i, 'Read-Host 需要交互输入，非交互执行会失败'],
];

export function analyzeScript(shell: ScriptShell, body: string): ScriptScanWarning[] {
  const rules = [...sharedRules, ...(shell === 'powershell' ? powershellRules : posixRules)];
  const warnings: ScriptScanWarning[] = [];
  body.split(/\r?\n/).forEach((line, index) => {
    for (const [code, pattern, message] of rules) {
      if (pattern.test(line) && !warnings.some((warning) => warning.code === code)) {
        warnings.push({ code, message, lineNumber: index + 1 });
      }
    }
  });
  return warnings.slice(0, 64);
}
