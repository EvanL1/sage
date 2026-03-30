use anyhow::{Context, Result};
use sage_types::UserProfile;

use super::Store;

impl Store {
    /// 保存用户 profile（upsert id=1）
    pub fn save_profile(&self, profile: &UserProfile) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let data = serde_json::to_string(profile).context("序列化 UserProfile 失败")?;
        let now = chrono::Local::now().to_rfc3339();
        conn.execute(
            "INSERT OR REPLACE INTO profile (id, data, sop_version, updated_at) VALUES (1, ?1, ?2, ?3)",
            rusqlite::params![data, profile.sop_version, now],
        ).context("保存 profile 失败")?;
        Ok(())
    }

    /// 读取 prompt 语言设置（"zh" | "en"），默认 "zh"
    pub fn prompt_lang(&self) -> String {
        self.load_profile()
            .ok()
            .flatten()
            .map(|p| p.identity.prompt_language)
            .unwrap_or_else(|| "zh".into())
    }

    /// 读取 id=1 的 profile
    pub fn load_profile(&self) -> Result<Option<UserProfile>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn
            .prepare("SELECT data FROM profile WHERE id = 1")
            .context("准备 load_profile 查询失败")?;
        let mut rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .context("执行 load_profile 查询失败")?;

        match rows.next() {
            Some(Ok(data)) => {
                let profile: UserProfile =
                    serde_json::from_str(&data).context("反序列化 UserProfile 失败")?;
                Ok(Some(profile))
            }
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    /// 追加一条 negative_rule（去重），写回 profile
    pub fn append_negative_rule(&self, rule: &str) -> Result<()> {
        let rule = rule.trim().to_string();
        if rule.is_empty() {
            return Ok(());
        }
        let mut profile = self.load_profile()?.unwrap_or_default();
        if !profile.negative_rules.iter().any(|r| r == &rule) {
            profile.negative_rules.push(rule);
            self.save_profile(&profile)?;
        }
        Ok(())
    }

    /// 从 profile 表读 sop_version
    pub fn get_sop_version(&self) -> Result<u32> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let version: Option<u32> = conn
            .query_row("SELECT sop_version FROM profile WHERE id = 1", [], |row| {
                row.get(0)
            })
            .ok();
        Ok(version.unwrap_or(0))
    }
}
