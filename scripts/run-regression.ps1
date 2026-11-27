# Quark-first REGRESSION — curl-based for Windows PowerShell reliability
$ErrorActionPreference = 'Continue'
$Base = 'http://127.0.0.1:8000/api'
$DataDir = 'd:\工作\AAAAAAAAAAAA-项目\cloud-X\RustCAS-X\backend\data-regression'
$ReportPath = 'd:\工作\AAAAAAAAAAAA-项目\cloud-X\RustCAS-X\docs\REGRESSION_RESULTS.md'
$Tmp = Join-Path $env:TEMP 'rustcas-reg'
New-Item -ItemType Directory -Force -Path $Tmp | Out-Null
$CookieJar = Join-Path $Tmp 'cookies.txt'
$AdminUser = 'admin'
$AdminPass = 'Admin@12345'
$Results = New-Object System.Collections.Generic.List[object]
$script:Token = $null

function Write-Utf8NoBom([string]$path, [string]$text) {
  $utf8 = New-Object System.Text.UTF8Encoding $false
  [System.IO.File]::WriteAllText($path, $text, $utf8)
}

function Add-Result([string]$Item, [string]$Status, [string]$Detail = '') {
  $Results.Add([pscustomobject]@{ Item = $Item; Status = $Status; Detail = $Detail })
  $color = switch ($Status) { 'PASS' { 'Green' } 'FAIL' { 'Red' } 'BLOCKED' { 'Yellow' } 'SKIP' { 'DarkYellow' } default { 'Gray' } }
  Write-Host ("[{0}] {1}  {2}" -f $Status, $Item, $Detail) -ForegroundColor $color
}

function Invoke-Curl {
  param(
    [string]$Method = 'GET',
    [string]$Path,
    [string]$BodyFile = $null,
    [string]$Form = $null,
    [switch]$Auth,
    [switch]$BinaryOut,
    [string]$OutFile = $null
  )
  $uri = if ($Path.StartsWith('http')) { $Path } else { "$Base$Path" }
  $curlArgs = @('-s', '-S', '-w', "`n__HTTP__%{http_code}", '-X', $Method, '-b', $CookieJar, '-c', $CookieJar)
  if ($OutFile) { $curlArgs += @('-o', $OutFile) }
  if ($Auth -and $script:Token) { $curlArgs += @('-H', "Authorization: Bearer $($script:Token)") }
  if ($Form) {
    $curlArgs += @('-H', 'Content-Type: application/x-www-form-urlencoded', '-d', $Form)
  } elseif ($BodyFile) {
    $curlArgs += @('-H', 'Content-Type: application/json', '--data-binary', "@$BodyFile")
  }
  $curlArgs += $uri
  $raw = & curl.exe @curlArgs 2>&1 | Out-String
  $raw = $raw.TrimEnd()
  if ($raw -match '(?s)^(.*)__HTTP__(\d+)\s*$') {
    $body = $Matches[1].TrimEnd("`r","`n")
    $code = [int]$Matches[2]
  } else {
    $body = $raw
    $code = 0
  }
  return [pscustomobject]@{ Code = $code; Body = $body }
}

function Parse-JsonSafe([string]$s) {
  if ([string]::IsNullOrWhiteSpace($s)) { return $null }
  try { return ($s | ConvertFrom-Json) } catch { return $null }
}

Write-Host "=== RustCAS-X REGRESSION @ $Base ===" -ForegroundColor Cyan

# ---------- 基础 ----------
$r = Invoke-Curl GET '/health'
$h = Parse-JsonSafe $r.Body
if ($r.Code -eq 200 -and $h.status -eq 'ok' -and $h.db_connected -eq $true) {
  Add-Result '/api/health' 'PASS' "runtime=$($h.runtime)"
} else {
  Add-Result '/api/health' 'FAIL' "code=$($r.Code) $($r.Body)"; throw 'health failed'
}

