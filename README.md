# mirror

基于白名单的 GitHub / npm 反向代理服务，专为 Tencent EO 全站加速优化。

## 特性

- **白名单控制** - 仅代理已配置的上游资源，防止滥用
- **多路由支持** - GitHub Releases、Raw 文件、头像、npm/unpkg、自定义镜像
- **地理位置过滤** - 可选的国家/地区访问控制
- **缓存控制** - 每种路由类型可配置独立 TTL
- **远程配置同步** - 定时从远程拉取白名单配置，自动热更新
- **安全加固** - 路径遍历防护、输入验证、大小限制
- **现代技术栈** - Rust 后端（Axum + Tokio）、Vue 3 前端

## 部署

CI 构建产物目录结构（二进制 + 前端 + 配置均在根目录）：

```
.
├── mirror       # 二进制
├── webui/dist/          # 前端静态文件
└── config/              # 配置文件（首次运行自动生成）
```

### Linux（nohup）

```bash
# 安装部署（CI 已将二进制、webui/dist、config/ 打包到同一目录）
tar -xzf mirror.tar.gz -C /opt/mirror/
cd /opt/mirror
chmod +x mirror

# 启动
nohup ./mirror > /dev/null 2>&1 &
echo $! > mirror.pid
echo "started (PID $(cat mirror.pid))"

# 查看状态
if kill -0 $(cat mirror.pid) 2>/dev/null; then
    echo "running (PID $(cat mirror.pid))"
else
    echo "stopped"
fi

# 重启
kill -TERM $(cat mirror.pid) 2>/dev/null
sleep 2
nohup ./mirror > /dev/null 2>&1 &
echo $! > mirror.pid
echo "restarted (PID $(cat mirror.pid))"

# 停止
kill -TERM $(cat mirror.pid) 2>/dev/null
rm -f mirror.pid
echo "stopped"

# 查看日志
tail -f logs/mirror.log
```

### Windows

```powershell
# 直接运行（前台，Ctrl+C 停止）
.\mirror.exe
```

> 生产环境建议配合 systemd（Linux）或 NSSM / 任务计划程序（Windows）实现进程守护与开机自启。

## 快速开始（开发）

### 环境要求

- **Rust** 1.70+ （后端）
- **pnpm** 8+ （前端）

### 开发模式

```bash
pnpm install
pnpm dev             # 同时启动前后端

# 或分别启动
pnpm dev:backend     # Rust 后端，cargo-watch 热重载
pnpm dev:frontend    # Vite 开发服务器
```

### 构建

```bash
pnpm build           # 构建所有
pnpm build:backend   # → target/release/mirror
pnpm build:frontend  # → webui/dist/
```

## 配置详解

所有配置集中在项目根目录的 `config/config.mirror.json` 这一个文件中（包含应用设置与全部白名单）。首次运行时若该文件不存在，程序会自动创建并填充默认配置。

> 该文件可能包含鉴权凭据，已默认加入 `.gitignore`，请勿提交到版本库。

### 应用设置

文件顶层为应用运行参数：

| 字段 | 类型 | 默认值 | 单位 | 说明 |
|------|------|--------|------|------|
| `host` | string | `"0.0.0.0"` | — | 监听地址 |
| `port` | number | `7878` | — | 监听端口 |
| `publicOrigin` | string | `"https://mirror.karinjs.com"` | — | 对外公开的访问地址（用于重定向） |
| `trustProxyHeaders` | bool | `true` | — | 是否信任反向代理转发的 `X-Forwarded-*` 头 |
| `logLevel` | string | `"info"` | — | 日志级别：`trace` / `debug` / `info` / `warn` / `error` |
| `geo.mode` | string | `"off"` | — | 地理位置过滤模式：`off` / `allow` / `deny` |
| `geo.headerName` | string | `"EO-Client-IPCountry"` | — | 携带国家代码的 HTTP 请求头名称 |
| `geo.countries` | string[] | `["CN","HK","MO","TW"]` | — | 需过滤的国家/地区代码列表 |
| `cacheTTL.raw` | number | `300` | **秒** | `/raw/` 路由的缓存 TTL |
| `cacheTTL.avatar` | number | `300` | **秒** | `/avatar/` 路由的缓存 TTL |
| `cacheTTL.unpkg` | number | `300` | **秒** | `/unpkg/` 路由的缓存 TTL |
| `mirror.defaultTTL` | number | `0` | **秒** | `/mirror/` 路由下未明确指定 TTL 的 URL 的默认值 |
| `mirror.defaultMaxSize` | number | `52428800` | **字节** | 默认响应体大小上限（50 MB） |
| `mirror.absoluteMaxSize` | number | `1073741824` | **字节** | 响应体的硬上限（1 GB） |
| `mirror.fetchTimeoutMs` | number | `30000` | **毫秒** | 上游回源超时时间 |
| `mirror.fetchRetries` | number | `2` | **次** | 上游回源失败（连接错误/超时/5xx）时的最大重试次数，总尝试次数 = 重试次数 + 1，采用指数退避（约 200ms、400ms…），仅在收到响应头之前重试 |
| `cors.enabledRoutes` | string[] | `["raw","unpkg","mirror"]` | — | 启用 CORS 响应头的路由列表 |
| `auth.enabled` | bool | `false` | — | 是否启用请求头鉴权 |
| `auth.key` | string | `""` | — | 鉴权请求头名称 |
| `auth.value` | string | `""` | — | 鉴权请求头的期望值 |

