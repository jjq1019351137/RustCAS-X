# Rust migration plan: backend (Python)

4 packages, 286 source files (45606 lines), 0 test files, 0 UI files, grouped into 129 units.

| Ready | Caution | Blocked | Keep in source language |
| ---: | ---: | ---: | ---: |
| 54 | 45 | 30 | 0 |

## Start here

Best first ports: safe to port (readiness) and worth porting (loops and string work, tests, size).

| # | Unit | Lines | Priority | Readiness | Tests | Bridge | Main risks |
| ---: | --- | ---: | ---: | ---: | ---: | --- | --- |
| 1 | `app/services/sync_execution_cleanup.py` | 39 | 74 | 89 | 0 | PyO3 | String offsets and slicing (1); Hash iteration order (1) |
| 2 | `app/services/title_vector_match.py` | 99 | 74 | 78 | 0 | PyO3 | String offsets and slicing (4); Hash iteration order (2) |
| 3 | `app/services/openlist_cleanup.py` | 122 | 65 | 71 | 0 | PyO3 | String offsets and slicing (4); Hash iteration order (3); Dynamic typing (4) |
| 4 | `app/services/audit_logs.py` | 60 | 62 | 95 | 0 | PyO3 | Number semantics (7) |
| 5 | `app/services/magic_regex.py` | 146 | 62 | 79 | 0 | PyO3 | Regexes built at runtime (1); Hash iteration order (1); Exceptions (7) |
| 6 | `app/services/resource_search.py` | 319 | 60 | 59 | 0 | PyO3 | Hash iteration order (5); String offsets and slicing (1); Dynamic typing (4) |
| 7 | `app/services/drive_account_signin_jobs.py` | 106 | 58 | 88 | 0 | PyO3 | Dynamic typing (1); Concurrency (1); Number semantics (3) |
| 8 | `app/services/users.py` | 61 | 58 | 87 | 0 | PyO3 | Hash iteration order (2); Exceptions (5); Number semantics (3) |
| 9 | `app/services/drive_browse.py` | 226 | 58 | 79 | 0 | PyO3 | String offsets and slicing (2); Exceptions (6); Number semantics (6) |
| 10 | `app/scripts/` | 185 | 57 | 89 | 0 | PyO3 | Hash iteration order (2) |

### `app/services/sync_execution_cleanup.py`

39 lines in 1 file, wave 1, priority 74, readiness 89, bridge **PyO3**.

- Imports `app/models/`: port it first, or port them together.
- No tests found: write characterization tests against the original code first.
- String offsets and slicing x1: `app/services/sync_execution_cleanup.py:32`
- Hash iteration order x1: `app/services/sync_execution_cleanup.py:32`

### `app/services/title_vector_match.py`

99 lines in 1 file, wave 1, priority 74, readiness 78, bridge **PyO3**.

- Imports `app/services/title_match.py`: port it first, or port them together.
- No tests found: write characterization tests against the original code first.
- String offsets and slicing x4: `app/services/title_vector_match.py:29`, `app/services/title_vector_match.py:87`, `app/services/title_vector_match.py:95`
- Hash iteration order x2: `app/services/title_vector_match.py:22`, `app/services/title_vector_match.py:46`

### `app/services/openlist_cleanup.py`

122 lines in 1 file, wave 3, priority 65, readiness 71, bridge **PyO3**.

- Imports `app/services/openlist_client_factory.py`: port it first, or port them together.
- No tests found: write characterization tests against the original code first.
- String offsets and slicing x4: `app/services/openlist_cleanup.py:117`, `app/services/openlist_cleanup.py:126`, `app/services/openlist_cleanup.py:130`
- Hash iteration order x3: `app/services/openlist_cleanup.py:15`, `app/services/openlist_cleanup.py:81`, `app/services/openlist_cleanup.py:95`
- Dynamic typing x4: `app/services/openlist_cleanup.py:39`, `app/services/openlist_cleanup.py:41`, `app/services/openlist_cleanup.py:43`
- Exceptions x3: `app/services/openlist_cleanup.py:35`, `app/services/openlist_cleanup.py:71`, `app/services/openlist_cleanup.py:127`
- Number semantics x1: `app/services/openlist_cleanup.py:29`

### `app/services/audit_logs.py`

60 lines in 1 file, wave 1, priority 62, readiness 95, bridge **PyO3**.

- Imports `app/models/`: port it first, or port them together.
- No tests found: write characterization tests against the original code first.
- Number semantics x7: `app/services/audit_logs.py:9`, `app/services/audit_logs.py:20`, `app/services/audit_logs.py:21`

### `app/services/magic_regex.py`

146 lines in 1 file, wave 4, priority 62, readiness 79, bridge **PyO3**.

- Import cycle with app/services/tasks.py, app/services/task_bundle_migration.py, app/services/drive_browse.py, app/api/routes/tasks.py, app/services/telegram_bot/, app/services/task_savepath_snapshot.py, app/services/sync_tasks.py, app/services/sync_execution_recovery.py, app/services/drama_share_repair.py, app/services/task_share_links.py, app/services/share_preview_batch.py, app/services/resource_search.py, app/services/drama_share_autoupdate.py, app/services/sync_task_triggers.py, app/services/dl302_strm.py, app/services/drive_account_lsdir_scan.py, app/services/drive_accounts.py, app/services/dl302_settings.py, app/services/dl302_cas.py, app/services/drama_linked_pipeline.py, app/services/deferred_sync_delete.py, app/extensions/runtime/: port them together.
- Imports `app/core/`, `app/extensions/runtime/`, `app/models/`: port them first, or port them together.
- No tests found: write characterization tests against the original code first.
- Regexes built at runtime x1: `app/services/magic_regex.py:32`
- Hash iteration order x1: `app/services/magic_regex.py:49`
- Exceptions x7: `app/services/magic_regex.py:31`, `app/services/magic_regex.py:34`, `app/services/magic_regex.py:120`
- Number semantics x1: `app/services/magic_regex.py:37`

## Order by package

