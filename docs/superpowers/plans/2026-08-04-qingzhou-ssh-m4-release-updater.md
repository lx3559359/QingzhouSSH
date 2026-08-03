# QingzhouSSH M4 发布与双源更新实施计划

> 按 TDD 执行：每项先添加失败测试，再做最小实现、运行相关门禁并提交。所有命令先加载 `scripts/dev-env.ps1`，所有可控文件只写项目目录。

## Task 1：锁定设计与发布契约

- [x] 编写 M4 设计，明确无管理员安装、便携包、Tauri 签名、SHA-256、双源回退、失败清理和用户确认。
- [x] 添加版本一致性和发布配置测试。
- [x] 提交：`docs: design milestone four release and updater`

## Task 2：定义更新领域模型和状态机

- [x] 先写状态转换、版本策略、清单字段和错误分类测试。
- [x] 添加 `domain/update.rs`，拒绝降级、未知状态、非法哈希、大小和平台。
- [x] 提交：`feat: model trusted update lifecycle`

## Task 3：实现清单与固定来源策略

- [x] 先写 GitHub 主源、ModelScope 回退、HTTPS/主机/路径白名单和安全错误不回退测试。
- [x] 添加可注入 manifest transport 与来源选择器。
- [x] 提交：`feat: select trusted dual update sources`

## Task 4：持久化更新设置与清理

- [x] 先写 `state.json` 原子保存、限频、相对路径和遗留 `.partial` 清理测试。
- [x] 实现 `<data-root>/updates` 状态存储，拒绝路径逃逸和绝对路径。
- [x] 提交：`feat: persist project-local update state`

## Task 5：集成 Tauri 签名下载与 SHA-256

- [x] 先用可注入下载器测试签名失败、哈希失败、大小不符、成功原子完成和失败清理。
- [x] 注册 `tauri-plugin-updater`，Rust 侧调用 `Update::download`，在其签名验证后再次核对 SHA-256。
- [x] 待安装对象只保存在 Rust 状态；安装要求显式确认。
- [x] 提交：`feat: verify and stage signed updates`

## Task 6：暴露强类型更新命令

- [x] 添加命令参数、状态冲突、进度单调和错误脱敏测试。
- [x] 实现获取状态、自动检查开关、检查、下载、安装和清理命令并注册。
- [x] 提交：`feat: expose typed updater commands`

## Task 7：扩展前端 API 与 Preview

- [x] 先写 Tauri invoke 名称/参数和 Preview 主源、回退、拒绝、下载/安装状态测试。
- [x] 添加更新 DTO、单调进度通道和纯内存 Preview 更新器。
- [x] 提交：`test: model updater preview flows`

## Task 8：实现立体设置与更新界面

- [x] 先写当前版本、数据目录、自动检查、来源、更新详情和最新状态组件测试。
- [x] 用白银渐变、强阴影和清晰文字替换设置占位页。
- [x] 提交：`feat: add dimensional update settings`

## Task 9：实现下载、确认和错误 UX

- [x] 先写镜像提示、进度、下载失败、签名/哈希拒绝、安装二次确认和清理测试。
- [x] 安装确认明确说明应用将退出；不提供静默自动安装。
- [x] 提交：`feat: control update download and install`

## Task 10：配置无管理员安装与便携包

- [x] 配置 NSIS 当前用户安装、多语言、更新 artifacts 和编译时公钥。
- [x] 添加 `portable.flag`、便携说明和项目内打包脚本测试。
- [x] 提交：`build: package installer and portable release`

## Task 11：生成并验证发布清单

- [x] 先写版本同步、清单结构、签名、哈希、大小、文件集合和源 URL 生成测试。
- [x] 实现 `scripts/build-release.ps1`、`scripts/verify-release.ps1` 和 SHA256SUMS/SBOM。
- [x] 提交：`build: generate reproducible release manifests`

## Task 12：添加双源发布流水线

- [x] GitHub Actions 只构建一次，然后上传 GitHub 与 ModelScope。
- [x] 在一次性 Windows runner 执行安装、启动、更新/卸载和便携冒烟。
- [x] 发布前回读两源并比较版本、构建 ID、签名、SHA-256、大小和文件集合。
- [x] 提交：`ci: publish identical dual-source releases`

## Task 13：补齐公开项目文档

- [x] 添加 Apache-2.0 LICENSE、SECURITY、支持矩阵、用户指南、数据目录、升级/回退与卸载说明。
- [x] README 改为完整产品状态并说明 GitHub/ModelScope 下载入口。
- [x] 提交：`docs: prepare public qingzhou release`

## Task 14：创建 GitHub 与 ModelScope 公共项目

- [x] 创建 `lx3559359/QingzhouSSH` 公共 GitHub 仓库并配置远端。
- [x] 使用用户的 ModelScope 身份创建同名公共项目；SDK 的 HOME/cache 必须位于项目目录。
- [ ] 配置 Tauri 私钥/口令与 ModelScope token 为 GitHub Actions secrets，不在输出或文件中暴露（Tauri secrets 已完成，等待用户提供 ModelScope token）。
- [ ] 提交并推送已本地合并的 `master`，不推送临时工作树分支。

## Task 15：M4 全量验收

- [x] 前端全量测试/构建、Rust fmt/Clippy/全量测试、Tauri NSIS 签名构建全部通过。
- [x] 内置浏览器验收设置页、主源、镜像回退、进度、拒绝和确认，截图保存在项目。
- [x] 运行发布物、密钥 canary、占位、C/D 路径和两源一致性审计。
- [x] 记录命令、测试数、包大小、哈希、项目 URL 和受信任签名边界。
- [ ] 把本计划全部完成项改为 `[x]`。
- [x] 提交：`docs: record milestone four acceptance`
