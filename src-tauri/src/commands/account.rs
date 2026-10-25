// 账户命令（阶段六批次 A：离线账户；批次 B 增加微软登录）
//
// 账户数据存 {root}/accounts.json，Rust 是唯一可信源；
// token 等敏感字段不出 Rust（前端只见 AccountView）。
// 任何账户变更后发 accounts-changed 事件驱动前端刷新。

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use super::AppState;
use crate::account::store::AccountStore;
use crate::account::{validate_offline_name, Account, AccountView};
use crate::launch::args::offline_uuid;

/// 账户列表结果
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountListResult {
    pub accounts: Vec<AccountView>,
    pub active_id: Option<String>,
}

fn emit_changed(app: &AppHandle) {
    let _ = app.emit("accounts-changed", ());
}

fn list_result(store: &AccountStore) -> AccountListResult {
    AccountListResult {
        accounts: store.accounts.iter().map(|a| a.view()).collect(),
        active_id: store.active_id.clone(),
    }
}

/// 列出全部账户与激活项
#[tauri::command]
pub async fn list_accounts(state: State<'_, AppState>) -> Result<AccountListResult, String> {
    let root = state.paths.root().to_path_buf();
    let store = tauri::async_runtime::spawn_blocking(move || AccountStore::load(&root))
        .await
        .map_err(|e| format!("账户读取任务失败：{e}"))?;
    Ok(list_result(&store))
}

/// 添加离线账户（自定义 ID），创建后自动激活
#[tauri::command]
pub async fn add_offline_account(
    app: AppHandle,
    state: State<'_, AppState>,
    name: String,
) -> Result<AccountView, String> {
    let name = name.trim().to_string();
    validate_offline_name(&name)?;
    let root = state.paths.root().to_path_buf();

    let account = tauri::async_runtime::spawn_blocking(move || {
        let mut store = AccountStore::load(&root);
        // 同名离线账户去重（同名同 UUID 无意义）
        if store
            .accounts
            .iter()
            .any(|a| a.kind == "offline" && a.name.eq_ignore_ascii_case(&name))
        {
            return Err(format!("已存在同名离线账户「{name}」"));
        }
        let account = Account {
            id: uuid::Uuid::new_v4().simple().to_string(),
            kind: "offline".into(),
            name: name.clone(),
            uuid: offline_uuid(&name).to_string(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
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
        };
        let view = account.view();
        store.accounts.push(account);
        // 自动激活：添加即用
        let new_id = store.accounts.last().unwrap().id.clone();
        store.set_active(&new_id)?;
        store.save(&root)?;
        Ok(view)
    })
    .await
    .map_err(|e| format!("账户写入任务失败：{e}"))??;

    emit_changed(&app);
    Ok(account)
}

/// 切换激活账户
#[tauri::command]
pub async fn set_active_account(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let root = state.paths.root().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let mut store = AccountStore::load(&root);
        store.set_active(&id)?;
        store.save(&root)
    })
    .await
    .map_err(|e| format!("账户写入任务失败：{e}"))??;
    emit_changed(&app);
    Ok(())
}

/// 移除账户（移除激活账户时激活项自动回落）。
/// 微软账户仅清除本地登录状态（不在服务端吊销令牌）。
#[tauri::command]
pub async fn remove_account(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let root = state.paths.root().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let mut store = AccountStore::load(&root);
        store.remove(&id)?;
        store.save(&root)
    })
    .await
    .map_err(|e| format!("账户写入任务失败：{e}"))??;
    emit_changed(&app);
    Ok(())
}

/// 发起微软设备码登录：立即返回；user_code 与状态经 ms-login-state 事件推送。
/// 同一时间仅允许一个流程，可经 cancel_microsoft_login 取消。
#[tauri::command]
pub async fn login_microsoft(app: AppHandle) -> Result<(), String> {
    crate::account::ms_auth::start_device_login(app)
}

/// 取消当前微软登录流程
#[tauri::command]
pub async fn cancel_microsoft_login() -> Result<(), String> {
    crate::account::ms_auth::cancel_login();
    Ok(())
}

// ---------------------------------------------------------------------------
// 正版皮肤 / 披风管理（批次 C）
// ---------------------------------------------------------------------------