Packages come after the workspace packages they use. Inside a package, port by wave: a unit's imports are in earlier waves.

### 1. root (`.`, portable, 0 test files)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `app/extensions/` | ready | 0 | 100 | PyO3 | - |
| 0 | `app/extensions/sync_plugins/` | ready | 0 | 100 | PyO3 | - |
| 0 | `app/db/repositories/` | caution | 106 | 82 | PyO3 | Class inheritance (16); Dynamic typing (2) |
| 0 | `app/db/drivers/` | ready | 181 | 79 | PyO3 | Hash iteration order (1); Class inheritance (3) |
| 0 | `app/extensions/plugins/` | caution | 373 | 71 | PyO3 | Dynamic typing (21); Integer width and overflow (1) |
| 0 | `app/db/` | ready | 160 | 68 | PyO3 | Generators (8); Dynamic typing (1) |
| 0 | `app/thirdparty/dl302_rpc/` | ready | 770 | 64 | PyO3 | Dynamic typing (34); Reflection and prototypes (1) |
| 0 | `app/models/` | caution | 722 | 51 | PyO3 | Class inheritance (35); Number semantics (115) |
| 0 | `app/core/` | blocked | 485 | 19 | PyO3 | Hash iteration order (3); Dynamic typing (9) |
| 0 | `app/extensions/adapters/` | caution | 4225 | 0 | PyO3 | String offsets and slicing (21); Hash iteration order (9) |
| 1 | `alembic/` | ready | 48 | 97 | PyO3 | Dynamic typing (1) |
| 1 | `app/middlewares/` | caution | 83 | 70 | PyO3 | String offsets and slicing (3); Hash iteration order (1) |
| 1 | `scripts/` | ready | 247 | 67 | PyO3 | String offsets and slicing (3); Hash iteration order (2) |
| 1 | `app/thirdparty/` | caution | 1282 | 8 | PyO3 | String offsets and slicing (9); Hash iteration order (2) |
| 2 | `app/scripts/` | ready | 185 | 89 | PyO3 | Hash iteration order (2) |
| 4 | `app/extensions/runtime/` | blocked | 8520 | 0 | PyO3 | Hash iteration order (67); String offsets and slicing (32) |
| 5 | `alembic/versions/` | caution | 2285 | 4 | PyO3 | Hash iteration order (82); Dynamic typing (9) |
| 7 | `app/api/` | blocked | 26 | 86 | PyO3 | - |
| 8 | `app/` | blocked | 93 | 71 | PyO3 | Hash iteration order (1); Dynamic typing (2) |

### 2. app/api/routes (`app/api/routes`, portable, 0 test files)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `app/api/routes/__init__.py` | ready | 1 | 98 | PyO3 | - |
| 1 | `app/api/routes/metrics.py` | blocked | 6 | 86 | PyO3 | - |
| 1 | `app/api/routes/health.py` | blocked | 24 | 85 | PyO3 | Exceptions (1); Clock and random numbers (1) |
| 2 | `app/api/routes/permissions.py` | blocked | 12 | 86 | PyO3 | - |
| 2 | `app/api/routes/system_settings.py` | blocked | 37 | 86 | PyO3 | - |
| 2 | `app/api/routes/tmdb_settings.py` | blocked | 37 | 86 | PyO3 | - |
| 2 | `app/api/routes/auth.py` | blocked | 69 | 85 | PyO3 | - |
| 2 | `app/api/routes/notifications.py` | blocked | 87 | 83 | PyO3 | Exceptions (1); Clock and random numbers (1) |
| 2 | `app/api/routes/audit_logs.py` | blocked | 68 | 82 | PyO3 | Number semantics (5); Exceptions (1) |
| 2 | `app/api/routes/roles.py` | blocked | 103 | 81 | PyO3 | Dynamic typing (2) |
| 3 | `app/api/routes/users.py` | blocked | 135 | 79 | PyO3 | Dynamic typing (2); Number semantics (1) |
| 3 | `app/api/routes/openlist_settings.py` | blocked | 116 | 66 | PyO3 | String offsets and slicing (2); Hash iteration order (1) |
| 4 | `app/api/routes/media_discovery.py` | blocked | 124 | 79 | PyO3 | Dynamic typing (2); Exceptions (1) |
| 4 | `app/api/routes/tasks.py` | blocked | 1781 | 0 | PyO3 | String offsets and slicing (9); Hash iteration order (9) |
| 5 | `app/api/routes/dashboard.py` | blocked | 18 | 86 | PyO3 | - |
| 5 | `app/api/routes/resource_search.py` | blocked | 34 | 86 | PyO3 | - |
| 5 | `app/api/routes/magic_regex.py` | blocked | 51 | 85 | PyO3 | - |
| 5 | `app/api/routes/cache_management.py` | blocked | 216 | 76 | PyO3 | Dynamic typing (2); Number semantics (7) |
| 5 | `app/api/routes/dl302_settings.py` | blocked | 273 | 72 | PyO3 | Hash iteration order (1); Dynamic typing (1) |
| 5 | `app/api/routes/tmdb_cache.py` | blocked | 392 | 66 | PyO3 | String offsets and slicing (1); Dynamic typing (2) |
| 5 | `app/api/routes/sync_tasks.py` | blocked | 733 | 23 | PyO3 | String offsets and slicing (5); Hash iteration order (1) |
| 6 | `app/api/routes/sync_plugins.py` | blocked | 73 | 82 | PyO3 | Dynamic typing (3) |
| 6 | `app/api/routes/plugins.py` | blocked | 55 | 81 | PyO3 | Dynamic typing (3) |
| 6 | `app/api/routes/drive_accounts.py` | blocked | 663 | 47 | PyO3 | String offsets and slicing (1); Hash iteration order (1) |

