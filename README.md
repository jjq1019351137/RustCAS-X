# RustCAS-X

cas-jjq / CAS 的 **Rust 后端迁移仓库**（Axum + SQLite + 原 Vue 前端）。

## 现状（Wave 0–5 骨架）

已用 [rust-anything](https://rustanything.ai) 对原 Python 后端做整仓分析（见 `docs/rust-anything-analyze.md`）。

| 模块 | 状态 |
| --- | --- |
| auth / setup / health / users / roles | 已实现 |
| drive-accounts CRUD + types（quark/cloud189） | 已实现；`extract_url` 已移植 |
| tasks / sync-tasks 创建 + run/stream | 骨架可跑（execution 落库） |
| deferred sync delete 队列 | 已实现（本地路径） |
| TMDB / OpenList settings；OpenList browse | 已实现 |
| 适配器 lsdir/转存、追剧全流程、双向同步全量 | **进行中** |

详见 `docs/MIGRATION.md`。

## 本地开发

```bash
# 后端
cd backend
cargo run

# 前端
cd frontend
pnpm install
pnpm dev
```

## Docker

默认推送 **GHCR**：`ghcr.io/jjq1019351137/rustcas-x`（无需额外 secrets）。  
若仓库配置了 `DOCKERHUB_USERNAME` / `DOCKERHUB_TOKEN`，会额外推 Docker Hub；有阿里云 secrets 再推阿里云。

GitHub Actions：仅 `main` 推送触发（打 `v*` 标签不单独跑第二遍；若提交已挂标签，镜像仍会带版本号）。

```bash
docker run -d -p 5115:5115 -v rustcas-data:/app/backend/data ghcr.io/jjq1019351137/rustcas-x:latest
```

打开 `http://localhost:5115` 完成首次管理员初始化。

## 来源

- 原项目：https://github.com/jjq1019351137/cas-jjq
- 本仓库：https://github.com/jjq1019351137/RustCAS-X
