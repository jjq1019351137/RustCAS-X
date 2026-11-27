use anyhow::{Context, Result};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;
use std::str::FromStr;

pub async fn connect(database_url: &str) -> Result<SqlitePool> {
    let opts = SqliteConnectOptions::from_str(database_url)
        .with_context(|| format!("parse DATABASE_URL {database_url}"))?
        .create_if_missing(true)
        .foreign_keys(true);

    let pool = SqlitePoolOptions::new()
        .max_connections(8)
        .connect_with(opts)
        .await
        .context("connect sqlite")?;

    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .context("run migrations")?;

    Ok(pool)
}

pub const PERMISSIONS_SEED: &[(&str, &str)] = &[
    ("audit:read", "审计日志查看"),
    ("user:read", "用户查看"),
    ("user:write", "用户管理"),
    ("role:read", "角色查看"),
    ("role:write", "角色管理"),
    ("permission:read", "权限查看"),
    ("drive_account:read", "驱动查看"),
    ("drive_account:write", "驱动管理"),
    ("plugin:read", "插件查看"),
    ("plugin:write", "插件管理"),
    ("notify:read", "通知查看"),
    ("notify:write", "通知管理"),
    ("task:read", "任务查看"),
    ("task:write", "任务管理"),
    ("task:run", "任务执行"),
    ("sync:read", "同步任务查看"),
    ("sync:write", "同步任务管理"),
    ("sync:run", "同步任务执行"),
];

pub async fn ensure_permissions_and_roles(pool: &SqlitePool) -> Result<()> {
    for (code, name) in PERMISSIONS_SEED {
        sqlx::query(
            r#"
            INSERT INTO permissions (code, name)
            VALUES (?, ?)
            ON CONFLICT(code) DO UPDATE SET name = excluded.name
            "#,
        )
        .bind(code)
        .bind(name)
        .execute(pool)
        .await?;
    }

    sqlx::query(
        r#"
        INSERT INTO roles (name, description)
        VALUES ('admin', '系统管理员')
        ON CONFLICT(name) DO NOTHING
        "#,
    )
    .execute(pool)
    .await?;

    let role_id: i64 = sqlx::query_scalar("SELECT id FROM roles WHERE name = 'admin'")
        .fetch_one(pool)
        .await?;

    let perm_ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM permissions")
        .fetch_all(pool)
        .await?;

    for pid in perm_ids {
        sqlx::query(
            r#"
            INSERT OR IGNORE INTO role_permissions (role_id, permission_id)
            VALUES (?, ?)
            "#,
        )
        .bind(role_id)
        .bind(pid)
        .execute(pool)
        .await?;
    }

    Ok(())
}