### 3. app/schemas (`app/schemas`, portable, 0 test files)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `app/schemas/common.py` | caution | 5 | 96 | PyO3 | Class inheritance (1) |
| 0 | `app/schemas/drive_account_auth.py` | caution | 6 | 95 | PyO3 | Class inheritance (2) |
| 0 | `app/schemas/auth.py` | caution | 13 | 94 | PyO3 | Class inheritance (3) |
| 0 | `app/schemas/openlist_settings.py` | caution | 8 | 94 | PyO3 | Class inheritance (2); Number semantics (3) |
| 0 | `app/schemas/sync_execution_files.py` | caution | 18 | 94 | PyO3 | Class inheritance (2); Number semantics (2) |
| 0 | `app/schemas/system_settings.py` | caution | 6 | 94 | PyO3 | Class inheritance (2); Number semantics (1) |
| 0 | `app/schemas/task_magic_regex.py` | caution | 9 | 94 | PyO3 | Class inheritance (2); Number semantics (1) |
| 0 | `app/schemas/drive_account_probe_scheduler.py` | caution | 12 | 93 | PyO3 | Class inheritance (2); Number semantics (4) |
| 0 | `app/schemas/task_bundle.py` | caution | 13 | 93 | PyO3 | Class inheritance (2) |
| 0 | `app/schemas/task_repair.py` | caution | 15 | 93 | PyO3 | Class inheritance (2); Number semantics (6) |
| 0 | `app/schemas/tmdb_settings.py` | caution | 16 | 93 | PyO3 | Class inheritance (2); Number semantics (6) |
| 0 | `app/schemas/audit_log.py` | caution | 32 | 91 | PyO3 | Class inheritance (3); Number semantics (10) |
| 0 | `app/schemas/notification.py` | caution | 25 | 91 | PyO3 | Class inheritance (6); Number semantics (3) |
| 0 | `app/schemas/plugin.py` | caution | 25 | 91 | PyO3 | Class inheritance (2); Number semantics (6) |
| 0 | `app/schemas/task_scheduler.py` | caution | 16 | 91 | PyO3 | Class inheritance (2); Number semantics (6) |
| 0 | `app/schemas/path_browse.py` | caution | 23 | 90 | PyO3 | Class inheritance (4); Number semantics (2) |
| 0 | `app/schemas/role.py` | caution | 18 | 90 | PyO3 | Class inheritance (5); Number semantics (4) |
| 0 | `app/schemas/magic_regex_rule.py` | ready | 31 | 89 | PyO3 | Class inheritance (4); Number semantics (7) |
| 0 | `app/schemas/cache_management.py` | caution | 52 | 87 | PyO3 | Class inheritance (11); Number semantics (10) |
| 0 | `app/schemas/media_discovery.py` | caution | 57 | 87 | PyO3 | Class inheritance (10); Number semantics (22) |
| 0 | `app/schemas/drive_account.py` | caution | 88 | 86 | PyO3 | Class inheritance (10); Number semantics (25) |
| 0 | `app/schemas/resource_search.py` | caution | 46 | 86 | PyO3 | Class inheritance (8); Number semantics (21) |
| 0 | `app/schemas/sync_task.py` | caution | 88 | 86 | PyO3 | Class inheritance (9); Number semantics (19) |
| 0 | `app/schemas/tmdb_cache.py` | caution | 88 | 85 | PyO3 | Class inheritance (15); Number semantics (28) |
| 0 | `app/schemas/dl302_settings.py` | caution | 153 | 83 | PyO3 | Class inheritance (16); Number semantics (24) |
| 0 | `app/schemas/task.py` | caution | 147 | 81 | PyO3 | Class inheritance (15); Number semantics (57) |
| 0 | `app/schemas/task_browse.py` | caution | 93 | 72 | PyO3 | Class inheritance (13); Number semantics (50) |
| 1 | `app/schemas/user.py` | caution | 35 | 90 | PyO3 | Class inheritance (8); Exceptions (1) |
| 1 | `app/schemas/dashboard.py` | caution | 71 | 88 | PyO3 | Class inheritance (7); Number semantics (17) |

### 4. app/services (`app/services`, portable, 0 test files)

