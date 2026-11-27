# RustCAS-X

cas-jjq / CAS 的 **Rust 后端迁移仓库**（Axum + SQLite + 原 Vue 前端）。

## 现状（Wave 0）

已用 [rust-anything](https://rustanything.ai) 对原 Python 后端做整仓分析（见 `docs/rust-anything-analyze.md`）：约 **4.5 万行 / 129 单元**。

本仓库当前可运行能力：

| 模块 | 状态 |
| --- | --- |
| `/api/health` | 已实现 |
| `/api/setup/*` | 已实现（初始化管理员） |
| `/api/auth/*` | 已实现（login/refresh/logout/me，bcrypt + JWT） |
| `/api/dashboard/overview` | 已实现 |
| `/api/tasks` / `/api/sync-tasks` | 列表只读骨架 |
| 驱动 / 追剧执行 / 同步执行 / TMDB / OpenList | **未迁移**（API stub，前端可启动） |

后续按 rust-anything 波次继续移植适配器与 runtime。

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