**TTL 语义（适用于所有 TTL 字段）：**

| TTL 值 | `Cache-Control` 响应头 | 说明 |
|--------|----------------------|------|
| `-2` | 透传上游 | 原样转发上游的 `Cache-Control` 和 `ETag` |
| `-1` | `public, max-age=31536000, immutable` | 1 年强制缓存（适合带版本号的静态资源） |
| `0` | `no-store` | 禁止缓存 |
| `> 0` | `public, max-age=<ttl>` | 自定义缓存时长（**单位：秒**） |

### whitelists — 白名单

文件中的 `whitelists` 对象集中保存全部白名单，包含 5 个子键：`avatar` / `raw` / `releases` / `unpkg` / `mirror`。各子键省略时默认为空（即该路由拒绝全部请求）。

```json
{
  "whitelists": {
    "avatar": ["karinjs", "NapNeko"],
    "raw": {
      "karinjs": {
        "karin": [
          { "branch": "HEAD", "file": "package.json" },
          { "branch": "main", "file": "README.md" }
        ]
      }
    },
    "releases": {
      "NapNeko": {
        "NapCatQQ": ["NapCat.Framework.zip", "NapCat.linux-amd64"]
      }
    },
    "unpkg": {
      "karin": ["package.json", "dist/karin.umd.js"]
    },
    "mirror": {
      "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions.json": 0,
      "https://example.com/stable/asset.zip": -1,
      "https://example.com/dynamic/data.json": { "ttl": 60, "maxSize": 1048576 }
    }
  }
}
```

#### `whitelists.avatar` — GitHub 头像白名单

字符串数组，每个元素是一个允许代理头像的 GitHub 用户名。请求 `/avatar/<user>.png` 时检查 `<user>` 是否命中，命中则代理 `https://github.com/<user>.png`。

#### `whitelists.raw` — GitHub Raw 文件白名单

三层嵌套结构 `{owner: {repo: [{branch, file}]}}`。请求 `/raw/<owner>/<repo>/<branch>/<file>` 时精确匹配；`branch` 为 `"HEAD"` 表示接受任何分支。

#### `whitelists.releases` — GitHub Releases 白名单

三层嵌套结构 `{owner: {repo: [asset_filename]}}`。请求 `/gh/<owner>/<repo>/releases/download/<tag>/<file>` 时校验 `<file>` 是否存在于对应仓库的允许列表中。

#### `whitelists.unpkg` — npm/unpkg 白名单

`{package_name: [file_paths]}`。请求 `/unpkg/<pkg>[@version]/<file>` 时校验文件路径是否在白名单中，支持版本号或版本范围。

#### `whitelists.mirror` — 自定义镜像白名单

URL 到规则（TTL 或 `{ttl, maxSize?}`）的映射。

- **简写形式**：值是数字时，即 TTL
- **完整形式**：`ttl` 指定缓存策略，`maxSize` 可选，覆盖全局 `defaultMaxSize`

### configSync — 远程配置自动同步

配置同步功能可以定时从一个远程直链 URL 拉取 `config.mirror.json`，通过 SHA-256 比对检测变更，自动热更新内存中的**白名单**，无需重启服务。

> **安全边界**：远程同步**只采用 `whitelists` 部分**。`auth` / `host` / `port` / `geo` / `configSync` 等始终使用本地值，远程**永远无法**关闭鉴权、修改监听地址或改写同步目标。即使同步源被攻陷，最坏也只能改动白名单（而白名单仍受各路由的 SSRF / 路径校验约束）。

```json
{
  "configSync": {
    "enabled": false,
    "intervalSeconds": 300,
    "url": "https://example.com/configs/config.mirror.json"
  }
}
```

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `configSync.enabled` | bool | `false` | 是否启用远程同步 |
| `configSync.intervalSeconds` | number | `300` | 检查间隔（秒），最小值为 1 |
| `configSync.url` | string | `""` | 远程 `config.mirror.json` 的直链 URL |

**工作流程：**