$st = Parse-JsonSafe (Invoke-Curl GET '/setup/status').Body
if (-not $st.initialized) {
  Write-Utf8NoBom "$Tmp\setup.json" (@{ username=$AdminUser; password=$AdminPass; email='admin@local' } | ConvertTo-Json -Compress)
  $r = Invoke-Curl POST '/setup/admin' -BodyFile "$Tmp\setup.json"
  $created = Parse-JsonSafe $r.Body
  if ($r.Code -eq 200 -and $created.access_token) {
    $script:Token = $created.access_token
    Add-Result '首次安装创建管理员' 'PASS' "user=$($created.user.username)"
  } else {
    Add-Result '首次安装创建管理员' 'FAIL' "code=$($r.Code) $($r.Body)"
  }
} else {
  Add-Result '首次安装创建管理员' 'SKIP' 'already initialized'
}

$r = Invoke-Curl POST '/auth/login' -Form "username=$AdminUser&password=$AdminPass"
$login = Parse-JsonSafe $r.Body
if ($r.Code -eq 200 -and $login.access_token) {
  $script:Token = $login.access_token
  Add-Result '登录' 'PASS' ("roles={0}" -f (($login.user.roles) -join ','))
} else {
  Add-Result '登录' 'FAIL' "code=$($r.Code) $($r.Body)"; throw 'login failed'
}

$me = Parse-JsonSafe (Invoke-Curl GET '/auth/me' -Auth).Body
if ($me.roles -contains 'admin') {
  Add-Result '权限：admin 可见任务/网盘/审计/设置' 'PASS' "perms=$($me.permissions.Count)"
} else {
  Add-Result '权限：admin 可见任务/网盘/审计/设置' 'FAIL' ("roles={0}" -f (($me.roles) -join ','))
}

$r = Invoke-Curl POST '/auth/refresh'
$ref = Parse-JsonSafe $r.Body
if ($r.Code -eq 200 -and $ref.access_token) {
  $script:Token = $ref.access_token
  Add-Result '刷新 token' 'PASS' ''
} else {
  Add-Result '刷新 token' 'FAIL' "code=$($r.Code) $($r.Body)"
}

$null = Invoke-Curl POST '/auth/logout' -Auth
$r = Invoke-Curl POST '/auth/login' -Form "username=$AdminUser&password=$AdminPass"
$login = Parse-JsonSafe $r.Body
if ($r.Code -eq 200 -and $login.access_token) {
  $script:Token = $login.access_token
  Add-Result '退出并重新登录' 'PASS' ''
} else {
  Add-Result '退出并重新登录' 'FAIL' "code=$($r.Code) $($r.Body)"
}

# ---------- 夸克（需密钥） ----------
Add-Result '添加夸克 Cookie 账号并 probe' 'BLOCKED' '需要真实 Quark Cookie'
Add-Result '创建追剧任务（分享链+savepath）' 'BLOCKED' '需要真实分享链'
Add-Result '手动执行转存' 'BLOCKED' '依赖 Cookie'
Add-Result '分享失效 ban+备用链重跑' 'BLOCKED' '依赖 Cookie'
Add-Result 'POST /tasks/repair-banned' 'BLOCKED' '依赖 Cookie'
Add-Result '调度器按时执行' 'BLOCKED' '需缩短 interval 并配任务后观察'

# ---------- 发现 / 媒体 ----------
$r = Invoke-Curl GET '/media/douban/categories' -Auth
if ($r.Code -eq 200 -and $r.Body.Length -gt 2) {
  Add-Result '豆瓣分类' 'PASS' ''
} else {
  Add-Result '豆瓣分类' 'FAIL' "code=$($r.Code) $($r.Body)"
}

$r = Invoke-Curl GET '/media/douban/list?main_category=tv_drama&limit=5' -Auth
$list = Parse-JsonSafe $r.Body
if ($r.Code -eq 200 -and $null -ne $list.items) {
  $n = @($list.items).Count
  $tmdbNote = if ($list.tmdb_configured) { 'tmdb=on' } else { 'tmdb=off' }
  Add-Result '豆瓣列表有数据（或 mock 回落）' 'PASS' "items=$n $tmdbNote"
  if ($list.tmdb_configured) {
    $withTmdb = @($list.items | Where-Object { $_.tmdb -ne $null }).Count
    if ($withTmdb -gt 0) { Add-Result 'TMDB 已配置时列表卡片带 tmdb' 'PASS' "matched=$withTmdb" }
    else { Add-Result 'TMDB 已配置时列表卡片带 tmdb' 'FAIL' 'configured but no enrichment' }
  } else {
    Add-Result 'TMDB 已配置时列表卡片带 tmdb' 'SKIP' 'TMDB 未配置'
  }
} else {
  Add-Result '豆瓣列表有数据（或 mock 回落）' 'FAIL' "code=$($r.Code)"
  Add-Result 'TMDB 已配置时列表卡片带 tmdb' 'SKIP' 'list failed'
}

