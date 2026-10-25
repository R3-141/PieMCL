// 账户系统（阶段六）
//
// - store.rs：账户持久化 {root}/accounts.json（全局账户表 + 激活账户 id）
// - ms_auth.rs：微软设备码登录 + XBL/XSTS/MC 认证链 + token 刷新（批次 B）
// - profile.rs：MC profile / 皮肤 / 披风 API（批次 C）
// - skin.rs / local_ygg.rs：离线皮肤（批次 D）
//
// 决策记录：token 明文 JSON 存储（用户决策）；全局单一激活账户；
// 离线皮肤仅支持抓取正版玩家注入（不支持自定义上传）；离线披风不做。

pub mod local_ygg;
pub mod ms_auth;
pub mod profile;
pub mod skin;
pub mod store;

use serde::{Deserialize, Serialize};

/// 正版 profile 的单张皮肤（批次 C 拉取后缓存）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkinEntry {
    pub id: String,
    /// ACTIVE / （历史皮肤无 state 或为空）
    #[serde(default)]
    pub state: String,
    pub url: String,
    /// classic（粗壮/Steve）/ slim（纤细/Alex）
    pub variant: String,
    #[serde(default)]
    pub alias: Option<String>,
}

/// 正版 profile 的单件披风（批次 C）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CapeEntry {
    pub id: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub alias: Option<String>,
    pub url: String,
}

/// 离线账户的注入皮肤元数据（批次 D；PNG 存 {root}/skins/{account_id}.png）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineSkin {
    /// 来源正版玩家名
    pub source_name: String,
    /// classic / slim
    pub variant: String,
    /// 抓取时间（epoch 秒）
    pub fetched_at: i64,
}

/// 单个账户（离线 / 微软正版）。
/// 微软 token 字段为明文存储（用户决策），不会通过 IPC 暴露给前端。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Account {
    /// 账户记录 id（uuid v4，与游戏内 UUID 无关）
    pub id: String,
    /// offline / microsoft
    pub kind: String,
    /// 玩家名（离线 = 自定义 ID；正版 = profile name）
    pub name: String,
    /// 游戏内 UUID（离线 = OfflinePlayer:{name} 的 v3 派生；正版 = profile uuid）
    pub uuid: String,
    /// 创建时间（epoch 秒，展示用）
    pub created_at: i64,

    // ---- 微软正版专属（离线账户为 None）----
    #[serde(default)]
    pub ms_access_token: Option<String>,
    #[serde(default)]
    pub ms_refresh_token: Option<String>,
    /// 微软 token 过期时间（epoch 秒）
    #[serde(default)]
    pub ms_expires_at: Option<i64>,
    #[serde(default)]
    pub mc_access_token: Option<String>,
    /// MC token 过期时间（epoch 秒，约 24h 有效）
    #[serde(default)]
    pub mc_expires_at: Option<i64>,
    /// Xbox 用户哈希（XBL/XSTS 返回，皮肤 API 部分调用需要）
    #[serde(default)]
    pub uhs: Option<String>,
    /// 正版玩家 xuid（启动参数 --xuid）
    #[serde(default)]
    pub xuid: Option<String>,

    // ---- 正版 profile 缓存（批次 C）----
    #[serde(default)]
    pub skins: Vec<SkinEntry>,
    #[serde(default)]
    pub capes: Vec<CapeEntry>,
    #[serde(default)]
    pub active_cape_id: Option<String>,

    // ---- 离线账户注入皮肤（批次 D）----
    #[serde(default)]
    pub offline_skin: Option<OfflineSkin>,
}

impl Account {
    /// 是否为离线账户
    pub fn is_offline(&self) -> bool {
        self.kind == "offline"
    }

    /// 前端可见视图（剥离全部 token 字段）
    pub fn view(&self) -> AccountView {
        AccountView {
            id: self.id.clone(),
            kind: self.kind.clone(),
            name: self.name.clone(),
            uuid: self.uuid.clone(),
            created_at: self.created_at,
            skins: self.skins.clone(),
            capes: self.capes.clone(),
            active_cape_id: self.active_cape_id.clone(),
            offline_skin: self.offline_skin.clone(),
        }
    }
}

/// 前端账户视图（无 token 等敏感字段；皮肤 / 披风为 profile 缓存，非敏感）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountView {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub uuid: String,
    pub created_at: i64,
    /// 正版 profile 缓存的皮肤 / 披风（离线账户为空）
    #[serde(default)]
    pub skins: Vec<SkinEntry>,
    #[serde(default)]
    pub capes: Vec<CapeEntry>,
    #[serde(default)]
    pub active_cape_id: Option<String>,
    /// 离线账户注入皮肤元数据（离线皮肤功能）
    #[serde(default)]
    pub offline_skin: Option<OfflineSkin>,
}

/// 离线玩家 ID 合法性：3–16 位字母 / 数字 / 下划线（与正版的玩家名规则一致）
pub fn validate_offline_name(name: &str) -> Result<(), String> {
    let ok = (3..=16).contains(&name.len())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ok {
        Ok(())
    } else {
        Err("离线玩家 ID 需为 3–16 位字母、数字或下划线".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_name_rules() {
        assert!(validate_offline_name("Steve").is_ok());
        assert!(validate_offline_name("ab").is_err(), "过短");
        assert!(validate_offline_name("a_1B-").is_err(), "非法字符");
        assert!(validate_offline_name("a1234567890123456").is_err(), "17 位过长");
        assert!(validate_offline_name("a123456789012345").is_ok(), "恰好 16 位");
    }
}