| Wave | Unit | Status | Lines | Readiness | Bridge | Main risks |
| ---: | --- | --- | ---: | ---: | --- | --- |
| 0 | `app/services/drive_account_auth_sessions.py` | ready | 53 | 92 | PyO3 | Clock and random numbers (3); Number semantics (1) |
| 0 | `app/services/__init__.py` | ready | 1 | 85 | PyO3 | - |
| 0 | `app/services/title_match.py` | caution | 240 | 37 | PyO3 | Regex features the `regex` crate lacks (8); Hash iteration order (7) |
| 0 | `app/services/resource_search_providers.py` | caution | 463 | 35 | PyO3 | Hash iteration order (7); String offsets and slicing (4) |
| 0 | `app/services/proxy_image_cache.py` | blocked | 340 | 33 | PyO3 | String offsets and slicing (5); Hash iteration order (1) |
| 1 | `app/services/permissions.py` | ready | 5 | 98 | PyO3 | - |
| 1 | `app/services/audit_logs.py` | ready | 60 | 95 | PyO3 | Number semantics (7) |
| 1 | `app/services/openlist_settings.py` | ready | 31 | 93 | PyO3 | Dynamic typing (2) |
| 1 | `app/services/system_settings.py` | ready | 39 | 93 | PyO3 | Dynamic typing (1); Exceptions (1) |
| 1 | `app/services/roles.py` | ready | 38 | 91 | PyO3 | Hash iteration order (1); Exceptions (5) |
| 1 | `app/services/sync_execution_cleanup.py` | ready | 39 | 89 | PyO3 | String offsets and slicing (1); Hash iteration order (1) |
| 1 | `app/services/scheduler_validation.py` | ready | 45 | 87 | PyO3 | Exceptions (6); Number semantics (4) |
| 1 | `app/services/share_preview_batch_cache.py` | ready | 98 | 87 | PyO3 | Dynamic typing (4); Number semantics (6) |
| 1 | `app/services/drama_update_progress.py` | ready | 100 | 85 | PyO3 | Dynamic typing (8); Number semantics (8) |
| 1 | `app/services/start_episode_filter.py` | ready | 88 | 84 | PyO3 | Regex features the `regex` crate lacks (1); Number semantics (12) |
| 1 | `app/services/drive_account_lsdir_static_state.py` | ready | 209 | 83 | PyO3 | Dynamic typing (2); Number semantics (10) |
| 1 | `app/services/tmdb_settings.py` | ready | 81 | 83 | PyO3 | Dynamic typing (1); Exceptions (1) |
| 1 | `app/services/invalid_share_links.py` | ready | 73 | 80 | PyO3 | Hash iteration order (2); Dynamic typing (3) |
| 1 | `app/services/title_vector_match.py` | ready | 99 | 78 | PyO3 | String offsets and slicing (4); Hash iteration order (2) |
| 1 | `app/services/auth.py` | blocked | 102 | 77 | PyO3 | Exceptions (7); Number semantics (2) |
| 1 | `app/services/drive_account_lsdir_cache.py` | ready | 459 | 62 | PyO3 | Hash iteration order (3); Dynamic typing (15) |
| 1 | `app/services/audit.py` | ready | 333 | 57 | PyO3 | String offsets and slicing (2); Dynamic typing (5) |
| 1 | `app/services/notifications/` | caution | 427 | 46 | PyO3 | Hash iteration order (6); String offsets and slicing (4) |
| 2 | `app/services/tmdb_cache_scheduler.py` | ready | 39 | 91 | PyO3 | Dynamic typing (2); Number semantics (6) |
| 2 | `app/services/drive_account_probe_scheduler.py` | ready | 33 | 90 | PyO3 | Dynamic typing (2); Number semantics (4) |
| 2 | `app/services/openlist_client_factory.py` | ready | 14 | 90 | PyO3 | Dynamic typing (2); Exceptions (1) |
| 2 | `app/services/users.py` | ready | 61 | 87 | PyO3 | Hash iteration order (2); Exceptions (5) |
| 2 | `app/services/task_scheduler.py` | ready | 54 | 86 | PyO3 | Dynamic typing (4); Number semantics (6) |
| 2 | `app/services/drive_account_lsdir_refresh_status.py` | ready | 288 | 71 | PyO3 | Hash iteration order (3); String offsets and slicing (1) |
| 2 | `app/services/tmdb_cache.py` | caution | 679 | 24 | PyO3 | Hash iteration order (7); String offsets and slicing (6) |
| 3 | `app/services/dashboard_drama.py` | ready | 177 | 81 | PyO3 | Hash iteration order (1); Dynamic typing (4) |
| 3 | `app/services/openlist_cleanup.py` | ready | 122 | 71 | PyO3 | String offsets and slicing (4); Hash iteration order (3) |
| 3 | `app/services/media_discovery.py` | ready | 210 | 64 | PyO3 | String offsets and slicing (2); Hash iteration order (2) |
| 4 | `app/services/drive_browse.py` | ready | 226 | 79 | PyO3 | String offsets and slicing (2); Exceptions (6) |
| 4 | `app/services/magic_regex.py` | ready | 146 | 79 | PyO3 | Regexes built at runtime (1); Hash iteration order (1) |
| 4 | `app/services/sync_execution_recovery.py` | ready | 147 | 70 | PyO3 | Hash iteration order (4); Dynamic typing (14) |
| 4 | `app/services/task_savepath_snapshot.py` | ready | 308 | 70 | PyO3 | Hash iteration order (2); Dynamic typing (7) |
| 4 | `app/services/deferred_sync_delete.py` | ready | 332 | 65 | PyO3 | String offsets and slicing (1); Hash iteration order (1) |
| 4 | `app/services/tasks.py` | ready | 261 | 65 | PyO3 | Hash iteration order (3); String offsets and slicing (1) |
| 4 | `app/services/sync_task_triggers.py` | ready | 351 | 64 | PyO3 | String offsets and slicing (1); Hash iteration order (1) |
| 4 | `app/services/drama_linked_pipeline.py` | ready | 412 | 60 | PyO3 | Hash iteration order (11); Dynamic typing (17) |
| 4 | `app/services/resource_search.py` | ready | 319 | 59 | PyO3 | Hash iteration order (5); String offsets and slicing (1) |
| 4 | `app/services/sync_tasks.py` | ready | 316 | 58 | PyO3 | Hash iteration order (8); String offsets and slicing (2) |
| 4 | `app/services/task_bundle_migration.py` | ready | 435 | 58 | PyO3 | Hash iteration order (10); Dynamic typing (18) |
| 4 | `app/services/drive_accounts.py` | ready | 518 | 57 | PyO3 | Hash iteration order (1); Dynamic typing (17) |
| 4 | `app/services/drama_share_autoupdate.py` | ready | 483 | 56 | PyO3 | String offsets and slicing (2); Hash iteration order (2) |
| 4 | `app/services/task_share_links.py` | ready | 278 | 54 | PyO3 | Hash iteration order (6); String offsets and slicing (1) |
| 4 | `app/services/dl302_cas.py` | caution | 538 | 50 | PyO3 | Hash iteration order (4); String offsets and slicing (1) |
| 4 | `app/services/share_preview_batch.py` | caution | 433 | 49 | PyO3 | String offsets and slicing (2); Hash iteration order (1) |
| 4 | `app/services/dl302_settings.py` | caution | 578 | 45 | PyO3 | Hash iteration order (11); Dynamic typing (19) |
| 4 | `app/services/drama_share_repair.py` | caution | 573 | 40 | PyO3 | Regexes built at runtime (4); Hash iteration order (4) |
| 4 | `app/services/dl302_strm.py` | blocked | 497 | 34 | PyO3 | Hash iteration order (13); String offsets and slicing (2) |
| 4 | `app/services/drive_account_lsdir_scan.py` | caution | 1175 | 32 | PyO3 | Hash iteration order (4); Dynamic typing (26) |
| 4 | `app/services/telegram_bot/` | caution | 5856 | 0 | PyO3 | String offsets and slicing (62); Hash iteration order (31) |
| 5 | `app/services/plugins.py` | ready | 55 | 95 | PyO3 | Exceptions (2); Number semantics (2) |
| 5 | `app/services/sync_plugins.py` | ready | 55 | 95 | PyO3 | Exceptions (2); Number semantics (2) |
| 5 | `app/services/drive_account_signin_jobs.py` | ready | 106 | 88 | PyO3 | Dynamic typing (1); Concurrency (1) |

