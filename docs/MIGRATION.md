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
| 2 | quark/cloud189：init/ls_dir/stoken/detail + **mkdir/get_fids/save_file/query_task** | **done（天翼账密登录/验证码已放弃深度）** |
| 3 | drama run：stoken → 列表 → mkdir(savepath) → **转存并等待任务** | **done** |
| 4 | sync run/stream + deferred_sync_deletes 队列 | **done** |
| 5 | TMDB/OpenList settings；OpenList browse HTTP 客户端 | **done** |
| 6 | 追剧失败自动修链 + repair-banned 后重跑 | **done** |
| 7 | 豆瓣发现页 + proxy-image 缓存 | **done** |
| 8 | 审计列表/写入、插件空注册表、网盘探针调度 | **done** |
| 9 | 同步/设置类写操作审计补齐 | **done** |

## Quark-first 约定

- **主线**：夸克追剧 / 同步 / 分享预览 / 资源搜索 / 通知 / TMDB / OpenList
- **放弃**：天翼深度登录验证码、dl302 / strm 全量、插件运行时执行
- **可空操作**：savepath snapshots、lsdir-cache refresh（返回稳定 shape）

## 下一步优先

1. Docker 镜像联调 / 前端回归清单（追剧、修链、豆瓣、探针）
2. Sync 文件级 walk/copy 与延时删除行为对照
3. 通知渠道边角（webhook 鉴权头等）
4. 插件运行时（若需要再开波次）

## 兼容约定

- API 前缀：`/api`
- 密码：bcrypt
- JWT：HS256，`sub` + `perms`
- Refresh：HttpOnly cookie `refresh_token` + SHA256(pepper)
- 驱动类型：`quark`（主）/ `cloud189`（保留壳，不继续深迁）
