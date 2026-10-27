// 账户命令（阶段六批次 A：离线账户；批次 B 增加微软登录）
//
// 账户数据存 {root}/accounts.json，Rust 是唯一可信源；
// token 等敏感字段不出 Rust（前端只见 AccountView）。
// 任何账户变更后发 accounts-changed 事件驱动前端刷新。

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use super::AppState;
use crate::account::authlib_auth::{self, ProfileInfo, ServerMeta};
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
            auth_server_url: None,
            auth_api_root: None,
            auth_server_name: None,
            auth_access_token: None,
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
/// 微软账户仅清除本地登录状态（不在服务端吊销令牌）；
/// 皮肤站账户 best-effort 吊销站点 token。
#[tauri::command]
pub async fn remove_account(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    let root = state.paths.root().to_path_buf();
    tauri::async_runtime::spawn_blocking(move || {
        let mut store = AccountStore::load(&root);
        if let Some(acc) = store.get(&id) {
            crate::account::authlib_auth::revoke_on_remove(acc);
        }
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
// 皮肤站登录（authlib-injector）
// ---------------------------------------------------------------------------

/// 预览皮肤站元数据（登录弹窗第一步：确认站点 / 注册链接）
#[tauri::command]
pub async fn fetch_authlib_server_meta(server_url: String) -> Result<ServerMeta, String> {
    authlib_auth::fetch_server_meta(&server_url).await
}

/// 登录结果：角色已确定（单角色/站点预选）时 account 直接入库返回；
/// 多角色待选择时 profiles 非空，token 暂存内存待 confirm_authlib_profile。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthlibLoginResult {
    pub account: Option<AccountView>,
    pub profiles: Vec<ProfileInfo>,
}

/// 待确认的登录会话（多角色选择期间仅存内存，不落盘）
struct PendingAuthlib {
    server_url: String,
    api_root: String,
    server_name: String,
    access_token: String,
    profiles: Vec<ProfileInfo>,
}

static PENDING_AUTHLIB: std::sync::Mutex<Option<PendingAuthlib>> = std::sync::Mutex::new(None);

/// 皮肤站账密登录。多角色账号返回角色列表等待选择，其余直接创建账户并激活。
#[tauri::command]
pub async fn login_authlib(
    app: AppHandle,
    state: State<'_, AppState>,
    server_url: String,
    username: String,
    password: String,
) -> Result<AuthlibLoginResult, String> {
    let server_url = authlib_auth::normalize_server_url(&server_url)?;
    if username.trim().is_empty() || password.is_empty() {
        return Err("请输入账号与密码".into());
    }
    let (meta, token, profiles, selected) =
        authlib_auth::login(&server_url, &username, &password).await?;
    if profiles.is_empty() {
        return Err("该皮肤站账号没有可用角色，请先在站点注册角色".into());
    }

    // 待选角色：站点未预选且多于一个 → 暂存内存等待用户选择
    let chosen: Option<ProfileInfo> = selected
        .filter(|s| profiles.iter().any(|p| p.id == s.id))
        .or_else(|| if profiles.len() == 1 { profiles.first().cloned() } else { None });
    let Some(chosen) = chosen else {
        *PENDING_AUTHLIB.lock().unwrap() = Some(PendingAuthlib {
            server_url: server_url.clone(),
            api_root: meta.api_root.clone(),
            server_name: meta.server_name.clone(),
            access_token: token,
            profiles,
        });
        return Ok(AuthlibLoginResult { account: None, profiles: Vec::new() });
    };

    let root = state.paths.root().to_path_buf();
    let account = tauri::async_runtime::spawn_blocking(move || {
        let mut store = AccountStore::load(&root);
        let account = authlib_auth::new_account(
            &server_url,
            &meta.api_root,
            &meta.server_name,
            &token,
            &chosen,
        )?;
        // 同站点同角色去重（UUID 相同即同一角色）
        if store.accounts.iter().any(|a| a.is_authlib() && a.uuid == account.uuid) {
            return Err(format!("已存在皮肤站角色「{}」，无需重复添加", account.name));
        }
        let view = account.view();
        store.accounts.push(account);
        let new_id = store.accounts.last().unwrap().id.clone();
        store.set_active(&new_id)?;
        store.save(&root)?;
        Ok(view)
    })
    .await
    .map_err(|e| format!("账户写入任务失败：{e}"))??;

    emit_changed(&app);
    Ok(AuthlibLoginResult { account: Some(account), profiles: Vec::new() })
}

/// 多角色账号登录后绑定所选角色（消费内存中的待确认会话）
#[tauri::command]
pub async fn confirm_authlib_profile(
    app: AppHandle,
    state: State<'_, AppState>,
    profile_id: String,
) -> Result<AccountView, String> {
    let pending = PENDING_AUTHLIB
        .lock()
        .unwrap()
        .take()
        .ok_or("没有待确认的皮肤站登录会话，请重新登录")?;
    let Some(profile) = pending.profiles.iter().find(|p| p.id == profile_id) else {
        return Err("所选角色不在该账号的角色列表中".into());
    };
    let profile = profile.clone();
    let root = state.paths.root().to_path_buf();
    let account = tauri::async_runtime::spawn_blocking(move || {
        let mut store = AccountStore::load(&root);
        let account = authlib_auth::new_account(
            &pending.server_url,
            &pending.api_root,
            &pending.server_name,
            &pending.access_token,
            &profile,
        )?;
        if store.accounts.iter().any(|a| a.is_authlib() && a.uuid == account.uuid) {
            return Err(format!("已存在皮肤站角色「{}」，无需重复添加", account.name));
        }
        let view = account.view();
        store.accounts.push(account);
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

/// 拉取皮肤站角色当前皮肤（3D 预览用；未设置皮肤返回 None）
#[tauri::command]
pub async fn get_authlib_skin_data(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Option<crate::account::authlib_auth::SkinData>, String> {
    let root = state.paths.root().to_path_buf();
    let account = load_account(&root, &account_id)?;
    if !account.is_authlib() {
        return Ok(None);
    }
    crate::account::authlib_auth::fetch_skin_data(&account).await
}

/// 发起 LittleSkin OAuth 设备码登录（免密授权）：立即返回；
/// 设备码与状态经 authlib-oauth-state 事件推送，可经 cancel_littleskin_oauth 取消。
#[tauri::command]
pub async fn login_littleskin_oauth(
    app: AppHandle,
    state: State<'_, AppState>,
    server_url: String,
) -> Result<(), String> {
    let server_url = crate::account::authlib_auth::normalize_server_url(&server_url)?;
    let root = state.paths.root().to_path_buf();
    crate::account::littleskin_oauth::start_device_login(app, server_url, root)
}

/// 取消当前 LittleSkin OAuth 登录流程
#[tauri::command]
pub async fn cancel_littleskin_oauth() -> Result<(), String> {
    crate::account::littleskin_oauth::cancel_login();
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

/// 应用内置预设皮肤（preset：steve / alex；模型随预设固定，可再手动切换）
#[tauri::command]
pub async fn apply_offline_preset(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    preset: String,
) -> Result<(), String> {
    let root = state.paths.root().to_path_buf();
    crate::account::skin::apply_preset(&root, &account_id, &preset).await?;
    emit_changed(&app);
    Ok(())
}

/// 应用 URL 皮肤（仅 http/https；模型沿用当前设置）
#[tauri::command]
pub async fn apply_offline_skin_url(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    url: String,
) -> Result<(), String> {
    let root = state.paths.root().to_path_buf();
    crate::account::skin::apply_url_skin(&root, &account_id, &url).await?;
    emit_changed(&app);
    Ok(())
}

/// 应用本地皮肤文件：path 为空时弹文件选择框（png）
#[tauri::command]
pub async fn apply_offline_skin_file(
    app: AppHandle,
    state: State<'_, AppState>,
    account_id: String,
    path: Option<String>,
) -> Result<(), String> {
    let picked = match path {
        Some(p) if !p.trim().is_empty() => p,
        _ => {
            use tauri_plugin_dialog::DialogExt;
            let app2 = app.clone();
            let picked = tauri::async_runtime::spawn_blocking(move || {
                app2.dialog()
                    .file()
                    .add_filter("皮肤图片", &["png"])
                    .blocking_pick_file()
            })
            .await
            .map_err(|e| format!("文件对话框任务失败：{e}"))?;
            match picked {
                Some(p) => p
                    .into_path()
                    .map_err(|e| format!("获取所选路径失败：{e}"))?
                    .to_string_lossy()
                    .into_owned(),
                None => return Ok(()), // 用户取消
            }
        }
    };
    let root = state.paths.root().to_path_buf();
    crate::account::skin::apply_file_skin(&root, &account_id, std::path::Path::new(&picked))
        .await?;
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
