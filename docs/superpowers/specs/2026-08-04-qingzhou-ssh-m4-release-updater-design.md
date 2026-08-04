# QingzhouSSH M4 发布与双源更新设计

## 1. 目标与边界

M4 把 M1–M3 的桌面程序交付为可公开下载的 Windows 产品：提供无需管理员权限的 NSIS 安装包、便携 ZIP、Tauri 签名更新包、GitHub Releases 主源和 ModelScope 国内镜像。客户端可以限频自动检查，也可以手动检查；发现更新后展示版本、说明、发布时间、大小和来源，只有用户明确确认才下载与安装。

M4 不增加遥测、云端账号体系、静默安装、远程配置执行或交互式 SSH 终端。Windows Authenticode 证书属于发布者身份基础设施；没有受信任证书时不能把 Tauri 更新签名描述成 SmartScreen/Authenticode 签名。

## 2. 发布物与版本契约

版本在 `package.json`、`src-tauri/Cargo.toml` 和 `src-tauri/tauri.conf.json` 保持一致，并使用 SemVer。每次正式发布只允许从一个已签名 Git 标签构建一次，生成：

- 当前用户 NSIS 安装包及 Tauri `.sig`；
- 包含可执行文件、`portable.flag`、许可证和便携说明的 ZIP；
- `latest.json`：Tauri 静态更新清单，包含版本、说明、RFC 3339 时间、平台 URL、Tauri 签名、SHA-256、字节数和构建标识；
- `SHA256SUMS` 与机器可读 `release-index.json`；
- SBOM、许可证和发布说明。

GitHub 与 ModelScope 的清单 URL 可以不同，但版本、构建标识、安装包字节、Tauri 签名、SHA-256 和大小必须相同。发布流水线在公开 Release 前下载回读两个源；任一缺失或不一致都失败，不允许只发布一侧。

## 3. 安装与便携模式

NSIS 使用默认 `perUser` 安装模式，不请求管理员权限。开发机只构建安装包，不在本机执行安装，避免把程序写入 C 盘 AppData；安装/卸载冒烟测试在一次性 Windows CI 运行器执行。

便携包中的 `portable.flag` 使数据根固定为可执行文件旁的 `data`。安装版首次启动仍要求用户选择数据根，不回退 AppData。卸载默认不删除用户选择的数据根；文档明确说明手工备份和删除方式。

## 4. 更新信任模型

Tauri Minisign 公钥编译进应用，私钥不得进入 Git、日志或发布物。开发机私钥保存在项目 `data/release-signing`，密钥自身有口令，口令用当前 Windows 用户 DPAPI 加密；CI 只从 GitHub Actions secrets 注入。

更新检查必须同时满足：

1. 端点和安装包均为 HTTPS，主机和路径符合编译时白名单；
2. 清单是受限 JSON、版本高于当前版本、平台为 `windows-x86_64`；
3. SHA-256 是 64 位小写十六进制，大小在配置上限内；
4. Tauri 下载先验证内置 Minisign 公钥签名；
5. 对已验证字节再次计算 SHA-256 和大小；
6. 下载只以 `.partial` 写入 `<data-root>/updates`，全部通过后原子改名；
7. 安装前再次核对内存中的待安装版本、来源和哈希，并要求显式确认。

任一步失败都删除临时文件并保留当前版本。应用不允许降级，不允许 HTTP、自定义证书跳过、运行时替换公钥或用户输入任意更新 URL。

## 5. 双源选择

检查顺序固定为 GitHub、ModelScope。GitHub 网络失败、超时、404/5xx 或无可用清单时才尝试 ModelScope。GitHub 返回格式错误、路径越界、哈希/签名冲突等安全错误时停止，不用备用源掩盖攻击或发布错误。

若主源正常且无更新，结果即为“已是最新”，不再用镜像覆盖版本判断。使用镜像时界面显示“已切换国内镜像”和主源失败摘要。自动检查失败只显示非阻塞状态；手动检查返回可操作错误。

## 6. Rust 更新状态机

状态固定为 `idle → checking → available → downloading → downloaded → installing`，并支持 `up_to_date`、`failed`。Rust 持有唯一待安装对象和下载字节；WebView 只接收脱敏元数据和单调进度事件，不接收安装包 URL、签名原文、绝对路径或原始网络错误。

命令为：

- `get_update_status`：当前版本、自动检查设置、上次检查和状态；
- `set_auto_update_check`：只控制检查，不控制下载/安装；
- `check_for_update`：手动或限频自动检查；
- `download_update`：签名和 SHA-256 双验证，发送进度；
- `install_update`：要求 `confirmed=true`，在 Windows 安装前由 Tauri 退出程序；
- `clear_downloaded_update`：清理已下载但未安装的登记文件。

`updates/state.json` 使用临时文件加原子替换，保存上次检查时间、是否自动检查、最后结果和相对文件名；不保存 URL、签名或 secret。遗留 `.partial` 在启动时清理。

## 7. 设置界面

“设置”页面继续使用白银渐变和立体阴影，展示当前版本、数据目录、自动检查开关、两个固定来源的状态、上次检查时间和更新说明。下载前显示大小、来源与风险提示；安装按钮必须弹出二次确认，说明应用将退出并由安装器接管。错误按网络、发布一致性、签名、哈希、空间和安装失败分类。

开发浏览器 Preview 使用内存更新器，能演示主源成功、主源失败切镜像、签名/哈希拒绝、下载进度和安装确认，但绝不写磁盘或启动安装器。

## 8. 发布自动化与平台项目

GitHub 公共仓库名为 `QingzhouSSH`，保存源码、Issue、Actions 和正式 Releases。ModelScope 使用同名公开模型仓库保存国内发布物镜像，因为该仓库类型提供客户端所需的公开单文件下载 API；同名 Studio 保留为项目展示页，不进入更新信任链。ModelScope 使用官方 `modelscope-hub`/`ms-hub` API 创建和上传，所有缓存与配置通过 `MODELSCOPE_HOME`、`MODELSCOPE_CACHE` 指向项目目录。

GitHub Actions 在 Windows 运行器上执行全量门禁、签名构建、安装/启动/卸载冒烟、便携启动和发布物生成；随后把同一目录上传两源并做下载回读。ModelScope token 和 Tauri 私钥只存在于 Actions secrets。

## 9. 验收门槛

- Manifest、URL 白名单、SemVer、状态机、限频、回退、安全错误不回退、签名/哈希拒绝和原子清理有 Rust 测试；
- 设置页、Preview 双源回退、进度、确认和错误有前端测试与浏览器截图；
- NSIS、便携 ZIP、签名、SHA256SUMS、SBOM 均在项目 `artifacts` 生成；
- GitHub/ModelScope 项目可公开访问，同一发布物回读哈希一致；
- CI 在干净 Windows 用户环境完成安装、启动、更新替换验证和卸载，应用数据仍由用户选择并与安装目录分离；
- C 盘 AppData、`D:\` 和 `D:\Codex Project` 根级污染扫描通过。

