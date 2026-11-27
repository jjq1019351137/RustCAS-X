# Quark-first 回归清单

用于 Docker / 本地联调。勾选即过。

## 基础

- [ ] `/api/health` 正常
- [ ] 首次安装创建管理员 + 登录 / 刷新 / 退出
- [ ] 权限：admin 可见任务 / 网盘 / 审计 / 设置

## 夸克追剧

- [ ] 添加夸克 Cookie 账号并 probe 成功
- [ ] 创建追剧任务（分享链 + savepath）
- [ ] 手动执行：转存成功，执行日志可看
- [ ] 分享失效：写入 ban + 自动切备用链并立即重跑
- [ ] `POST /tasks/repair-banned`：修复后自动重跑，`reran` > 0
- [ ] 调度器开启后按时执行（可临时缩短 interval）

## 发现 / 媒体

- [ ] 豆瓣分类 / 列表有数据（或 mock 回落）
- [ ] TMDB 已配置时列表卡片带 `tmdb`
- [ ] `/api/media/proxy-image` 可加载海报
- [ ] 缓存页：proxy-image stats / purge / clear

## 同步

- [ ] local→local 复制文件
- [ ] strategy.`one_way_delete_extras`：目标多余文件进入延时删除队列
- [ ] strategy.`openlist_copy_batch_size` / `request_interval_seconds` 生效
- [ ] 执行中取消：状态变为 cancelled，任务 `is_running=0`
- [ ] local↔openlist / openlist→openlist（需先配 OpenList）
- [ ] 执行文件列表 `sync_execution_files` 有记录
- [ ] 完成后有通知（若已开启渠道）
- [ ] 天翼 families/登录入口返回友好提示（非裸 404）

## 通知

- [ ] webhook：自定义 `headers` / `bearer_token` 生效
- [ ] telegram / wxpusher 测试发送
- [ ] 追剧 / 同步结束触发 runtime 通知

## 管理边角

- [ ] 审计日志列表 / 详情可见关键写操作
- [ ] 网盘探针调度配置可保存；`refresh-profiles` 可用
- [ ] `/plugins` 返回 `[]`（非对象）
- [ ] 用户批量启停 / 角色设置

## 明确不做（本镜像）

- dl302 / strm
- 天翼账密验证码深度登录
- 插件运行时执行
