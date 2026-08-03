# M4 发布与在线更新验收记录

验收日期：2026-08-04

## 结论

M4 的产品代码、签名构建、发布物生成、双源清单、浏览器交互和本地路径约束均已通过验收。GitHub 与 ModelScope 公共项目已经创建；GitHub 中的 Tauri 签名密钥已配置。首次双源发布前仍需由项目所有者在 GitHub Actions 中补充 `MODELSCOPE_API_TOKEN`，因此本次未创建版本标签，也未对外发布 v0.1.0。

本机不执行 NSIS 安装/卸载冒烟测试，因为 `currentUser` 安装会写入当前 Windows 用户的 LocalAppData，与本项目“不占用 C 盘”的开发机约束冲突。相同的安装、启动、更新替换、卸载流程已固化在一次性 Windows GitHub Actions 运行器中；本地完成真实签名构建、密码学验签和便携包验证。

## 自动化门禁

- `pnpm test`：18 个测试文件、54 项测试全部通过。
- `pnpm build`：TypeScript 与 Vite 生产构建通过；主 JS 产物 450.73 kB，gzip 125.42 kB。
- 六组 PowerShell 契约测试全部通过：开发路径、安装/便携包、公开文档、发布物、版本配置、双源发布管线。
- `cargo fmt -- --check`：通过。
- `cargo clippy --all-targets --all-features -- -D warnings`：零警告通过。
- `cargo test --all-targets`：103 项通过、0 项失败、9 项需要本地 SSH/SFTP 夹具的测试按设计忽略。
- 9 项 SSH/SFTP/工作流真实夹具测试已在本轮 M4 验收前全部单独运行通过，包括密码与密钥认证、指纹拒绝、日志/SFTP、取消、恢复点和回滚。
- `pnpm tauri build --bundles nsis`：真实 Tauri/NSIS 签名构建通过；NSIS 工具缓存位于项目 `target/.tauri`。
- `scripts/verify-release.ps1`：真实发布目录通过文件集合、大小、SHA-256、SBOM、双源 URL、清单一致性和 Minisign/Ed25519 内容验签。
- `scripts/verify-d-drive.ps1`：通过；可控缓存、临时文件、构建目录、测试数据与发布物均位于项目目录。

## 浏览器验收

开发预览的更新场景 URL 会自动选择纯内存 Preview API；生产构建不会启用该路径。内置浏览器实测以下状态：

- [GitHub 主源发现更新](screenshots/m4-update-github.png)
- [GitHub 不可用后回退 ModelScope](screenshots/m4-update-modelscope.png)
- [签名验证失败并拒绝清理](screenshots/m4-update-signature-rejected.png)
- [当前已是最新版本](screenshots/m4-update-up-to-date.png)

界面保持白银渐变、强阴影和高对比文字；来源、版本、构建 ID、大小、回退原因和安全错误均可见。成功下载的单调进度、下载完成状态、安装二次确认、取消确认和清理更新文件由 `SettingsPage` 组件自动化测试覆盖；验收未触发真实安装。

## 真实发布物

发布目录：`artifacts/release/v0.1.0`

构建标识：`build-0.1.0-8e1cfdd02ca3`

| 文件 | 字节 | SHA-256 |
| --- | ---: | --- |
| `轻舟 SSH_0.1.0_x64-setup.exe` | 5,971,451 | `8e1cfdd02ca32d0fc72f2d254fd4e7af23a9011718b8c1b2575df20b65c77a56` |
| `轻舟 SSH_0.1.0_x64-setup.exe.sig` | 420 | `7e2d2dca4c0114cc9b92911d2a1474cab0473e9dc841b269716b0842cea039a2` |
| `QingzhouSSH-v0.1.0-windows-x86_64-portable.zip` | 8,273,096 | `f6949def1a15bc6d174c32f284fcb653dc96abac4608b9c062495d9af6bda88a` |
| `release-metadata.json` | 1,384 | `34e220f8622e5c50d5b27042fd23390d59a6f4d897427778cc3a8029573e39c3` |
| `SHA256SUMS` | 654 | `fd1aca4f6c2543b729ddadf302c57fbf8edf70b89be0c18bb7ac3670a1e8fec3` |
| `SBOM.spdx.json` | 616,304 | `ebea49df3ec0908b3d97fb59226622b919aa35d28ace4fc9a5a63fee20bc0a3d` |
| `github/latest.json` | 1,199 | `25f02f87140e38b946bee74f8f88962a9df3ce1bdc869b390167b259dd96c01f` |
| `modelscope/latest.json` | 1,247 | `161f7be62b16c82dc8813e60200d3d36cdacd4738b83a681c85672ff55271075` |

Tauri v2 对 NSIS 安装器本身签名，更新载荷契约是同一个 `setup.exe` 与其 `.exe.sig`，不是额外的 `.nsis.zip`。GitHub 与 ModelScope 清单引用同一安装器字节、签名、SHA-256、大小和构建标识。

## 信任与数据边界

- 更新只接受固定的 `lx3559359/QingzhouSSH` GitHub Release 路径和固定的 ModelScope Studio 文件路径。
- 仅网络不可用、404 或服务端错误允许从 GitHub 回退 ModelScope；签名、哈希、版本、平台或清单安全错误禁止回退。
- 下载先由 Tauri Updater 验证签名，再核对 SHA-256 和大小；失败的 `.partial` 或暂存文件会被清理。
- 安装必须由用户明确确认，不进行静默下载或静默安装。
- 本地维护私钥、DPAPI 加密口令和公钥文件位于项目 `data/release-signing`，整个 `data/` 已被 Git 忽略；Git 跟踪文件中不存在私钥或令牌值。
- D 盘根目录未发现项目生成的 `.local`、`artifacts`、`data`、`target`、Qingzhou 或 release 目录。

## 公共项目与外部配置

- GitHub：<https://github.com/lx3559359/QingzhouSSH>（Public）
- ModelScope：<https://modelscope.cn/studios/lx3559359/QingzhouSSH>（Public，Apache-2.0）
- 本地 `feature/full-product` 已快进合并到 `master`，且仅推送合并后的 `master`；临时功能分支未推送。
- GitHub Actions secrets 已配置：`TAURI_SIGNING_PRIVATE_KEY`、`TAURI_SIGNING_PRIVATE_KEY_PASSWORD`。
- GitHub Actions variable 已配置：`QINGZHOU_MODELSCOPE_NAMESPACE=lx3559359`。
- 待项目所有者配置：`MODELSCOPE_API_TOKEN`。令牌不得发送到聊天或写入项目文件，应直接通过 `gh secret set MODELSCOPE_API_TOKEN --repo lx3559359/QingzhouSSH` 的安全输入提示录入。