## PyPI dependencies

| Package | Units | Rust | Fit | Note |
| --- | ---: | --- | --- | --- |
| `fastapi` | 29 | - | none | Web framework: keep the app in Python. |
| `starlette` | 2 | - | none | Web framework: keep the app in Python. |
| `apscheduler` | 3 | ? | unknown |  |
| `cachetools` | 3 | ? | unknown |  |
| `emoji` | 1 | ? | unknown |  |
| `guessit` | 1 | ? | unknown |  |
| `natsort` | 1 | ? | unknown |  |
| `pydantic-settings` | 1 | ? | unknown |  |
| `pymysql` | 1 | ? | unknown |  |
| `rsa` | 1 | ? | unknown |  |
| `treelib` | 1 | ? | unknown |  |
| `sqlalchemy` | 74 | sqlx | partial | ORM layers port by hand. |
| `pydantic` | 30 | serde, garde | partial | Validation errors will differ. |
| `requests` | 10 | reqwest | good |  |
| `grpc` | 5 | tonic | good |  |
| `bcrypt` | 1 | argon2, bcrypt | good |  |
| `google.protobuf` | 1 | prost | good |  |
| `jose` | 1 | jsonwebtoken | good |  |
| `urllib3` | 1 | reqwest | good |  |

## Blocked

- `app/`: `fastapi` has no Rust equivalent.
- `app/api/`: `fastapi` has no Rust equivalent.
- `app/api/routes/audit_logs.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/auth.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/cache_management.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/dashboard.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/dl302_settings.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/drive_accounts.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/health.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/magic_regex.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/media_discovery.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/metrics.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/notifications.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/openlist_settings.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/permissions.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/plugins.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/resource_search.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/roles.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/sync_plugins.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/sync_tasks.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/system_settings.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/tasks.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/tmdb_cache.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/tmdb_settings.py`: `fastapi` has no Rust equivalent.
- `app/api/routes/users.py`: `fastapi` has no Rust equivalent.
- `app/core/`: `fastapi` has no Rust equivalent.
- `app/extensions/runtime/`: Uses dynamic code (eval, new Function or computed imports).
- `app/services/auth.py`: `fastapi` has no Rust equivalent.
- `app/services/dl302_strm.py`: `fastapi` has no Rust equivalent.
- `app/services/proxy_image_cache.py`: `fastapi` has no Rust equivalent.

## Risk guide

- **String offsets and slicing** (High): Rust slices strings at UTF-8 byte offsets that fall on character boundaries. Convert offsets from the source's indexing units as needed and test with non-ASCII text.
- **Regex features the `regex` crate lacks** (High): Lookaround, backreferences and the `y` flag are not in the `regex` crate. Use `fancy-regex` for those patterns, or rewrite them, and test them on non-ASCII text: `\w`, `\b` and case-insensitive matching differ between engines even where a pattern compiles. JS `/i` without `u` never matches a non-ASCII letter to an ASCII one, Rust `(?i)` does (`ſ` matches `s`, the Kelvin sign `K` matches `k`): write `(?i-u)` or compare with `eq_ignore_ascii_case`.
- **Regexes built at runtime** (High): Patterns come from data or config in JS syntax. Rust regex syntax differs, so user-visible behavior can change. Pin it with tests on real patterns.
- **Dynamic code (eval, new Function, computed import)** (High): eval, new Function and computed imports have no Rust equivalent. Keep this part in JS or replace it with an explicit registry.
- **Dynamic typing** (Medium): Values whose shape is checked at runtime need an explicit Rust enum or `serde_json::Value`.
- **Exceptions** (Low): Represent source exceptions with `Result`. Preserve the original exception type and message when errors cross the bridge.
- **Async and promises** (Medium): Promises become futures. Bridge with napi-rs `async fn` or `wasm-bindgen-futures`, or keep the async shell in JS and port the sync core.
- **Generators** (Medium): Lazy generators become iterators, or return a collected Vec across the bridge.
- **Class inheritance** (Medium): Class hierarchies become traits plus composition; `instanceof` checks become enums.
- **Reflection and prototypes** (Medium): Proxy, Reflect, Symbol and prototype tricks need a redesign, not a translation.
- **Number semantics** (Low): JS numbers are f64; bitwise ops truncate to i32; parseInt is lenient. Pick Rust integer types deliberately. Printing differs: `${n}` gives `1e+21`, `1e-7` and `0` for -0 where Rust's `{}` does not, so use `js::number_to_string`. `Math.round` rounds halves up (-2.5 gives -2), Rust's `round` away from zero: use `js::math_round`. `toFixed` rounds exact halves up ((2.5).toFixed(0) is "3"), Rust's `{:.0}` to even.
- **Integer width and overflow** (Medium): Python ints never overflow; C, Go and Java ints wrap or are undefined on overflow. Pick i64, i128 or a bignum crate deliberately, and use wrapping_* where the source wraps.
- **Division and modulo** (Medium): Python `//` and `%` floor toward negative infinity; Rust `/` and `%` truncate: use `div_euclid`/`rem_euclid` or explicit floor math. C division by zero is undefined.
- **Hash iteration order** (High): Set and map iteration order comes from the source language's hash function. A port cannot reproduce it: sort before output, or make the order part of the contract.
- **Concurrency** (Medium): Threads, goroutines and locks need a design in Rust (channels, Send/Sync), not a line-by-line translation; keep concurrency in the caller where possible.
- **Clock and random numbers** (Low): Results that depend on the time or on random numbers differ on every run, so a plain recording cannot pin them down. Record with `record --pin-clock` (TS/JS with the napi bridge, Python): `Date.now()`, `new Date()` and `Math.random` (`time.time`, `datetime.now` and `random`) then return fixed values during record, diff and gate, and the port reads the same ones from `src/port/pinned.rs` (`now_ms()`, `random()`, a documented mulberry32). Or pass the time and the seed in as parameters.