/// 弹出文件对话框选择皮肤 PNG（取消返回 None）
#[tauri::command]
pub async fn pick_skin_image(app: AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;

    let app2 = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        app2.dialog()
            .file()
            .add_filter("皮肤图片", &["png"])
            .blocking_pick_file()
    })
    .await
    .map_err(|e| format!("对话框任务执行失败：{e}"))?;
    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|e| format!("获取所选路径失败：{e}"))?;
    Ok(Some(path.to_string_lossy().into_owned()))
}

/// 加载账户（按 id）供皮肤 / 披风命令使用
fn load_account(root: &std::path::Path, id: &str) -> Result<crate::account::Account, String> {
    let store = AccountStore::load(root);
    store
        .get(id)
        .cloned()
        .ok_or_else(|| "账户不存在或已被移除".into())
}

/// 应用皮肤库中的历史皮肤
#[tauri::command]
pub async fn apply_library_skin(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    skin_id: String,
) -> Result<(), String> {
    let root = state.paths.root().to_path_buf();
    let account = load_account(&root, &account_id)?;
    crate::account::profile::apply_library_skin(&root, &account, &skin_id).await?;
    emit_changed(&app);
    Ok(())
}

/// 切换当前皮肤的模型（variant：slim / classic）
#[tauri::command]
pub async fn switch_skin_variant(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    variant: String,
) -> Result<(), String> {
    let root = state.paths.root().to_path_buf();
    let account = load_account(&root, &account_id)?;
    crate::account::profile::switch_skin_variant(&root, &account, &variant).await?;
    emit_changed(&app);
    Ok(())
}

/// 上传本地 PNG 皮肤（variant：slim / classic）
#[tauri::command]
pub async fn upload_skin(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    path: String,
    variant: String,
) -> Result<(), String> {
    let root = state.paths.root().to_path_buf();
    let account = load_account(&root, &account_id)?;
    crate::account::profile::upload_skin(&root, &account, &path, &variant).await?;
    emit_changed(&app);
    Ok(())
}

/// 以 URL 应用皮肤（官方 API 要求 https 地址）
#[tauri::command]
pub async fn apply_skin_url(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    url: String,
    variant: String,
) -> Result<(), String> {
    let root = state.paths.root().to_path_buf();
    let account = load_account(&root, &account_id)?;
    crate::account::profile::apply_skin_url(&root, &account, &url, &variant).await?;
    emit_changed(&app);
    Ok(())
}

/// 装备披风
#[tauri::command]
pub async fn equip_cape(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    cape_id: String,
) -> Result<(), String> {
    let root = state.paths.root().to_path_buf();
    let account = load_account(&root, &account_id)?;
    crate::account::profile::equip_cape(&root, &account, &cape_id).await?;
    emit_changed(&app);
    Ok(())
}

/// 脱下披风
#[tauri::command]
pub async fn unequip_cape(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
) -> Result<(), String> {
    let root = state.paths.root().to_path_buf();
    let account = load_account(&root, &account_id)?;
    crate::account::profile::unequip_cape(&root, &account).await?;
    emit_changed(&app);
    Ok(())
}

// ---------------------------------------------------------------------------
// 离线皮肤（批次 D：抓取正版玩家注入）
// ---------------------------------------------------------------------------

/// 抓取正版玩家皮肤并应用到离线账户
#[tauri::command]
pub async fn fetch_and_apply_offline_skin(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    player_name: String,
) -> Result<(), String> {
    let root = state.paths.root().to_path_buf();
    crate::account::skin::fetch_and_apply(&root, &account_id, &player_name).await?;
    emit_changed(&app);
    Ok(())
}

/// 移除离线账户皮肤（恢复默认）
#[tauri::command]
pub async fn remove_offline_skin(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
) -> Result<(), String> {
    let root = state.paths.root().to_path_buf();
    crate::account::skin::remove(&root, &account_id).await?;
    emit_changed(&app);
    Ok(())
}

/// 切换离线皮肤模型（variant：slim / classic；贴图不变）
#[tauri::command]
pub async fn set_offline_skin_variant(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    variant: String,
) -> Result<(), String> {
    let root = state.paths.root().to_path_buf();
    crate::account::skin::set_variant(&root, &account_id, &variant).await?;
    emit_changed(&app);
    Ok(())
}

/// 读取离线账户皮肤 PNG 的 data URL（3D 预览用；无皮肤返回 None）
#[tauri::command]
pub async fn get_offline_skin_data_url(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Option<String>, String> {
    let root = state.paths.root().to_path_buf();
    crate::account::skin::read_skin_data_url(&root, &account_id).await
}
