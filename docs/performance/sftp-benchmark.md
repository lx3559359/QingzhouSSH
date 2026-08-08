# SFTP 性能基准

此基准使用项目内 SSH/SFTP 夹具执行一次预热和至少三次正式测量。每个样本都会完成上传、目录读取、均衡校验下载、严格校验兜底下载和取消响应检查；任何正确性检查失败都会让命令返回非零。

## 本地基线

先验证脚本契约：

```powershell
pnpm run test:sftp-performance-contract
```

再记录 16 MiB 本地回环基线：

```powershell
pnpm run benchmark:sftp -- -PayloadBytes 16777216 -Iterations 3 -OutputPath artifacts/benchmarks/local-sftp.json
```

比较改动后的结果时使用相同参数和不同输出文件：

```powershell
pnpm run benchmark:sftp -- -PayloadBytes 16777216 -Iterations 3 -OutputPath artifacts/benchmarks/candidate-sftp.json
```

JSON 报告包含版本、提交、平台、架构、请求的 RTT/带宽、逐次样本、目录延迟中位数、上传/下载吞吐中位数、校验策略、CPU、内存峰值、进度事件数和取消延迟。输出路径被限制在 `artifacts/benchmarks`，不会写入用户配置目录或磁盘根目录。

## 网络整形证据

Windows 和当前项目内直连夹具没有安全的进程级流量整形器，因此本地报告会明确写入 `networkShape: "unavailable"`。这类结果只用于诊断回归，不能宣称达到特定 RTT/带宽下的设计阈值。

正式阈值必须由 Linux CI 中具备隔离网络命名空间和 `tc netem` 的夹具提供。CI 应固定 `-RttMs`、`-BandwidthMbps`、负载大小和迭代次数，并保存 JSON 工件；没有 `networkShape` 证据的运行不得作为跨版本性能结论。

## 判读规则

- `samples[*].success` 必须全部为 `true`。
- `pipelineMaxInFlight` 不得超过 8。
- `pipelineMaxBufferedBytes` 不得超过 16 MiB。
- 吞吐比较使用中位数，不删除失败样本；失败样本会保留在报告中并使命令失败。
- 进度事件由后端以 100 ms 间隔限频，完成与阶段切换事件不受限频影响。
