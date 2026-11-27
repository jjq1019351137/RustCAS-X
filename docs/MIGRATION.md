# Rust 迁移说明

## 工具

已安装并配置 **rust-anything**（Cursor MCP）。分析报告：`docs/rust-anything-analyze.md`。

建议循环（单单元）：`analyze → bridge → record → port → build → diff → shadow → gate`。

整仓目标：纯 Rust Axum 服务（本仓库），而非长期 PyO3 混编；rust-anything 用于对齐行为与排序优先级。

## Wave 规划

| Wave | 内容 | 状态 |
| --- | --- | --- |
| 0 | HTTP 壳、SQLite migrate、auth/setup/health、Docker/GHA | **done** |
| 1 | users/roles/permissions、system save-rules、app_settings KV | **done** |
| 2 | quark/cloud189 适配器 meta + `extract_url` + drive-accounts CRUD/probe 骨架 | **done（lsdir/转存待全量）** |
| 3 | drama run/stream 骨架（解析分享链接 + execution 日志） | **done（转存/扫盘待全量）** |
| 4 | sync run/stream + deferred_sync_deletes 队列 | **done（文件同步待全量）** |
| 5 | TMDB/OpenList settings；OpenList browse HTTP 客户端 | **done（TMDB API/缓存待全量）** |

## 下一步优先

1. Quark/Cloud189：`init` / `ls_dir` / `get_stoken` / `save_file` 对齐 Python 行为并用 rust-anything `record/diff`  
2. Drama：TMDB 进度 + 目录匹配 + 转存闭环  
3. Sync：本地↔网盘 walk/copy + 删除延时真实落地  
4. 通知插件 / 调度 crontab  

## 兼容约定

- API 前缀：`/api`
- 密码：bcrypt
- JWT：HS256，`sub` + `perms`
- Refresh：HttpOnly cookie `refresh_token` + SHA256(pepper)
- 驱动类型：仅 `quark` / `cloud189`