## Whole app

Shape: **server**.

| Entry point | Kind | Found from |
| --- | --- | --- |
| `app/main.py` | server | `FastAPI(` at app/main.py:94 |
| `app/scripts/ensure_admin.py` | cli | `if __name__ == "__main__"` at app/scripts/ensure_admin.py:112 |
| `app/scripts/init_db.py` | cli | `if __name__ == "__main__"` at app/scripts/init_db.py:109 |
| `app/thirdparty/dl302_cloud189_helper.py` | cli | `if __name__ == "__main__"` at app/thirdparty/dl302_cloud189_helper.py:45 |
| `scripts/migrate_dl302_store_to_mysql.py` | cli | `if __name__ == "__main__"` at scripts/migrate_dl302_store_to_mysql.py:160 |
| `scripts/migrate_sqlite_to_mysql.py` | cli | `if __name__ == "__main__"` at scripts/migrate_sqlite_to_mysql.py:138 |

### Every dependency

| Dependency | Declared | Used by | Rust | Fit | Note |
| --- | --- | --- | --- | --- | --- |
| `fastapi` (also `starlette`) | yes | 29 | - | none | Web framework: keep the app in Python. |
| `uvicorn` | yes | 0 | axum | none | a server is an application rewrite, not a port |
| `apscheduler` | yes | 3 | ? | unknown |  |
| `cachetools` | yes | 3 | ? | unknown |  |
| `emoji` | yes | 1 | ? | unknown |  |
| `guessit` | yes | 1 | ? | unknown |  |
| `natsort` | yes | 1 | ? | unknown |  |
| `pydantic-settings` | yes | 1 | ? | unknown |  |
| `pymysql` | yes | 1 | ? | unknown |  |
| `rsa` | yes | 1 | ? | unknown |  |
| `treelib` | yes | 1 | ? | unknown |  |
| `alembic` | yes | 0 | ? | unknown |  |
| `email-validator` | yes | 0 | ? | unknown |  |
| `pysocks` | yes | 0 | ? | unknown |  |
| `python-multipart` | yes | 0 | ? | unknown |  |
| `sqlalchemy` | yes | 74 | sqlx | partial | ORM layers port by hand. |
| `pydantic` | yes | 30 | serde, garde | partial | Validation errors will differ. |
| `requests` (also `urllib3`) | yes | 10 | reqwest | good |  |
| `grpcio` (also `grpc`) | yes | 5 | tonic | good |  |
| `bcrypt` | yes | 1 | argon2, bcrypt | good |  |
| `protobuf` (also `google.protobuf`) | yes | 1 | prost | good |  |
| `python-jose` (also `jose`) | yes | 1 | jsonwebtoken | good |  |

### Pieces, in port order

