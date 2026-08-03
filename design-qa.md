# 轻舟 SSH M1 界面设计 QA

## 比较目标与证据

- source visual truth path: `C:\Users\LUOJIX~1\AppData\Local\Temp\codex-clipboard-0ab62c04-23fe-4433-a802-e2d8a2e20649.png`
- implementation URL: `http://127.0.0.1:1420/?preview=ready`
- implementation screenshot path: `D:\Codex Project\轻量化SSH快捷工具\.worktrees\m1-secure-foundation\docs\design-qa\task9-server-page.png`
- supporting screenshots:
  - `D:\Codex Project\轻量化SSH快捷工具\.worktrees\m1-secure-foundation\docs\design-qa\task9-data-root.png`
  - `D:\Codex Project\轻量化SSH快捷工具\.worktrees\m1-secure-foundation\docs\design-qa\task9-add-server-dialog.png`
- source pixels: `991 × 689`
- implementation pixels: `1265 × 712`
- implementation CSS viewport: `1280 × 720`
- density normalization: 浏览器截图为 1× CSS 密度；源图与实现图按完整内容区并列查看，不做像素级缩放结论。
- state: 源图是工作流编辑器；实现图是 M1 服务器连接成功页。两者不是同一产品状态，本次只比较已确认的共同视觉语言（顶栏、点阵背景、白银卡片、阴影、文字对比、彩色图标和按钮），不对区域数量或节点布局做虚假的像素级一致性判断。

## Findings

- 未发现可执行的 P0、P1 或 P2 问题。
- 源图与实现图共同采用蓝靛渐变顶栏、连续浅色点阵背景、圆角白银渐变表面和明显的多层外阴影；实现中的服务器卡片、数据目录卡片和弹窗没有回到扁平化样式。
- 左右边缘、卡片高光和阴影均被圆角正确裁剪；正文保持深色高对比，未重现早期“文字看不见”的问题。

## 必查保真表面

- fonts and typography: 使用 Segoe UI / Microsoft YaHei UI 系统字体栈；标题、正文、标签和等宽服务器地址有清晰层级。桌面截图中无截断或异常换行；390 px CSS 窄窗口检查中正文正常换行。
- spacing and layout rhythm: 桌面页边距、标题与主操作间距、卡片内边距、18 px 圆角和三层阴影形成与源图一致的立体节奏。390 px CSS 窄窗口下实测 `body=375 px`、`app-window=355 px`、`server-card=317 px`，无横向溢出。
- colors and visual tokens: 白银渐变、蓝紫主操作、绿色成功、橙色警告、红色阻断均通过统一 token 表达；状态同时有图标和文字，不只依赖颜色。
- image quality and asset fidelity: 品牌图标复用项目真实 SVG 资源；界面图标来自 Phosphor 官方图标库并统一使用圆润 duotone/solid 风格。没有用 emoji、文本字符、CSS 绘图或临时手绘 SVG 冒充图标资产。
- copy and content: 首次启动说明明确数据范围与用户选择权；服务器页说明身份核验和自动识别目的；凭据说明明确 DPAPI 保护。文案能脱离开发上下文独立成立。
- accessibility: 表单有显式标签，弹窗有 `role=dialog`、`aria-modal` 和标题关联，错误使用 `role=alert`，按钮有可见焦点，动画遵循 `prefers-reduced-motion`。

## Full-view comparison evidence

源图和桌面实现截图在同一次比较输入中并列打开。完整视图足以辨认顶栏渐变、点阵背景、卡片边缘、三层阴影、图标、按钮、标题和正文，因此这些共同表面可直接判定。实现保留了源图的空间层级与金属银白质感，同时根据服务器页状态使用更大的主卡片和留白，这是产品状态差异而非视觉漂移。

## Focused region comparison evidence

未另做裁剪比较：两张完整截图中的关键共同区域均清晰可读，且源图不存在服务器页或首次启动页的同状态局部区域。额外打开了数据目录页与添加服务器弹窗，用于检查白银卡片、表单凹面、文字对比和阴影的一致性。

## Primary interactions tested

- 点击“检查并连接”，确认状态从“已保存”变为“连接正常”，并展示 openEuler、系统家族、包管理器、服务管理器、架构和 Shell。
- 点击“添加服务器”，确认弹窗打开；检查密码/私钥分段控件、端口默认值和 DPAPI 提示；通过关闭按钮退出弹窗。
- 打开首次启动数据目录状态，确认不出现 AppData 默认路径或自动选择文案。
- 自动化组件测试另覆盖新指纹信任、已变更指纹阻断、端口校验、凭据输入互斥和秘密清空。
- 浏览器控制台 error 日志：服务器页 `0`，数据目录页 `0`。

## Open Questions

- 当前视觉真值只提供工作流编辑器状态。后续实现工作流页面时，应以同一截图做同状态、同区域的精确比较；这不阻塞本次 M1 服务器与首次启动界面。

## Comparison history

- Pass 1: 首次实现截图曾受到临时窄视口覆盖影响，属于证据规格未归一化，不作为设计发现。
- Normalization: 明确设置 `1280 × 720` CSS 视口，重新捕获服务器连接成功页，并与源图在同一比较输入中复核。
- Pass 2: 未发现 P0/P1/P2 差异；无需视觉修复。桌面、窄窗口、首次启动和添加服务器弹窗证据均无阻断项。

## Implementation Checklist

- [x] 白银渐变表面与圆角内裁剪高光
- [x] 三层外阴影与清晰边缘
- [x] 高对比深色文字
- [x] 蓝紫、绿、橙、红语义色及图标文字双重表达
- [x] 真实品牌 SVG 与统一图标库
- [x] 桌面和窄窗口响应式检查
- [x] 核心交互与浏览器错误检查

## Follow-up Polish

- 后续工作流页面实现后，对左侧步骤卡、中间节点卡和右侧参数卡做同状态逐区比较。

final result: passed
