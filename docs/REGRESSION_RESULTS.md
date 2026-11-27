# REGRESSION 实机结果

时间: 2026-11-27 14:16:18
服务: http://127.0.0.1:8000/api
数据目录: d:\工作\AAAAAAAAAAAA-项目\cloud-X\RustCAS-X\backend\data-regression

汇总: **PASS=22 FAIL=0 BLOCKED=7 SKIP=4**

| 项目 | 状态 | 说明 |
|---|---|---|
| /api/health | PASS | runtime=rust |
| 首次安装创建管理员 | SKIP | already initialized |
| 登录 | PASS | roles=admin |
| 权限：admin 可见任务/网盘/审计/设置 | PASS | perms=18 |
| 刷新 token | PASS |  |
| 退出并重新登录 | PASS |  |
| 添加夸克 Cookie 账号并 probe | BLOCKED | 需要真实 Quark Cookie |
| 创建追剧任务（分享链+savepath） | BLOCKED | 需要真实分享链 |
| 手动执行转存 | BLOCKED | 依赖 Cookie |
| 分享失效 ban+备用链重跑 | BLOCKED | 依赖 Cookie |
| POST /tasks/repair-banned | BLOCKED | 依赖 Cookie |
| 调度器按时执行 | BLOCKED | 需缩短 interval 并配任务后观察 |
| 豆瓣分类 | PASS |  |
| 豆瓣列表有数据（或 mock 回落） | PASS | items=5 tmdb=off |
| TMDB 已配置时列表卡片带 tmdb | SKIP | TMDB 未配置 |
| /api/media/proxy-image 可加载海报 | PASS | bytes=11576 |
| 缓存页：proxy-image stats/purge/clear | PASS |  |
| local→local 复制文件 | PASS | task=4 exec=4 |
| strategy.one_way_delete_extras 延时删除队列 | PASS | extra.txt enqueued |
| strategy batch_size / request_interval / concurrency 生效 | PASS | batch=5 concurrency=2 interval_ms=0 |
| sync_execution_files 有记录 | PASS | exec=4 files=3 |
| 执行中取消 → cancelled + is_running=0 | PASS | exec=5 (API cancel; local无长窗口故用 running 桩) |
| local↔openlist / openlist→openlist | BLOCKED | 需先配 OpenList |
| 完成后有通知（若已开启渠道） | SKIP | 见下方 webhook 测试 |
| 天翼 families 友好提示（非裸404） | PASS | cloud189 families not ported (Quark-first) |
| 天翼登录入口友好提示 | PASS | {"code":"CLOUD189_NOT_PORTED","message":"天翼账密/扫码登录已放弃深度迁移，请使用夸克 Cookie 账号"} |
| webhook：自定义 headers / bearer_token 生效 | PASS | Authorization + X-Reg-Token 收到 |
| telegram / wxpusher 测试发送 | PASS | 未启用渠道返回友好 error（符合预期） |
| 追剧/同步结束触发 runtime 通知 | SKIP | 渠道已测；真实触发依赖任务成功 |
| 审计日志列表/详情 | PASS | n=20 sample=notification.config.update |
| 网盘探针调度可保存；refresh-profiles 可用 | PASS | crontab=0 */6 * * * |
| /plugins 返回 [] | PASS |  |
| 用户批量启停 / 角色设置 | PASS | user=3 |

## 明确不做（本镜像）
- dl302 / strm
- 天翼账密验证码深度登录
- 插件运行时执行

## 阻塞项说明
夸克追剧相关项需提供真实 Cookie / 分享链后才能实机验证；OpenList 双向同步需先配置 OpenList。