- wave 0: `app/api/routes/__init__.py` (1 line)
- wave 0: `app/core/`, `app/db/`, `app/db/drivers/`, `app/db/repositories/`, `app/models/`, tangled: one piece (1654 lines)
- wave 0: `app/extensions/` (0 lines)
- wave 0: `app/extensions/adapters/` (4225 lines)
- wave 0: `app/extensions/plugins/` (373 lines)
- wave 0: `app/extensions/sync_plugins/` (0 lines)
- wave 0: `app/schemas/audit_log.py` (32 lines)
- wave 0: `app/schemas/auth.py` (13 lines)
- wave 0: `app/schemas/cache_management.py` (52 lines)
- wave 0: `app/schemas/common.py` (5 lines)
- wave 0: `app/schemas/dl302_settings.py` (153 lines)
- wave 0: `app/schemas/drive_account.py` (88 lines)
- wave 0: `app/schemas/drive_account_auth.py` (6 lines)
- wave 0: `app/schemas/drive_account_probe_scheduler.py` (12 lines)
- wave 0: `app/schemas/magic_regex_rule.py` (31 lines)
- wave 0: `app/schemas/media_discovery.py` (57 lines)
- wave 0: `app/schemas/notification.py` (25 lines)
- wave 0: `app/schemas/openlist_settings.py` (8 lines)
- wave 0: `app/schemas/path_browse.py` (23 lines)
- wave 0: `app/schemas/plugin.py` (25 lines)
- wave 0: `app/schemas/resource_search.py` (46 lines)
- wave 0: `app/schemas/role.py` (18 lines)
- wave 0: `app/schemas/sync_execution_files.py` (18 lines)
- wave 0: `app/schemas/sync_task.py` (88 lines)
- wave 0: `app/schemas/system_settings.py` (6 lines)
- wave 0: `app/schemas/task.py` (147 lines)
- wave 0: `app/schemas/task_browse.py` (93 lines)
- wave 0: `app/schemas/task_bundle.py` (13 lines)
- wave 0: `app/schemas/task_magic_regex.py` (9 lines)
- wave 0: `app/schemas/task_repair.py` (15 lines)
- wave 0: `app/schemas/task_scheduler.py` (16 lines)
- wave 0: `app/schemas/tmdb_cache.py` (88 lines)
- wave 0: `app/schemas/tmdb_settings.py` (16 lines)
- wave 0: `app/services/__init__.py` (1 line)
- wave 0: `app/services/drive_account_auth_sessions.py` (53 lines)
- wave 0: `app/services/proxy_image_cache.py` (340 lines)
- wave 0: `app/services/resource_search_providers.py` (463 lines)
- wave 0: `app/services/title_match.py` (240 lines)
- wave 0: `app/thirdparty/dl302_rpc/` (770 lines)
- wave 1: `alembic/` (48 lines)
- wave 1: `app/api/routes/health.py` (24 lines)
- wave 1: `app/api/routes/metrics.py` (6 lines)
- wave 1: `app/middlewares/` (83 lines)
- wave 1: `app/schemas/dashboard.py` (71 lines)
- wave 1: `app/schemas/user.py` (35 lines)
- wave 1: `app/services/audit.py` (333 lines)
- wave 1: `app/services/audit_logs.py` (60 lines)
- wave 1: `app/services/auth.py` (102 lines)
- wave 1: `app/services/drama_update_progress.py` (100 lines)
- wave 1: `app/services/drive_account_lsdir_cache.py` (459 lines)
- wave 1: `app/services/drive_account_lsdir_static_state.py` (209 lines)
- wave 1: `app/services/invalid_share_links.py` (73 lines)
- wave 1: `app/services/notifications/` (427 lines)
- wave 1: `app/services/openlist_settings.py` (31 lines)
- wave 1: `app/services/permissions.py` (5 lines)
- wave 1: `app/services/roles.py` (38 lines)
- wave 1: `app/services/scheduler_validation.py` (45 lines)
- wave 1: `app/services/share_preview_batch_cache.py` (98 lines)
- wave 1: `app/services/start_episode_filter.py` (88 lines)
- wave 1: `app/services/sync_execution_cleanup.py` (39 lines)
- wave 1: `app/services/system_settings.py` (39 lines)
- wave 1: `app/services/title_vector_match.py` (99 lines)
- wave 1: `app/services/tmdb_settings.py` (81 lines)
- wave 1: `app/thirdparty/` (1282 lines)
- wave 1: `scripts/` (247 lines)
- wave 2: `app/api/routes/audit_logs.py` (68 lines)
- wave 2: `app/api/routes/auth.py` (69 lines)
- wave 2: `app/api/routes/notifications.py` (87 lines)
- wave 2: `app/api/routes/permissions.py` (12 lines)
- wave 2: `app/api/routes/roles.py` (103 lines)
- wave 2: `app/api/routes/system_settings.py` (37 lines)
- wave 2: `app/api/routes/tmdb_settings.py` (37 lines)
- wave 2: `app/scripts/` (185 lines)
- wave 2: `app/services/drive_account_lsdir_refresh_status.py` (288 lines)
- wave 2: `app/services/drive_account_probe_scheduler.py` (33 lines)
- wave 2: `app/services/openlist_client_factory.py` (14 lines)
- wave 2: `app/services/task_scheduler.py` (54 lines)
- wave 2: `app/services/tmdb_cache.py` (679 lines)
- wave 2: `app/services/tmdb_cache_scheduler.py` (39 lines)
- wave 2: `app/services/users.py` (61 lines)
- wave 3: `app/api/routes/openlist_settings.py` (116 lines)
- wave 3: `app/api/routes/users.py` (135 lines)
- wave 3: `app/services/dashboard_drama.py` (177 lines)
- wave 3: `app/services/media_discovery.py` (210 lines)
- wave 3: `app/services/openlist_cleanup.py` (122 lines)
- wave 4: `app/api/routes/media_discovery.py` (124 lines)
- wave 4: `app/api/routes/tasks.py`, `app/extensions/runtime/`, `app/services/deferred_sync_delete.py`, `app/services/dl302_cas.py`, `app/services/dl302_settings.py`, `app/services/dl302_strm.py`, `app/services/drama_linked_pipeline.py`, `app/services/drama_share_autoupdate.py`, `app/services/drama_share_repair.py`, `app/services/drive_account_lsdir_scan.py`, `app/services/drive_accounts.py`, `app/services/drive_browse.py`, `app/services/magic_regex.py`, `app/services/resource_search.py`, `app/services/share_preview_batch.py`, `app/services/sync_execution_recovery.py`, `app/services/sync_task_triggers.py`, `app/services/sync_tasks.py`, `app/services/task_bundle_migration.py`, `app/services/task_savepath_snapshot.py`, `app/services/task_share_links.py`, `app/services/tasks.py`, `app/services/telegram_bot/`, tangled: one piece (24483 lines)
- wave 5: `alembic/versions/` (2285 lines)
- wave 5: `app/api/routes/cache_management.py` (216 lines)
- wave 5: `app/api/routes/dashboard.py` (18 lines)
- wave 5: `app/api/routes/dl302_settings.py` (273 lines)
- wave 5: `app/api/routes/magic_regex.py` (51 lines)
- wave 5: `app/api/routes/resource_search.py` (34 lines)
- wave 5: `app/api/routes/sync_tasks.py` (733 lines)
- wave 5: `app/api/routes/tmdb_cache.py` (392 lines)
- wave 5: `app/services/drive_account_signin_jobs.py` (106 lines)
- wave 5: `app/services/plugins.py` (55 lines)
- wave 5: `app/services/sync_plugins.py` (55 lines)
- wave 6: `app/api/routes/drive_accounts.py` (663 lines)
- wave 6: `app/api/routes/plugins.py` (55 lines)
- wave 6: `app/api/routes/sync_plugins.py` (73 lines)
- wave 7: `app/api/` (26 lines)
- wave 8: `app/` (93 lines)

### Decisions for the user

**D1** (shape): The app is a server, entered through app/main.py, app/scripts/ensure_admin.py, app/scripts/init_db.py, app/thirdparty/dl302_cloud189_helper.py, scripts/migrate_dl302_store_to_mysql.py, scripts/migrate_sqlite_to_mysql.py. Port every entry point, in that shape?

> Yes: an axum front door (`app front-door`) takes the server over first

