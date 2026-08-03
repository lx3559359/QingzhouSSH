# Security Policy

## 支持版本

安全修复面向 GitHub Releases 与 ModelScope 同时发布的最新公开版本。预览构建、用户自行修改的构建和已经被新版本替代的旧版本不承诺单独维护；报告中仍可注明受影响的最早版本。

## 私下报告漏洞

优先使用仓库的 [GitHub Security Advisory](https://github.com/lx3559359/QingzhouSSH/security/advisories/new) 提交私密报告。仓库尚未开放该入口时，请创建不含技术细节的普通 Issue，请求维护者提供私密联系渠道。

**Do not** 在公开 Issue、讨论、截图或日志中提交以下内容：

- 密码、私钥、私钥口令、访问令牌或真实服务器地址；
- 可直接利用的主机指纹绕过、命令注入、路径逃逸或更新签名绕过细节；
- 包含用户数据根绝对路径、数据库、保险库或诊断包的原始附件。

报告建议包含版本、Windows 版本、远端 Linux 类型、最小复现步骤、预期/实际行为、影响范围，以及经过脱敏的日志。维护者会先确认收到，再根据可复现性、影响和发布风险协调修复与披露时间。

## 需要重点报告的边界

- **Host key**：首次信任绕过、指纹变化后仍发送凭据，或错误服务器被静默信任。
- **Credential protection**：DPAPI 密文之外出现明文密码、私钥或口令，或敏感内容进入日志/诊断包。
- **Command boundary**：绕过二次确认、获得 PTY/持续 stdin，或在未授权目标执行任务。
- **Path boundary**：下载、备份、更新或缓存逃出用户选择的数据根目录。
- **Updater signature**：未通过编译时公钥验证仍安装，清单 URL 越过固定仓库，或 GitHub 安全错误错误地回退 ModelScope。

Tauri **Updater signature**（Minisign 体系）用于证明更新包由项目发布密钥签发；SHA-256 用于确认下载字节与清单一致。它们与 Windows **Authenticode** 代码签名是两条独立信任链。没有 Authenticode 证书时，SmartScreen 可能显示未知发布者，但更新器仍必须通过 Tauri 签名才能安装。

发现发布私钥、GitHub Actions secret 或 ModelScope token 泄漏时，应立即停止发布、轮换对应凭据、撤销受影响发布，并用新密钥发布需要用户明确确认的迁移版本。
