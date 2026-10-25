// 账户持久化：{root}/accounts.json
//
// 全局账户表 + 激活账户 id（全局作用域：启动任意实例均使用激活账户）。
// 文件很小，按需整体读写（与 ModSources 相同的轻量模式，不占 AppState）。
// 文件缺失或损坏时静默回落空表（账户信息丢失不应阻塞启动器其他功能）。

use std::path::Path;

use serde::{Deserialize, Serialize};

use super::Account;

/// 账户表
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountStore {
    #[serde(default)]
    pub accounts: Vec<Account>,
    /// 当前激活账户（None = 未设置；启动实例时必需）
    #[serde(default)]
    pub active_id: Option<String>,
}

impl AccountStore {
    pub fn load(root: &Path) -> Self {
        let path = root.join("accounts.json");
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let path = root.join("accounts.json");
        let text = serde_json::to_string_pretty(self)
            .map_err(|e| format!("序列化账户数据失败：{e}"))?;
        std::fs::write(&path, text).map_err(|e| format!("保存账户数据失败：{e}"))
    }

    /// 激活账户
    pub fn active(&self) -> Option<&Account> {
        let id = self.active_id.as_ref()?;
        self.accounts.iter().find(|a| a.id == *id)
    }

    /// 激活账户（可变）
    pub fn active_mut(&mut self) -> Option<&mut Account> {
        let id = self.active_id.clone()?;
        self.accounts.iter_mut().find(|a| a.id == id)
    }

    /// 按 id 取账户
    #[allow(dead_code)]
    pub fn get(&self, id: &str) -> Option<&Account> {
        self.accounts.iter().find(|a| a.id == id)
    }

    /// 按 id 取账户（可变）
    pub fn get_mut(&mut self, id: &str) -> Option<&mut Account> {
        self.accounts.iter_mut().find(|a| a.id == id)
    }

    /// 设置激活账户（不存在时报错）
    pub fn set_active(&mut self, id: &str) -> Result<(), String> {
        if !self.accounts.iter().any(|a| a.id == id) {
            return Err(format!("账户不存在：{id}"));
        }
        self.active_id = Some(id.to_string());
        Ok(())
    }

    /// 移除账户；若移除的是激活账户，激活项回落到剩余账户的第一个（可能为 None）
    pub fn remove(&mut self, id: &str) -> Result<(), String> {
        let before = self.accounts.len();
        self.accounts.retain(|a| a.id != id);
        if self.accounts.len() == before {
            return Err(format!("账户不存在：{id}"));
        }
        if self.active_id.as_deref() == Some(id) {
            self.active_id = self.accounts.first().map(|a| a.id.clone());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acc(id: &str) -> Account {
        Account {
            id: id.into(),
            kind: "offline".into(),
            name: format!("P{id}"),
            uuid: "u".into(),
            created_at: 0,
            ms_access_token: None,
            ms_refresh_token: None,
            ms_expires_at: None,
            mc_access_token: None,
            mc_expires_at: None,
            uhs: None,
            xuid: None,
            skins: Vec::new(),
            capes: Vec::new(),
            active_cape_id: None,
            offline_skin: None,
        }
    }

    #[test]
    fn active_fallback_on_remove() {
        let mut s = AccountStore::default();
        s.accounts.push(acc("a"));
        s.accounts.push(acc("b"));
        s.set_active("a").unwrap();
        // 删除激活账户 → 回落到第一个剩余
        s.remove("a").unwrap();
        assert_eq!(s.active_id.as_deref(), Some("b"));
        s.remove("b").unwrap();
        assert_eq!(s.active_id, None);
        // 重复删除报错
        assert!(s.remove("b").is_err());
    }

    #[test]
    fn roundtrip_with_tokens() {
        let dir = std::env::temp_dir().join(format!("piemcl-acc-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = AccountStore::default();
        let mut a = acc("a");
        a.kind = "microsoft".into();
        a.ms_refresh_token = Some("rt".into());
        a.mc_access_token = Some("mt".into());
        s.accounts.push(a);
        s.set_active("a").unwrap();
        s.save(&dir).unwrap();
        let loaded = AccountStore::load(&dir);
        assert_eq!(loaded.active_id.as_deref(), Some("a"));
        assert_eq!(loaded.active().unwrap().ms_refresh_token.as_deref(), Some("rt"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
