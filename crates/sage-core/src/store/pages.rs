use anyhow::Result;

use super::Store;

impl Store {
    /// 创建自定义页面，返回新记录 id
    pub fn save_custom_page(&self, title: &str, markdown: &str) -> Result<i64> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "INSERT INTO custom_pages (title, markdown) VALUES (?1, ?2)",
            rusqlite::params![title, markdown],
        )?;
        Ok(conn.last_insert_rowid())
    }

    /// 更新已有自定义页面的标题和内容
    pub fn update_custom_page(&self, id: i64, title: &str, markdown: &str) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "UPDATE custom_pages SET title = ?1, markdown = ?2, updated_at = datetime('now') WHERE id = ?3",
            rusqlite::params![title, markdown, id],
        )?;
        Ok(())
    }

    /// 获取单个自定义页面，返回 (id, title, markdown, created_at, updated_at)
    pub fn get_custom_page(
        &self,
        id: i64,
    ) -> Result<Option<(i64, String, String, String, String)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, title, markdown, created_at, updated_at FROM custom_pages WHERE id = ?1",
        )?;
        let mut rows = stmt.query_map(rusqlite::params![id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
            ))
        })?;
        Ok(rows.next().transpose()?)
    }

    /// 列出自定义页面（不含 markdown 内容），返回 Vec<(id, title, created_at, updated_at)>
    pub fn list_custom_pages(
        &self,
        limit: usize,
    ) -> Result<Vec<(i64, String, String, String)>> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        let mut stmt = conn.prepare(
            "SELECT id, title, created_at, updated_at FROM custom_pages ORDER BY created_at DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map(rusqlite::params![limit as i64], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(Into::into)
    }

    /// 删除自定义页面
    pub fn delete_custom_page(&self, id: i64) -> Result<()> {
        let conn = self
            .conn
            .lock()
            .map_err(|e| anyhow::anyhow!("数据库锁获取失败: {e}"))?;
        conn.execute(
            "DELETE FROM custom_pages WHERE id = ?1",
            rusqlite::params![id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn store() -> Store {
        Store::open_in_memory().unwrap()
    }

    #[test]
    fn save_and_get_page() {
        let s = store();
        let id = s.save_custom_page("Test Page", "# Test\nHello").unwrap();
        assert!(id > 0);
        let page = s.get_custom_page(id).unwrap().unwrap();
        assert_eq!(page.1, "Test Page");
        assert_eq!(page.2, "# Test\nHello");
    }

    #[test]
    fn update_page() {
        let s = store();
        let id = s.save_custom_page("Old Title", "old content").unwrap();
        s.update_custom_page(id, "New Title", "new content").unwrap();
        let page = s.get_custom_page(id).unwrap().unwrap();
        assert_eq!(page.1, "New Title");
        assert_eq!(page.2, "new content");
    }

    #[test]
    fn list_pages() {
        let s = store();
        s.save_custom_page("Page A", "content a").unwrap();
        s.save_custom_page("Page B", "content b").unwrap();
        let pages = s.list_custom_pages(10).unwrap();
        assert_eq!(pages.len(), 2);
    }

    #[test]
    fn delete_page() {
        let s = store();
        let id = s.save_custom_page("To Delete", "bye").unwrap();
        s.delete_custom_page(id).unwrap();
        assert!(s.get_custom_page(id).unwrap().is_none());
    }

    #[test]
    fn get_nonexistent_page() {
        let s = store();
        assert!(s.get_custom_page(9999).unwrap().is_none());
    }
}