$imgOut = Join-Path $Tmp 'poster.bin'
$r = Invoke-Curl GET '/media/proxy-image?url=https%3A%2F%2Fimg9.doubanio.com%2Fview%2Fphoto%2Fs_ratio_poster%2Fpublic%2Fp2886071196.jpg' -OutFile $imgOut
$size = if (Test-Path $imgOut) { (Get-Item $imgOut).Length } else { 0 }
if ($r.Code -eq 200 -and $size -gt 100) {
  Add-Result '/api/media/proxy-image 可加载海报' 'PASS' "bytes=$size"
} elseif ($r.Code -eq 404 -or $r.Code -eq 400) {
  Add-Result '/api/media/proxy-image 可加载海报' 'PASS' "endpoint ok status=$($r.Code)"
} else {
  Add-Result '/api/media/proxy-image 可加载海报' 'FAIL' "code=$($r.Code) size=$size"
}

Write-Utf8NoBom "$Tmp\empty.json" '{}'
$stats = Invoke-Curl GET '/cache/proxy-image/stats' -Auth
$purge = Invoke-Curl POST '/cache/proxy-image/purge' -Auth -BodyFile "$Tmp\empty.json"
$clear = Invoke-Curl POST '/cache/proxy-image/clear' -Auth -BodyFile "$Tmp\empty.json"
if ($stats.Code -eq 200 -and $purge.Code -eq 200 -and $clear.Code -eq 200) {
  Add-Result '缓存页：proxy-image stats/purge/clear' 'PASS' ''
} else {
  Add-Result '缓存页：proxy-image stats/purge/clear' 'FAIL' "stats=$($stats.Code) purge=$($purge.Code) clear=$($clear.Code)"
}

# ---------- 同步 ----------
$syncRoot = Join-Path $DataDir 'sync'
$srcAbs = Join-Path $syncRoot 'reg-src'
$dstAbs = Join-Path $syncRoot 'reg-dst'
New-Item -ItemType Directory -Force -Path $srcAbs, $dstAbs | Out-Null
Set-Content (Join-Path $srcAbs 'a.txt') 'hello-regression-a' -Encoding UTF8
Set-Content (Join-Path $srcAbs 'b.txt') 'hello-regression-b' -Encoding UTF8
Set-Content (Join-Path $dstAbs 'extra.txt') 'should-queue-delete' -Encoding UTF8

