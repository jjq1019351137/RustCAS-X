# Rust 迁移说明

## 工具

已安装并配置 **rust-anything**（Cursor MCP）。分析报告：`docs/rust-anything-analyze.md`。

建议循环（单单元）：`analyze → bridge → record → port → build → diff → shadow → gate`。

整仓目标：纯 Rust Axum 服务（本仓库），而非长期 PyO3 混编；rust-anything 用于对齐行为与排序优先级。

## Wave 规划（摘要）

1. **Wave 0（本提交）**：HTTP 壳、配置/密钥、SQLite migrate、auth/setup/health、任务列表骨架、Docker/GHA  
2. **Wave 1**：models/db 对齐、users/roles、system settings  
3. **Wave 2**：quark / cloud189 适配器核心  
4. **Wave 3**：drama executor / scheduler  
5. **Wave 4**：sync executor / deferred delete  
6. **Wave 5**：TMDB / OpenList / notifications  

## 兼容约定

- API 前缀：`/api`
- 密码：bcrypt
- JWT：HS256，`sub` + `perms`
- Refresh：HttpOnly cookie `refresh_token` + SHA256(pepper)