**D2** (blocked app/): app/ is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D3** (blocked app/api/): app/api/ is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D4** (blocked app/api/routes/audit_logs.py): app/api/routes/audit_logs.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D5** (blocked app/api/routes/auth.py): app/api/routes/auth.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D6** (blocked app/api/routes/cache_management.py): app/api/routes/cache_management.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D7** (blocked app/api/routes/dashboard.py): app/api/routes/dashboard.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D8** (blocked app/api/routes/dl302_settings.py): app/api/routes/dl302_settings.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D9** (blocked app/api/routes/drive_accounts.py): app/api/routes/drive_accounts.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D10** (blocked app/api/routes/health.py): app/api/routes/health.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D11** (blocked app/api/routes/magic_regex.py): app/api/routes/magic_regex.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D12** (blocked app/api/routes/media_discovery.py): app/api/routes/media_discovery.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D13** (blocked app/api/routes/metrics.py): app/api/routes/metrics.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D14** (blocked app/api/routes/notifications.py): app/api/routes/notifications.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D15** (blocked app/api/routes/openlist_settings.py): app/api/routes/openlist_settings.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D16** (blocked app/api/routes/permissions.py): app/api/routes/permissions.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D17** (blocked app/api/routes/plugins.py): app/api/routes/plugins.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D18** (blocked app/api/routes/resource_search.py): app/api/routes/resource_search.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D19** (blocked app/api/routes/roles.py): app/api/routes/roles.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D20** (blocked app/api/routes/sync_plugins.py): app/api/routes/sync_plugins.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D21** (blocked app/api/routes/sync_tasks.py): app/api/routes/sync_tasks.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D22** (blocked app/api/routes/system_settings.py): app/api/routes/system_settings.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D23** (blocked app/api/routes/tasks.py): app/api/routes/tasks.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D24** (blocked app/api/routes/tmdb_cache.py): app/api/routes/tmdb_cache.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D25** (blocked app/api/routes/tmdb_settings.py): app/api/routes/tmdb_settings.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D26** (blocked app/api/routes/users.py): app/api/routes/users.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D27** (blocked app/core/): app/core/ is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D28** (blocked app/extensions/runtime/): app/extensions/runtime/ is blocked: Uses dynamic code (eval, new Function or computed imports). Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D29** (blocked app/services/auth.py): app/services/auth.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D30** (blocked app/services/dl302_strm.py): app/services/dl302_strm.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D31** (blocked app/services/proxy_image_cache.py): app/services/proxy_image_cache.py is blocked: `fastapi` has no Rust equivalent. Rework that part in Rust, or keep the unit outside?

> Research the blocker first; then port it with the piece that uses it

**D32** (dependency fastapi): No crate does what fastapi does (used by 29 units; Web framework: keep the app in Python). Reimplement what the app uses, or drop the feature?

> Reimplement only the part the app uses, proved by its recorded cases

**D33** (dependency uvicorn): No crate does what uvicorn does (declared, no unit imports it; a server is an application rewrite, not a port). Reimplement what the app uses, or drop the feature?

> Reimplement only the part the app uses, proved by its recorded cases

**D34** (dependency apscheduler): Which crate replaces apscheduler (used by 3 units)?

> Research it against primary sources into rust/research/apscheduler.md before porting what uses it

**D35** (dependency cachetools): Which crate replaces cachetools (used by 3 units)?

> Research it against primary sources into rust/research/cachetools.md before porting what uses it

**D36** (dependency emoji): Which crate replaces emoji (used by 1 unit)?

> Research it against primary sources into rust/research/emoji.md before porting what uses it

**D37** (dependency guessit): Which crate replaces guessit (used by 1 unit)?

> Research it against primary sources into rust/research/guessit.md before porting what uses it

**D38** (dependency natsort): Which crate replaces natsort (used by 1 unit)?

> Research it against primary sources into rust/research/natsort.md before porting what uses it

**D39** (dependency pydantic-settings): Which crate replaces pydantic-settings (used by 1 unit)?

> Research it against primary sources into rust/research/pydantic-settings.md before porting what uses it

**D40** (dependency pymysql): Which crate replaces pymysql (used by 1 unit)?

> Research it against primary sources into rust/research/pymysql.md before porting what uses it

**D41** (dependency rsa): Which crate replaces rsa (used by 1 unit)?

> Research it against primary sources into rust/research/rsa.md before porting what uses it

**D42** (dependency treelib): Which crate replaces treelib (used by 1 unit)?

> Research it against primary sources into rust/research/treelib.md before porting what uses it

**D43** (dependency alembic): Which crate replaces alembic (declared, no unit imports it)?

> Research it against primary sources into rust/research/alembic.md before porting what uses it

**D44** (dependency email-validator): Which crate replaces email-validator (declared, no unit imports it)?

> Research it against primary sources into rust/research/email-validator.md before porting what uses it

**D45** (dependency pysocks): Which crate replaces pysocks (declared, no unit imports it)?

> Research it against primary sources into rust/research/pysocks.md before porting what uses it

**D46** (dependency python-multipart): Which crate replaces python-multipart (declared, no unit imports it)?

> Research it against primary sources into rust/research/python-multipart.md before porting what uses it

**D47** (dependency sqlalchemy): sqlalchemy maps to sqlx only in part (used by 74 units; ORM layers port by hand). Use it?

> Use sqlx, and record cases for the behavior it does not cover

**D48** (dependency pydantic): pydantic maps to serde, garde only in part (used by 30 units; Validation errors will differ). Use it?

> Use serde, garde, and record cases for the behavior it does not cover

**D49** (tangled units): app/core/, app/db/, app/db/drivers/, app/db/repositories/, app/models/ import each other (1654 lines). Port them as one piece?

> Yes: one piece in wave 0, no glue between its parts

**D50** (tangled units): app/api/routes/tasks.py, app/extensions/runtime/, app/services/deferred_sync_delete.py, app/services/dl302_cas.py, app/services/dl302_settings.py, app/services/dl302_strm.py, app/services/drama_linked_pipeline.py, app/services/drama_share_autoupdate.py, app/services/drama_share_repair.py, app/services/drive_account_lsdir_scan.py, app/services/drive_accounts.py, app/services/drive_browse.py, app/services/magic_regex.py, app/services/resource_search.py, app/services/share_preview_batch.py, app/services/sync_execution_recovery.py, app/services/sync_task_triggers.py, app/services/sync_tasks.py, app/services/task_bundle_migration.py, app/services/task_savepath_snapshot.py, app/services/task_share_links.py, app/services/tasks.py, app/services/telegram_bot/ import each other (24483 lines). Port them as one piece?

> Yes: one piece in wave 4, no glue between its parts

