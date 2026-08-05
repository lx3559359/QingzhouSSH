import type { AppErrorDto } from './contracts';

export interface UserFacingError {
  summary: string;
  detail: string | null;
  retryable: boolean;
}

export function describeTaskError(cause: unknown): UserFacingError {
  const error = normalizeAppError(cause);
  const summary = taskSummary(error);
  return {
    summary,
    detail: error.message && error.message !== summary ? error.message : null,
    retryable: error.retryable ?? ['ssh', 'io', 'remote_state_uncertain'].includes(error.code),
  };
}

export function normalizeAppError(error: unknown): AppErrorDto {
  if (
    typeof error === 'object' &&
    error !== null &&
    'code' in error &&
    'message' in error &&
    typeof error.code === 'string' &&
    typeof error.message === 'string'
  ) {
    return {
      code: error.code,
      message: error.message,
      retryable:
        'retryable' in error && typeof error.retryable === 'boolean'
          ? error.retryable
          : undefined,
    };
  }
  if (error instanceof Error) return { code: 'unknown', message: error.message };
  if (typeof error === 'string' && error.trim()) return { code: 'unknown', message: error };
  return { code: 'unknown', message: '操作失败' };
}

function taskSummary(error: AppErrorDto): string {
  switch (error.code) {
    case 'validation':
      return '任务参数不完整或格式不正确，请检查标红的输入项后重试。';
    case 'security':
      return '服务器身份或登录验证未通过，请到“服务器”页面重新测试连接并确认主机指纹与凭据。';
    case 'compatibility':
      return '当前服务器系统不支持此任务，请重新检测系统能力或选择其他任务。';
    case 'permission':
      return '远程账号权限不足，请更换有权限的账号或调整服务器权限后重试。';
    case 'ssh':
    case 'io':
      return '无法连接到目标服务器，请确认服务器在线、SSH 地址和端口正确后重试。';
    case 'ssh_command':
      return '服务器已连接，但远程命令执行失败；请展开技术详情查看退出码和服务器返回信息。';
    case 'output_limit_exceeded':
      return '任务输出超过安全上限，请缩小查询范围或降低结果数量。';
    case 'cancelled':
      return '任务已取消。';
    case 'not_ready':
      return '应用数据目录尚未准备好，请重新启动应用并确认 D 盘项目目录可访问。';
    default:
      return '任务运行失败，请检查服务器连接和任务参数后重试。';
  }
}
