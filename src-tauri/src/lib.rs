mod account;
mod commands;
// core / download / loader 公开供 src/bin/ 下的开发工具（forge 安装矩阵冒烟）复用
pub mod core;
pub mod download;
pub mod game;
pub mod instance;
pub mod java;
pub mod launch;
pub mod loader;
mod mods;
mod natfrp;
mod servers;
mod version;

use crate::commands::AppState;
use crate::core::paths::LauncherPaths;
use crate::core::settings::AppSettings;
use crate::download::DownloadEngine;
use crate::instance::store::InstanceStore;
use crate::version::repo::VersionRepo;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            use std::sync::Arc;
            use tauri::Manager;
            // 双根目录（对齐主流启动器，PCL 风格）：
            //   release = exe 同目录：PieMCL\（启动器私有）+ .minecraft\（游戏公共根）
            //   debug = %APPDATA%/com.piemcl.launcher（启动器私有），游戏根为其下 .minecraft
            let app_data = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("无法确定应用数据目录：{e}"))?;
            #[cfg(not(debug_assertions))]
            let (launcher_dir, game_root) = {
                let base = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                    .unwrap_or_else(|| app_data.clone());
                (base.join("PieMCL"), base.join(".minecraft"))
            };
            #[cfg(debug_assertions)]
            let (launcher_dir, game_root) = (app_data.clone(), app_data.join(".minecraft"));

            // 旧布局（v1：根下 instances\）自动迁移到 v2 结构。
            // 这是结构升级（代码只认新布局），无需用户确认；迁移步骤均为
            // 幂等 move/merge，中断后重跑即可续迁，完成写标记不再重复执行。
            {
                use crate::core::paths::{is_old_layout, migrate_v2, MIGRATED_MARKER};
                #[cfg(not(debug_assertions))]
                let old = {
                    // 优先级：exe\.minecraft（上一版便携根）> AppData（未迁移的更旧数据）
                    if is_old_layout(&game_root) {
                        Some(game_root.clone())
                    } else if is_old_layout(&app_data)
                        && !launcher_dir.join("settings.json").is_file()
                    {
                        Some(app_data.clone())
                    } else {
                        None
                    }
                };
                #[cfg(debug_assertions)]
                let old = if is_old_layout(&app_data) {
                    Some(app_data.clone())
                } else {
                    None
                };
                if let Some(old) = old {
                    if !old.join(MIGRATED_MARKER).exists() {
                        match migrate_v2(&old, &launcher_dir, &game_root) {
                            Ok(()) => {
                                let _ = std::fs::write(old.join(MIGRATED_MARKER), b"");
                                eprintln!("[migrate] v1 → v2 目录结构迁移完成：{old:?}");
                            }
                            Err(e) => {
                                eprintln!("[migrate] 目录结构迁移失败：{e}（下次启动重试）");
                            }
                        }
                    }
                }
            }

            let paths = LauncherPaths::new(launcher_dir, game_root);
            tauri::async_runtime::block_on(paths.ensure_dirs())?;
            // 加载全局设置并按其并发上限创建下载引擎
            let settings = tauri::async_runtime::block_on(AppSettings::load(&paths));
            // 初始化全局网络代理配置（各 HTTP 客户端构造时读取）
            crate::core::net::init_proxy(crate::core::net::ProxyConfig {
                mode: settings.proxy_mode.clone(),
                url: settings.proxy_url.clone(),
            });
            let download = DownloadEngine::new(
                paths.tmp_dir(),
                settings.global_concurrency as usize,
                settings.per_host_concurrency as usize,
            );
            app.manage(AppState {
                versions: VersionRepo::new(paths.clone()),
                instances: InstanceStore::new(paths.clone()),
                download,
                settings: Arc::new(tokio::sync::RwLock::new(settings)),
                active_downloads: Arc::new(std::sync::Mutex::new(
                    std::collections::HashMap::new(),
                )),
                running: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
                killed: Arc::new(std::sync::Mutex::new(std::collections::HashSet::new())),
                retry_defs: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
                natfrp: Arc::new(std::sync::Mutex::new(None)),
                paths,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::instance::list_instances,
            commands::instance::get_instance,
            commands::instance::create_instance,
            commands::instance::delete_instance,
            commands::instance::duplicate_instance,
            commands::instance::update_instance_settings,
            commands::instance::open_instance_folder,
            commands::version::list_mc_versions,
            commands::version::list_loader_versions,
            commands::settings::get_settings,
            commands::settings::save_settings,
            commands::settings::get_data_dir,
            commands::settings::open_data_dir,
            commands::settings::open_external,
            commands::settings::clear_market_cache,
            commands::settings::save_background_image,
            commands::settings::read_background_image,
            commands::settings::remove_background_image,
            commands::settings::pick_background_image,
            commands::settings::read_background_original,
            commands::pack::parse_pack_file,
            commands::pack::list_export_items,
            commands::download::pause_download,
            commands::download::resume_download,
            commands::download::cancel_download,
            commands::download::retry_download,
            commands::game::install_game_resources,
            commands::game::install_loader,
            commands::gameconfig::load_game_config,
            commands::gameconfig::save_game_config,
            commands::gameconfig::write_game_config,
            commands::java::detect_java,
            commands::java::download_java,
            commands::java::add_custom_java,
            commands::window::set_window_blur,
            commands::launch::launch_instance,
            commands::launch::kill_instance,
            commands::launch::list_running,
            commands::account::list_accounts,
            commands::account::add_offline_account,
            commands::account::set_active_account,
            commands::account::remove_account,
            commands::account::login_microsoft,
            commands::account::cancel_microsoft_login,
            commands::account::pick_skin_image,
            commands::account::apply_library_skin,
            commands::account::switch_skin_variant,
            commands::account::upload_skin,
            commands::account::apply_skin_url,
            commands::account::equip_cape,
            commands::account::unequip_cape,
            commands::account::fetch_and_apply_offline_skin,
            commands::account::remove_offline_skin,
            commands::account::set_offline_skin_variant,
            commands::account::get_offline_skin_data_url,
            commands::mods::list_mods,
            commands::mods::toggle_mod,
            commands::mods::delete_mods,
            commands::mods::list_trash,
            commands::mods::restore_mod,
            commands::mods::empty_trash,
            commands::mods::install_local_mods,
            commands::mods::search_mods,
            commands::mods::list_online_versions,
            commands::mods::plan_mod_install,
            commands::mods::confirm_mod_install,
            commands::mods::install_fabric_addons,
            commands::mods::list_optifine_forge,
            commands::mods::install_optifine,
            commands::mods::check_mod_updates,
            commands::mods::fetch_icon,
            commands::pack::pick_modpack,
            commands::pack::import_modpack,
            commands::pack::export_modpack,
            commands::servers::list_servers,
            commands::servers::add_server,
            commands::servers::update_server,
            commands::servers::remove_server,
            commands::servers::ping_server,
            commands::servers::export_servers_to_instance,
            commands::media::list_screenshots,
            commands::media::read_screenshot,
            commands::media::delete_screenshot,
            commands::media::reveal_screenshot,
            commands::media::copy_screenshot_to_clipboard,
            commands::media::list_saves,
            commands::media::read_save_icon,
            commands::media::delete_save,
            commands::media::backup_save,
            commands::media::open_saves_dir,
            commands::natfrp::natfrp_status,
            commands::natfrp::natfrp_save_config,
            commands::natfrp::natfrp_open_page,
            commands::natfrp::natfrp_pick_frpc,
            commands::natfrp::natfrp_download_frpc,
            commands::natfrp::natfrp_start,
            commands::natfrp::natfrp_stop,
            commands::natfrp::natfrp_fetch_tunnel,
            commands::natfrp::natfrp_list_tunnels,
            commands::natfrp::natfrp_list_nodes,
            commands::natfrp::natfrp_create_tunnel,
            commands::natfrp::natfrp_delete_tunnel,
        ])
        .run(tauri::generate_context!())
        .expect("启动 Tauri 应用失败");
}
