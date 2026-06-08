# mirror-karinjs · 白名单同步源（deploy-sync）

本分支是 [mirror](https://github.com/KarinJS/mirror) 的**远程白名单同步源**，只包含两个文件：

- `config.mirror.json` —— 供各实例拉取的白名单（**只有白名单，不含任何应用配置**）
- `README.md` —— 本说明

> 本分支故意不含代码、也不含 `host`/`port`/`auth` 等应用设置，只做纯白名单数据源。

## 工作方式

各实例在自己的 `config/config.json`（应用设置）里启用同步，指向本文件：

```json
"configSync": {
  "enabled": true,
  "intervalSeconds": 300,
  "url": "https://raw.githubusercontent.com/KarinJS/mirror/refs/heads/deploy-sync/config.mirror.json"
}
```

实例每隔 `intervalSeconds` 秒拉取本文件，热更新内存里的白名单并写回本地 `config/config.mirror.json`，无需重启。`auth` / `host` / `port` / `geo` / `configSync` 等**全部在各实例本地的 `config.json` 里**——本分支只能改白名单（仍受各路由的 SSRF / 路径校验约束）。

## config.mirror.json 格式

5 个子键，省略的默认为空（即该路由拒绝全部）：

```json
{
  "avatar": ["karinjs"],
  "raw": { "owner": { "repo": [{ "branch": "HEAD", "file": "package.json" }] } },
  "releases": { "owner": { "repo": ["asset.zip"] } },
  "unpkg": { "pkg": ["file/path"] },
  "mirror": { "https://host/path": 0 }
}
```

## 托管方式

同步端的 `Content-Type` 校验接受 JSON 与 `text/plain`，拒绝 `text/html`/二进制。常见托管都行：

- ✅ **GitHub raw**（实时生效，`text/plain`，**当前线上用这个**）
- ✅ jsDelivr（`application/json`，有 ~12h 缓存）
- ✅ GitHub Pages / Cloudflare / EO / 对象存储

## 更新白名单

改 `config.mirror.json` → `commit` & `push` 本分支 → 各实例下个同步周期自动生效。