1. 每隔 `intervalSeconds` 秒，对配置的 `url` 发起一次 HTTP GET 请求
2. **强制校验响应 `Content-Type` 必须为 JSON**（`application/json` / `text/json` / `*+json`）——非 JSON（HTML、纯文本等）直接拒绝
3. 计算响应体的 SHA-256，与上次成功采用的哈希比对；相同则跳过
4. 若不同 → 完整解析并校验整份配置（结构 + 语义）→ 取其 `whitelists` 与**本地**应用设置合并 → 写入本地文件 → 热更新内存白名单
5. 若请求失败 / 非 JSON / 校验失败 → 记录告警日志，本地文件与内存配置均不受影响，下个周期重试

**安全特性：**

- 远程仅能影响白名单；`auth` / `host` / `port` / `geo` / `configSync` 恒为本地值
- 强制 JSON `Content-Type` + 完整结构/语义校验，无效或被劫持（如返回登录页）的响应一律拒绝，绝不写入磁盘
- 网络错误 / 非 2xx 状态码 / 超出 10 MB 体积上限均记录告警，不影响服务正常运行
- 同步 URL 经过 SSRF / DNS 重绑定防护校验（必须 https、禁止内网/环回/userinfo）

> **托管提示**：同步源必须以 JSON `Content-Type` 返回。GitHub **raw**（`raw.githubusercontent.com`）返回的是 `text/plain`，会被拒绝；请改用 GitHub **Pages**、jsDelivr、Cloudflare 或你自己的 EO/对象存储（这些默认以 `application/json` 返回 `.json`）。

## 路由说明

| 路由 | 格式 | 示例 |
|------|------|------|
| **GitHub Releases** | `/gh/<owner>/<repo>/releases/download/<tag>/<file>` | `/gh/NapNeko/NapCatQQ/releases/download/v4.18.0/NapCat.Framework.zip` |
| **GitHub Raw** | `/raw/<owner>/<repo>/<branch>/<path>` | `/raw/karinjs/karin/main/package.json` |
| **GitHub 头像** | `/avatar/<user>.png` | `/avatar/karinjs.png` |
| **npm/unpkg** | `/unpkg/<pkg>[@version]/<file>` | `/unpkg/karin/package.json` |
| **通用镜像** | `/mirror/<host>/<path>` | `/mirror/example.com/file.zip` |

## 安全性

**核心防护：**

- 路径遍历验证（所有路由）
- 白名单优先设计（默认拒绝）
- 输入清理（拒绝 `..`、`//`、`\\`）
- 查询参数拒绝（带 `?` 的请求直接 404，防缓存绕过）
- 流式大小限制
- 地理位置阻断（Fail-Closed）
- 请求头鉴权（可选）
- 无 SQL/命令注入风险

## 缓存头处理

后端返回正确的 `Cache-Control` 头，适配 CDN 集成：

- **Releases**（`ttl: -1`）：`public, max-age=31536000, immutable`
- **Raw / Avatar / unpkg**：由 `config.mirror.json` 中 `cacheTTL` 对应字段控制
- **Mirror**：由 `whitelists.mirror` 中按 URL 配置的 TTL 控制
- **不缓存**（`ttl: 0`）：`no-store`

### EO CDN 配置建议

1. 关闭 EO 全站缓存
2. 后端缓存头将控制缓存行为
3. EO 会尊重 `Cache-Control` 指令
4. 在 EO 中配置 `EO-Client-Country` 请求头注入

## 项目结构

```
.
├── src/                   # Rust 后端
│   ├── main.rs            # 入口
│   ├── server.rs          # Axum 路由与中间件
│   ├── config.rs          # 配置结构定义与加载
│   ├── sync.rs            # 远程配置同步后台任务
│   ├── proxy.rs           # 上游代理逻辑
│   ├── routes/            # 路由处理器
│   │   ├── releases.rs    # GitHub Releases
│   │   ├── raw.rs         # GitHub Raw
│   │   ├── avatar.rs      # GitHub 头像
│   │   ├── unpkg.rs       # npm/unpkg
│   │   ├── mirror.rs      # 通用镜像
│   │   └── mod.rs
│   ├── geo.rs             # 地理位置检查
│   ├── stats.rs           # 请求统计
│   ├── http_utils.rs      # HTTP 工具函数
│   └── error.rs           # 错误类型
├── webui/                 # Vue 3 前端
│   ├── src/
│   ├── public/
│   └── dist/
├── config/                # JSON 配置文件（运行时生成）
├── Cargo.toml
├── package.json
└── pnpm-workspace.yaml
```

## 开发指南

### 添加新路由

1. 在 `src/routes/` 创建新文件
2. 实现路由处理函数
3. 在 `src/routes/mod.rs` 中导出
4. 在 `src/server.rs` 中注册路由

### 修改配置结构

1. 更新 `src/config.rs` 中的类型定义
2. 更新 `config/` 目录下的默认配置生成逻辑
3. 更新本文档

### 前端开发

```bash
cd webui
pnpm dev
# Vite 开发服务器，API 请求代理到 http://127.0.0.1:3000
```

## 许可证

MIT