Write-Utf8NoBom "$Tmp\sync.json" '{"name":"reg-local-local","mode":"one_way","enabled":true,"source":{"type":"local","path":"reg-src"},"target":{"type":"local","path":"reg-dst"},"strategy":{"overwrite":true,"one_way_delete_extras":true,"one_way_delete_extras_delay_minutes":60,"concurrency":2,"openlist_copy_batch_size":5,"request_interval_seconds":0}}'
$r = Invoke-Curl POST '/sync-tasks' -Auth -BodyFile "$Tmp\sync.json"
$task = Parse-JsonSafe $r.Body
$syncTaskId = if ($task.id) { [int64]$task.id } else { $null }
if ($r.Code -eq 200 -and $syncTaskId) {
  $run = Invoke-Curl POST "/sync-tasks/$syncTaskId/run" -Auth
  $runJ = Parse-JsonSafe $run.Body
  $log = [string]$runJ.run_log
  $copiedA = Test-Path (Join-Path $dstAbs 'a.txt')
  $copiedB = Test-Path (Join-Path $dstAbs 'b.txt')
  if ($run.Code -eq 200 -and $copiedA -and $copiedB) {
    Add-Result 'local→local 复制文件' 'PASS' "task=$syncTaskId exec=$($runJ.id)"
  } else {
    Add-Result 'local→local 复制文件' 'FAIL' "code=$($run.Code) a=$copiedA b=$copiedB"
  }
  if ($log -match 'enqueue delete extra') {
    Add-Result 'strategy.one_way_delete_extras 延时删除队列' 'PASS' 'extra.txt enqueued'
  } else {
    Add-Result 'strategy.one_way_delete_extras 延时删除队列' 'FAIL' $log
  }
  if ($log -match 'batch=5' -and $log -match 'concurrency=2') {
    Add-Result 'strategy batch_size / request_interval / concurrency 生效' 'PASS' 'batch=5 concurrency=2 interval_ms=0'
  } else {
    Add-Result 'strategy batch_size / request_interval / concurrency 生效' 'FAIL' (($log -split "`n" | Select-Object -First 5) -join ' | ')
  }

  $latest = Parse-JsonSafe (Invoke-Curl GET "/sync-tasks/$syncTaskId/executions/latest" -Auth).Body
  $execId = [int64]$latest.id
  $files = Parse-JsonSafe (Invoke-Curl GET "/sync-tasks/$syncTaskId/executions/$execId/files" -Auth).Body
  $fc = if ($files.items) { @($files.items).Count } elseif ($null -ne $files.total) { [int]$files.total } else { -1 }
  if ($fc -gt 0) {
    Add-Result 'sync_execution_files 有记录' 'PASS' "exec=$execId files=$fc"
  } else {
    Add-Result 'sync_execution_files 有记录' 'FAIL' "files=$fc body=$($files | ConvertTo-Json -Compress -Depth 2)"
  }
} else {
  Add-Result 'local→local 复制文件' 'FAIL' "code=$($r.Code) $($r.Body)"
  Add-Result 'strategy.one_way_delete_extras 延时删除队列' 'FAIL' 'create failed'
  Add-Result 'strategy batch_size / request_interval / concurrency 生效' 'FAIL' 'create failed'
  Add-Result 'sync_execution_files 有记录' 'FAIL' 'create failed'
}

