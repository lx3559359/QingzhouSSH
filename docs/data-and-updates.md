# 数据、更新、回退与卸载

## Data root（数据根目录）

运行时解析顺序固定为：

1. `QINGZHOU_DATA_ROOT` 环境变量（开发、测试和受控部署）；
2. 可执行文件旁存在 `portable.flag` 时，优先读取同目录 `data-root.json` 指向的自定义目录，否则使用同目录 `data`；
3. `HKCU\Software\QingzhouSSH\DataRoot` 保存的路径指针；
4. 第一次启动由用户明确选择。

注册表只保存路径指针。应用不会回退 `%APPDATA%` 或 `%LOCALAPPDATA%` 保存数据库、凭据、日志、下载、备份、更新或 WebView2 持久缓存。

主要目录如下：

| 路径 | 内容 |
| --- | --- |
| `<data-root>\app.db` | 服务器、任务、历史、工作流和状态元数据 |
| `<data-root>\vault` | 当前 Windows 用户 DPAPI 加密的凭据 |
| `<data-root>\downloads` | 日志、SFTP 下载和诊断包 |
| `<data-root>\backups` | 工作流恢复点 |
| `<data-root>\cache\webview2` | WebView2 持久数据 |
| `<data-root>\updates` | 更新状态、临时下载和已校验待安装包 |

“设置与更新”中的“更改数据目录”会先检查目标路径、写入权限、可用空间和目录关系；用户确认后客户端退出，由独立工作器复制全部声明数据、逐文件验证 SHA-256，再原子切换路径指针并重新打开客户端。旧目录始终保留，不会自动删除。失败时继续使用原目录，并可从设置页安全补传重试。不要在迁移过程中移动源目录或目标目录。

`QINGZHOU_DATA_ROOT` 属于受控部署覆盖项，启用后客户端会明确显示“环境变量锁定”，不能从界面修改。便携版改为自定义目录后，可在设置页恢复到程序旁的 `data`；若该位置已有旧数据，为避免覆盖，需先把旧文件夹改名保留。

跨 Windows 用户或跨电脑复制时，DPAPI 密文通常无法解密，服务器凭据必须重新录入。

## 安装版与便携版

NSIS 安装器使用 `currentUser`，只安装到当前 Windows 用户，不请求管理员权限。应用业务数据与安装目录分离，卸载程序不会默认删除用户选择的数据根。

便携版必须完整解压，`QingzhouSSH.exe` 与空文件 `portable.flag` 保持同级。所有业务数据和 WebView2 缓存进入旁边的 `data`。升级便携版时先退出程序，备份旧 `data`，再替换程序文件；不要用新 ZIP 覆盖正在运行的目录。

## Online update（在线更新）

- GitHub Releases 为主源；[`lx3559359/QingzhouSSH`](https://modelscope.cn/models/lx3559359/QingzhouSSH) ModelScope 模型仓库为国内发布镜像，提供公开单文件下载 API；同名 [Studio](https://modelscope.cn/studios/lx3559359/QingzhouSSH) 仅作为项目展示页。
- 只有网络失败、404 或服务端故障允许从 GitHub 回退 ModelScope。无效 JSON、陌生字段、越权 URL 或其他安全错误会直接阻断。
- 两源发布同一更新包字节、签名、SHA-256、大小和构建 ID；清单 URL 因源不同而不同。
- **Tauri updater signature** 使用编译时公钥验证发布者；随后再次验证 SHA-256 和字节数。任一项不一致都会清除临时文件并拒绝安装。
- 自动检查只检查元数据。下载和安装都需要用户操作；安装前明确提示应用将退出，不提供静默安装。

发布私钥不得进入源码、发布 ZIP 或日志。本地维护密钥只允许放在 `D:\Codex Project\轻量化SSH快捷工具\data\release-signing`（已被 Git 忽略）；CI 使用 GitHub Actions secrets。Tauri 更新签名与 Authenticode 独立，没有 Authenticode 证书时 Windows 仍可能显示未知发布者。

## Rollback（回退）

更新器只接受更高 SemVer，不提供静默降级。需要回退时：

1. 退出应用并完整备份数据根；
2. 从官方 GitHub/ModelScope 获取目标旧版本，核对 `SHA256SUMS` 和发布说明；
3. 安装版先正常卸载当前程序（保留数据），再安装旧版；便携版在备份后替换程序目录并保留原 `data`；
4. 启动前确认旧版是否支持当前数据库结构。若发布说明标明不可逆迁移，应恢复升级前的整目录备份，而不是让旧版直接打开新数据库。

不要通过修改 `latest.json`、关闭签名校验或替换编译时公钥来强制回退。

## Uninstall（卸载）

安装版从 Windows 当前用户的“已安装的应用”运行卸载器，不需要管理员权限。卸载后用户选择的数据根仍然保留；确认备份无误后，用户可手工删除那个**明确路径**。工具不会递归删除盘符根目录、项目父目录或无法确认的路径。

便携版卸载时先退出应用，备份需要的数据，再删除整个解压目录。若把 `data` 放在程序旁，删除目录同时会删除全部业务数据。

## 本仓库的 D 盘约束

当前开发工作区为 `D:\Codex Project\轻量化SSH快捷工具`。所有可控依赖、缓存、临时文件、测试夹具、签名维护文件和发布物必须位于该目录内的 `.local`、`target`、`dist`、`data` 或 `artifacts`，不得写入 `D:\` 根目录、`D:\Codex Project` 父目录或 C 盘 AppData。`scripts\verify-d-drive.ps1` 用于持续审计这些路径。
