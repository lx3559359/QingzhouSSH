# Milestone 3 验收记录

验收范围：可视化工作流编辑、不可变版本、图校验、串行运行、条件分支、失败暂停与重试、取消/`uncertain`、恢复点、逆序回滚、诊断和项目内数据约束。

## 功能与安全范围

- 八类节点：开始、快捷任务、自定义命令/脚本、上传、下载、日志检索、条件判断和停止提示。
- 三栏编辑器：白银渐变步骤库、点阵画布/立体节点/SVG 连线、右侧参数与连接检查器；支持节点添加、选择、拖动、缩放、删除和显式 `next`/`true`/`false` 连接。
- 保存采用不可变版本；运行引用指定版本。Rust 在保存和运行前共享同一套图限制、参数和条件校验。
- 运行严格串行并持久化节点 attempt、M2 execution 引用、单调事件和终态；条件未选分支标记 `skipped`。
- 可重试失败进入 `paused`，只重试失败节点；取消无法确认时使用 `uncertain`，重载后从数据库恢复详情。
- 上传覆盖恢复点写入 `backups/workflows`；回滚需要二次确认并按逆序恢复或删除新文件；诊断 JSON 写入 `downloads` 并只返回相对路径。
- 脚本文本不进入浏览器存储、确认摘要、事件或诊断；服务器凭据由 DPAPI/测试保护器加密并经过 canary 扫描。

## 真实联调

执行：

```powershell
.\scripts\test-ssh-live.ps1 -SkipPythonDependencyInstall
```

结果：`ssh_live` 3/3、`sftp_live` 1/1、`m2_live` 1/1、`workflow_live` 1/1 全部通过。工作流 live 用例先用缺失本地上传源注入可重试失败，确认后续节点未运行；补齐文件后同节点第二次 attempt 成功，随后完成下载、日志结果条件真分支和假分支跳过。之后验证覆盖文件恢复点、逆序回滚、恢复点清理、诊断相对路径，以及任务/脚本/日志/上传/下载五类真实失败暂停。

夹具部署目录、既有配置、模拟服务状态、日志、测试密钥、Python 包和所有远端文件都位于项目 `.local`；脚本结束时夹具进程停止且模拟远端根目录清理。

## 自动化门禁

以下命令均从项目工作树执行，并先加载 `scripts/dev-env.ps1` 提供的项目内缓存和构建目录：

| 门禁 | 结果 |
| --- | --- |
| `scripts/tests/dev-env.tests.ps1` | 通过；可控开发路径均位于项目目录 |
| `scripts/verify-d-drive.ps1` | 通过；开发与应用路径位于 D 盘项目目录 |
| `pnpm test` | 17 个测试文件、42 个测试全部通过 |
| `pnpm build` | 生产前端构建通过 |
| `cargo fmt -- --check` | 通过 |
| `cargo clippy --locked --all-targets -- -D warnings` | 通过，零警告 |
| `cargo test --locked --all-targets -- --nocapture` | 84 个非 live 测试通过，9 个 live 测试默认忽略 |
| `scripts/test-ssh-live.ps1 -SkipPythonDependencyInstall` | `ssh_live` 3/3、`sftp_live` 1/1、`m2_live` 1/1、`workflow_live` 1/1 通过 |
| M3 独立 live 用例 | 取消恢复、恢复点捕获、回滚 3 个用例均已在对应任务中通过；因此默认忽略的 9 个 live 测试均有真实执行证据 |
| `pnpm tauri build --debug --no-bundle` | 通过；生成 `target/debug/qingzhou-ssh.exe` |

占位扫描只命中真实 HTML 表单 `placeholder` 和用于拒绝旧里程碑文案的测试断言，没有未实现的 M3 按钮、伪运行逻辑或 TODO。M4 仅作为明确的后续路线图出现。

## 界面人工验收

使用内置浏览器打开 `?preview=ready` 内存模型完成了校验、保存不可变 v1、成功运行、保存失败注入 v2、失败暂停和同节点第二次 attempt 重试成功。页面控制台没有 warning/error，页面宽度为 1280 px 时 `bodyScrollWidth` 为 1265 px，不存在页面级横向溢出。

样式读取和截图确认左侧步骤库、中间画布、右侧检查器使用同一套白银渐变；三者都有 `26px/46px`、`11px/18px`、`3px/6px` 三层阴影，正文颜色保持深色。中央画布保留点阵背景和自身滚动区，右侧卡片下方与页面背景一致。

- [编辑器与条件节点](screenshots/m3-workflow-condition.png)
- [Rust 校验通过](screenshots/m3-workflow-validation.png)
- [失败暂停与可重试提示](screenshots/m3-workflow-paused.png)
- [同节点第 2 次尝试成功](screenshots/m3-workflow-retry.png)
- [成功运行时间线](screenshots/m3-workflow-run-panel.png)

节点拖动/缩放、危险确认、取消、回滚和诊断下载同时由 42 个前端测试及真实 `workflow_live`、取消恢复、恢复点和回滚用例验证。Preview 明确标识且只使用浏览器内存；真实桌面能力由 Tauri API 与 live 测试验证。

## 数据目录审计

- 注册表指针为 `D:\Codex Project\轻量化SSH快捷工具\data`；业务数据库、`vault`、`logs`、`downloads`、`backups`、`templates`、`cache` 和 `updates` 均位于该目录。
- 精确检查 `%LOCALAPPDATA%`、`%APPDATA%`、`D:\` 和 `D:\Codex Project`，不存在 QingzhouSSH 数据目录或根级业务文件。
- 发现的旧 WebView2 缓存 `C:\Users\luojixiang1\AppData\Local\com.qingzhoussh.desktop` 在确认无相关进程后，已迁移到项目数据目录 `cache/webview2-legacy-appdata-20260803`；源目录已不存在，迁移内容保留，可审计且未直接删除。
- live 夹具已停止，模拟远端根目录已清理；开发预览数据只位于工作树 `.local/dev-data`。

## M4 边界

M3 不显示伪在线更新状态。Windows 安装包、签名、更新清单、GitHub Releases 与 ModelScope 双源发布在 M4 单独设计、实现和验收。