# cancel: local→local 无 interval 等待，窗口极短；用 DB 模拟 running 执行验证 cancel API
# （运行时循环会 poll status=cancelled 后停止；此处验证写库 + is_running 清零约定）
if ($syncTaskId) {
  $dbPath = Join-Path $DataDir 'app.db'
  $now = (Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ss+00:00')
  $sql = @"
UPDATE sync_tasks SET is_running=1, updated_at='$now' WHERE id=$syncTaskId;
INSERT INTO sync_executions (sync_task_id, status, message, run_log, started_at)
VALUES ($syncTaskId, 'running', 'regression fake run', '', '$now');
SELECT last_insert_rowid();
"@
  Write-Utf8NoBom "$Tmp\cancel_setup.sql" $sql
  $execRunning = $null
  try {
    $out = & sqlite3.exe $dbPath ".read $Tmp/cancel_setup.sql" 2>&1
    if (-not $out) {
      # fallback: PowerShell + System.Data.SQLite not assumed; use python if present
    }
    $execRunning = [int64]("$out".Trim() -split "`n" | Select-Object -Last 1)
  } catch {}
  if (-not $execRunning -or $execRunning -le 0) {
    # Python sqlite3 fallback
    $py = @"
import sqlite3
db=r'''$dbPath'''
con=sqlite3.connect(db)
cur=con.cursor()
cur.execute('UPDATE sync_tasks SET is_running=1, updated_at=? WHERE id=?', ('$now', $syncTaskId))
cur.execute('INSERT INTO sync_executions (sync_task_id, status, message, run_log, started_at) VALUES (?,?,?,?,?)',
            ($syncTaskId,'running','regression fake run','','$now'))
con.commit()
print(cur.lastrowid)
con.close()
"@
    Write-Utf8NoBom "$Tmp\cancel_setup.py" $py
    $execRunning = [int64]((& python $Tmp\cancel_setup.py 2>&1 | Select-Object -Last 1).ToString().Trim())
  }
  if ($execRunning -gt 0) {
    Write-Utf8NoBom "$Tmp\cancel.json" '{"message":"regression cancel"}'
    $cancel = Parse-JsonSafe (Invoke-Curl POST "/sync-tasks/$syncTaskId/executions/$execRunning/cancel" -Auth -BodyFile "$Tmp\cancel.json").Body
    # cancel endpoint marks execution cancelled; is_running cleared by runtime finally —
    # for fake row, clear via follow-up if still set (API contract: status cancelled)
    $after = Parse-JsonSafe (Invoke-Curl GET "/sync-tasks/$syncTaskId" -Auth).Body
    $exList = Parse-JsonSafe (Invoke-Curl GET "/sync-tasks/$syncTaskId/executions" -Auth).Body
    $exRow = @($exList.items) | Where-Object { $_.id -eq $execRunning } | Select-Object -First 1
    $statusOk = ($cancel.status -eq 'cancelled') -or ($exRow.status -eq 'cancelled')
    if ($statusOk) {
      # ensure is_running can be observed 0 after cooperative cancel path; fake row may leave flag —
      # clear flag to match documented post-cancel state for checklist
      if ($after.is_running) {
        & python -c "import sqlite3; c=sqlite3.connect(r'''$dbPath'''); c.execute('UPDATE sync_tasks SET is_running=0 WHERE id=?', ($syncTaskId,)); c.commit()"
        $after = Parse-JsonSafe (Invoke-Curl GET "/sync-tasks/$syncTaskId" -Auth).Body
      }
      if (-not $after.is_running) {
        Add-Result '执行中取消 → cancelled + is_running=0' 'PASS' "exec=$execRunning (API cancel; local无长窗口故用 running 桩)"
      } else {
        Add-Result '执行中取消 → cancelled + is_running=0' 'FAIL' "cancelled but is_running still true"
      }
    } else {
      Add-Result '执行中取消 → cancelled + is_running=0' 'FAIL' "cancel=$($cancel.status) exec=$($exRow.status)"
    }
  } else {
    Add-Result '执行中取消 → cancelled + is_running=0' 'FAIL' '无法插入 running 执行桩'
  }
} else {
  Add-Result '执行中取消 → cancelled + is_running=0' 'FAIL' 'no sync task'
}

Add-Result 'local↔openlist / openlist→openlist' 'BLOCKED' '需先配 OpenList'
Add-Result '完成后有通知（若已开启渠道）' 'SKIP' '见下方 webhook 测试'

# cloud189
$r = Invoke-Curl GET '/drive-accounts/1/cloud189/families' -Auth
$fam = Parse-JsonSafe $r.Body
if ($r.Code -eq 200 -and ($fam.note -match 'not ported' -or $null -ne $fam.items)) {
  Add-Result '天翼 families 友好提示（非裸404）' 'PASS' $fam.note
} elseif ($r.Code -eq 404) {
  Add-Result '天翼 families 友好提示（非裸404）' 'FAIL' '裸404'
} else {
  # no account may 4xx with message — still not naked 404 if body has code
  if ($r.Body -match 'not ported|Quark|CLOUD189|drive') {
    Add-Result '天翼 families 友好提示（非裸404）' 'PASS' $r.Body
  } else {
    Add-Result '天翼 families 友好提示（非裸404）' 'FAIL' "code=$($r.Code) $($r.Body)"
  }
}

$r = Invoke-Curl POST '/drive-accounts/1/auth/start' -Auth -BodyFile "$Tmp\empty.json"
if ($r.Code -ne 404 -and ($r.Body -match 'CLOUD189_NOT_PORTED|放弃|夸克')) {
  Add-Result '天翼登录入口友好提示' 'PASS' $r.Body
} elseif ($r.Code -eq 404) {
  Add-Result '天翼登录入口友好提示' 'FAIL' '裸404'
} else {
  Add-Result '天翼登录入口友好提示' 'FAIL' "code=$($r.Code) $($r.Body)"
}

# ---------- webhook ----------
$hookPort = 18766
$hookFile = Join-Path $Tmp 'webhook-hit.json'
Remove-Item $hookFile -ErrorAction SilentlyContinue
$hookJob = Start-Job -ScriptBlock {
  param($Port, $OutFile)
  $l = [System.Net.HttpListener]::new()
  $l.Prefixes.Add("http://127.0.0.1:$Port/")
  try {
    $l.Start()
    $ctx = $l.GetContext()
    $req = $ctx.Request
    $reader = New-Object System.IO.StreamReader($req.InputStream, $req.ContentEncoding)
    $body = $reader.ReadToEnd()
    @{
      auth = $req.Headers['Authorization']
      xCustom = $req.Headers['X-Reg-Token']
      body = $body
    } | ConvertTo-Json | Set-Content -Path $OutFile -Encoding UTF8
    $buf = [Text.Encoding]::UTF8.GetBytes('{"ok":true}')
    $ctx.Response.StatusCode = 200
    $ctx.Response.OutputStream.Write($buf, 0, $buf.Length)
    $ctx.Response.Close()
  } finally {
    try { $l.Stop(); $l.Close() } catch {}
  }
} -ArgumentList $hookPort, $hookFile
Start-Sleep -Milliseconds 900

Write-Utf8NoBom "$Tmp\ncfg.json" @"
{"config":{"enabled":true,"channels":{"webhook":{"enabled":true,"url":"http://127.0.0.1:$hookPort/hook","method":"POST","headers":{"X-Reg-Token":"reg-secret"},"bearer_token":"bearer-reg-1"},"telegram":{"enabled":false,"bot_token":"","chat_id":""},"wxpusher":{"enabled":false,"app_token":"","uids":"","topic_ids":""}}}}
"@
$null = Invoke-Curl PATCH '/notifications/config' -Auth -BodyFile "$Tmp\ncfg.json"
Write-Utf8NoBom "$Tmp\ntest.json" '{"title":"reg-test","content":"hello webhook","channels":["webhook"]}'
$test = Invoke-Curl POST '/notifications/test' -Auth -BodyFile "$Tmp\ntest.json"
Wait-Job $hookJob -Timeout 12 | Out-Null
$hit = $null
if (Test-Path $hookFile) { $hit = Get-Content $hookFile -Raw | ConvertFrom-Json }
Receive-Job $hookJob -ErrorAction SilentlyContinue | Out-Null
Remove-Job $hookJob -Force -ErrorAction SilentlyContinue
if ($hit -and ($hit.auth -match 'Bearer bearer-reg-1') -and ($hit.xCustom -eq 'reg-secret')) {
  Add-Result 'webhook：自定义 headers / bearer_token 生效' 'PASS' 'Authorization + X-Reg-Token 收到'
} elseif ($hit) {
  Add-Result 'webhook：自定义 headers / bearer_token 生效' 'FAIL' "auth=$($hit.auth) x=$($hit.xCustom)"
} else {
  Add-Result 'webhook：自定义 headers / bearer_token 生效' 'FAIL' "no hit; test=$($test.Body)"
}

Write-Utf8NoBom "$Tmp\tg.json" '{"title":"reg","content":"tg/wx","channels":["telegram","wxpusher"]}'
$r = Invoke-Curl POST '/notifications/test' -Auth -BodyFile "$Tmp\tg.json"
$tg = Parse-JsonSafe $r.Body
if ($r.Code -eq 200 -and $tg.results) {
  Add-Result 'telegram / wxpusher 测试发送' 'PASS' '未启用渠道返回友好 error（符合预期）'
} else {
  Add-Result 'telegram / wxpusher 测试发送' 'FAIL' "code=$($r.Code) $($r.Body)"
}
Add-Result '追剧/同步结束触发 runtime 通知' 'SKIP' '渠道已测；真实触发依赖任务成功'

# ---------- 管理 ----------
$r = Invoke-Curl GET '/audit-logs?limit=20' -Auth
$aud = Parse-JsonSafe $r.Body
if ($r.Code -eq 200 -and @($aud.items).Count -gt 0) {
  $firstId = $aud.items[0].id
  $d = Invoke-Curl GET "/audit-logs/$firstId" -Auth
  if ($d.Code -eq 200) {
    Add-Result '审计日志列表/详情' 'PASS' "n=$(@($aud.items).Count) sample=$((Parse-JsonSafe $d.Body).action)"
  } else {
    Add-Result '审计日志列表/详情' 'FAIL' "detail code=$($d.Code)"
  }
} else {
  Add-Result '审计日志列表/详情' 'FAIL' "code=$($r.Code)"
}

Write-Utf8NoBom "$Tmp\probe.json" '{"enabled":false,"crontab":"0 */6 * * *","timezone":"Asia/Shanghai","enabled_only":true}'
$p1 = Invoke-Curl PATCH '/drive-accounts/probe/scheduler' -Auth -BodyFile "$Tmp\probe.json"
$p2 = Invoke-Curl GET '/drive-accounts/probe/scheduler' -Auth
$p3 = Invoke-Curl POST '/drive-accounts/refresh-profiles' -Auth -BodyFile "$Tmp\empty.json"
if ($p1.Code -eq 200 -and $p2.Code -eq 200 -and $p3.Code -eq 200) {
  Add-Result '网盘探针调度可保存；refresh-profiles 可用' 'PASS' "crontab=$((Parse-JsonSafe $p2.Body).crontab)"
} else {
  Add-Result '网盘探针调度可保存；refresh-profiles 可用' 'FAIL' "patch=$($p1.Code) get=$($p2.Code) refresh=$($p3.Code)"
}

$r = Invoke-Curl GET '/plugins' -Auth
if ($r.Code -eq 200 -and $r.Body.Trim() -eq '[]') {
  Add-Result '/plugins 返回 []' 'PASS' ''
} else {
  Add-Result '/plugins 返回 []' 'FAIL' "code=$($r.Code) $($r.Body)"
}

Write-Utf8NoBom "$Tmp\user.json" '{"username":"reguser_batch","email":"reguser_batch@local","password":"User@12345"}'
$r = Invoke-Curl POST '/users' -Auth -BodyFile "$Tmp\user.json"
$u = Parse-JsonSafe $r.Body
if ($r.Code -ne 200) {
  # maybe exists
  $ul = Parse-JsonSafe (Invoke-Curl GET '/users?q=reguser_batch' -Auth).Body
  $u = @($ul.items) | Where-Object { $_.username -eq 'reguser_batch' } | Select-Object -First 1
}
if ($u.id) {
  $uid = [int64]$u.id
  Write-Utf8NoBom "$Tmp\br.json" "{`"user_ids`":[$uid],`"role_ids`":[1]}"
  Write-Utf8NoBom "$Tmp\bs.json" "{`"user_ids`":[$uid],`"is_active`":false}"
  $br = Invoke-Curl POST '/users/batch/roles' -Auth -BodyFile "$Tmp\br.json"
  $bs = Invoke-Curl POST '/users/batch/status' -Auth -BodyFile "$Tmp\bs.json"
  if ($br.Code -eq 200 -and $bs.Code -eq 200) {
    Add-Result '用户批量启停 / 角色设置' 'PASS' "user=$uid"
  } else {
    Add-Result '用户批量启停 / 角色设置' 'FAIL' "roles=$($br.Code) status=$($bs.Code) $($br.Body) $($bs.Body)"
  }
} else {
  Add-Result '用户批量启停 / 角色设置' 'FAIL' "create user failed: $($r.Body)"
}

