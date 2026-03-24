use anyhow::{Context, Result};
use sage_types::ProviderConfig;

use super::Store;

impl Store {
    /// 保存或更新 provider 配置（upsert by provider_id）
    pub fn save_provider_config(&self, config: &ProviderConfig) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let now = chrono::Local::now().to_rfc3339();
        let enabled: i32 = if config.enabled { 1 } else { 0 };
        let priority: Option<i32> = config.priority.map(|p| p as i32);
        conn.execute(
            "INSERT OR REPLACE INTO provider_config
             (provider_id, api_key, model, base_url, enabled, priority, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            rusqlite::params![
                config.provider_id,
                config.api_key,
                config.model,
                config.base_url,
                enabled,
                priority,
                now,
            ],
        )
        .context("保存 provider_config 失败")?;
        Ok(())
    }

    /// 加载所有 provider 配置
    pub fn load_provider_configs(&self) -> Result<Vec<ProviderConfig>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare("SELECT provider_id, api_key, model, base_url, enabled, priority FROM provider_config")
            .context("准备 load_provider_configs 查询失败")?;
        let rows = stmt
            .query_map([], |row| {
                let enabled_int: i32 = row.get(4)?;
                let priority_int: Option<i32> = row.get(5)?;
                Ok(ProviderConfig {
                    provider_id: row.get(0)?,
                    api_key: row.get(1)?,
                    model: row.get(2)?,
                    base_url: row.get(3)?,
                    enabled: enabled_int != 0,
                    priority: priority_int.map(|p| p as u8),
                })
            })
            .context("执行 load_provider_configs 查询失败")?;
        let mut configs = Vec::new();
        for row in rows {
            configs.push(row?);
        }
        Ok(configs)
    }

    /// 删除指定 provider 配置
    pub fn delete_provider_config(&self, id: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "DELETE FROM provider_config WHERE provider_id = ?1",
            rusqlite::params![id],
        )
        .context("删除 provider_config 失败")?;
        Ok(())
    }
}