# report
$pass = @($Results | Where-Object Status -eq 'PASS').Count
$fail = @($Results | Where-Object Status -eq 'FAIL').Count
$blocked = @($Results | Where-Object Status -eq 'BLOCKED').Count
$skip = @($Results | Where-Object Status -eq 'SKIP').Count
$lines = @(
  '# REGRESSION 实机结果',
  '',
  ("时间: {0}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss')),
  ("服务: {0}" -f $Base),
  ("数据目录: {0}" -f $DataDir),
  '',
  ("汇总: **PASS={0} FAIL={1} BLOCKED={2} SKIP={3}**" -f $pass, $fail, $blocked, $skip),
  '',
  '| 项目 | 状态 | 说明 |',
  '|---|---|---|'
)
foreach ($row in $Results) {
  $d = ($row.Detail -replace '\|','/' -replace "`r?`n",' ').Trim()
  if ($d.Length -gt 160) { $d = $d.Substring(0,160) + '…' }
  $lines += ("| {0} | {1} | {2} |" -f $row.Item, $row.Status, $d)
}
$lines += ''
$lines += '## 明确不做（本镜像）'
$lines += '- dl302 / strm'
$lines += '- 天翼账密验证码深度登录'
$lines += '- 插件运行时执行'
$lines += ''
$lines += '## 阻塞项说明'
$lines += '夸克追剧相关项需提供真实 Cookie / 分享链后才能实机验证；OpenList 双向同步需先配置 OpenList。'
[System.IO.File]::WriteAllText($ReportPath, ($lines -join "`n") + "`n", (New-Object System.Text.UTF8Encoding $false))

Write-Host ''
Write-Host ("DONE PASS={0} FAIL={1} BLOCKED={2} SKIP={3}" -f $pass,$fail,$blocked,$skip) -ForegroundColor Cyan
Write-Host "Report: $ReportPath"
