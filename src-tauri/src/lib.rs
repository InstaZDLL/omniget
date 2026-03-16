use std::collections::HashMap;
use std::sync::Arc;

use platforms::hotmart::api::Course;
use platforms::hotmart::auth::HotmartSession;
use platforms::udemy::api::UdemyCourse;
use platforms::udemy::auth::UdemySession;
use platforms::telegram::auth::{TelegramSessionHandle, TelegramState};
use tokio_util::sync::CancellationToken;

pub struct P2pSendHandle {
    pub cancel_token: CancellationToken,
    pub paused: Arc<std::sync::atomic::AtomicBool>,
}
pub type ActiveP2pSends = Arc<tokio::sync::Mutex<HashMap<String, P2pSendHandle>>>;

pub mod commands;
pub mod core;
pub mod external_url;
pub mod hotkey;
pub mod models;
pub mod native_host;
pub mod platforms;
pub mod storage;
pub mod tray;

pub struct CoursesCache {
    pub courses: Vec<Course>,
    pub fetched_at: std::time::Instant,
}

pub struct UdemyCoursesCache {
    pub courses: Vec<UdemyCourse>,
    pub fetched_at: std::time::Instant,
}

pub struct AppState {
    pub hotmart_session: Arc<tokio::sync::Mutex<Option<HotmartSession>>>,
    pub active_downloads: Arc<tokio::sync::Mutex<HashMap<u64, CancellationToken>>>,
    pub active_generic_downloads: Arc<tokio::sync::Mutex<HashMap<u64, (String, CancellationToken)>>>,
    pub active_conversions: Arc<tokio::sync::Mutex<HashMap<u64, CancellationToken>>>,
    pub registry: core::registry::PlatformRegistry,
    pub courses_cache: Arc<tokio::sync::Mutex<Option<CoursesCache>>>,
    pub session_validated_at: Arc<tokio::sync::Mutex<Option<std::time::Instant>>>,
    pub telegram_session: TelegramSessionHandle,
    pub download_queue: Arc<tokio::sync::Mutex<core::queue::DownloadQueue>>,
    pub auth_registry: core::auth::AuthRegistry,
    pub udemy_session: Arc<tokio::sync::Mutex<Option<UdemySession>>>,
    pub udemy_courses_cache: Arc<tokio::sync::Mutex<Option<UdemyCoursesCache>>>,
    pub udemy_session_validated_at: Arc<tokio::sync::Mutex<Option<std::time::Instant>>>,
    pub udemy_api_webview: Arc<tokio::sync::Mutex<Option<tauri::WebviewWindow>>>,
    pub udemy_api_result: Arc<std::sync::Mutex<Option<String>>>,
    pub torrent_session: Arc<tokio::sync::Mutex<Option<Arc<librqbit::Session>>>>,
    pub active_p2p_sends: ActiveP2pSends,
    pub frontend_ready: Arc<tokio::sync::Mutex<bool>>,
    pub pending_external_events: Arc<tokio::sync::Mutex<Vec<external_url::ExternalUrlEvent>>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt::init();

    let session = Arc::new(tokio::sync::Mutex::new(None));
    let telegram_session: TelegramSessionHandle =
        Arc::new(tokio::sync::Mutex::new(TelegramState::new()));

    let mut registry = core::registry::PlatformRegistry::new();
    registry.register(Arc::new(
        platforms::hotmart::downloader::HotmartDownloader::new(
            session.clone(),
            models::settings::AppSettings::default().download,
            20,
            3,
            8,
        ),
    ));
    registry.register(Arc::new(
        platforms::instagram::InstagramDownloader::new(),
    ));
    registry.register(Arc::new(
        platforms::pinterest::PinterestDownloader::new(),
    ));
    registry.register(Arc::new(
        platforms::tiktok::TikTokDownloader::new(),
    ));
    registry.register(Arc::new(
        platforms::twitter::TwitterDownloader::new(),
    ));
    registry.register(Arc::new(
        platforms::twitch::TwitchClipsDownloader::new(),
    ));
    registry.register(Arc::new(
        platforms::bluesky::BlueskyDownloader::new(),
    ));
    registry.register(Arc::new(
        platforms::reddit::RedditDownloader::new(),
    ));
    registry.register(Arc::new(
        platforms::youtube::YouTubeDownloader::new(),
    ));
    registry.register(Arc::new(
        platforms::vimeo::VimeoDownloader::new(),
    ));
    registry.register(Arc::new(
        platforms::telegram::downloader::TelegramDownloader::new(
            telegram_session.clone(),
        ),
    ));
    let torrent_session: Arc<tokio::sync::Mutex<Option<Arc<librqbit::Session>>>> =
        Arc::new(tokio::sync::Mutex::new(None));
    registry.register(Arc::new(
        platforms::magnet::MagnetDownloader::new(torrent_session.clone()),
    ));
    registry.register(Arc::new(
        platforms::p2p::P2pDownloader::new(),
    ));
    registry.register(Arc::new(
        platforms::generic_ytdlp::GenericYtdlpDownloader::new(),
    ));

    let auth_registry = core::auth::AuthRegistry::new();

    let state = AppState {
        hotmart_session: session,
        active_downloads: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        active_generic_downloads: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        active_conversions: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        registry,
        courses_cache: Arc::new(tokio::sync::Mutex::new(None)),
        session_validated_at: Arc::new(tokio::sync::Mutex::new(None)),
        telegram_session,
        download_queue: Arc::new(tokio::sync::Mutex::new(core::queue::DownloadQueue::new(2))),
        auth_registry,
        udemy_session: Arc::new(tokio::sync::Mutex::new(None)),
        udemy_courses_cache: Arc::new(tokio::sync::Mutex::new(None)),
        udemy_session_validated_at: Arc::new(tokio::sync::Mutex::new(None)),
        udemy_api_webview: Arc::new(tokio::sync::Mutex::new(None)),
        udemy_api_result: Arc::new(std::sync::Mutex::new(None)),
        torrent_session,
        active_p2p_sends: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
        frontend_ready: Arc::new(tokio::sync::Mutex::new(false)),
        pending_external_events: Arc::new(tokio::sync::Mutex::new(Vec::new())),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            if let Some(url) = external_url::find_external_url_arg(argv.iter().skip(1).map(|arg| arg.as_str())) {
                let app_handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = external_url::handle_external_url(&app_handle, url, "command-line").await {
                        tracing::warn!("Failed to handle external URL from second instance: {}", error);
                    }
                });
            } else {
                tray::show_window(app);
            }
        }))
        .manage(state)
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state == tauri_plugin_global_shortcut::ShortcutState::Pressed {
                        hotkey::on_hotkey_pressed(app);
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_clipboard_manager::init())
        .setup(|app| {
            let settings = storage::config::load_settings(app.handle());
            core::http_client::init_proxy(settings.proxy.clone());
            tray::setup(app.handle())?;
            hotkey::register_from_settings(app.handle());
            if let Err(error) = native_host::ensure_registered() {
                tracing::warn!("Failed to register Chrome native host: {}", error);
            }
            if let Some(url) = external_url::find_external_url_arg(std::env::args().skip(1)) {
                let app_handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(error) = external_url::handle_external_url(&app_handle, url, "command-line").await {
                        tracing::warn!("Failed to handle startup external URL: {}", error);
                    }
                });
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::auth::hotmart_login,
            commands::auth::hotmart_check_session,
            commands::auth::hotmart_logout,
            commands::courses::hotmart_list_courses,
            commands::courses::hotmart_refresh_courses,
            commands::courses::hotmart_get_modules,
            commands::diagnostics::get_rate_limit_stats,
            commands::downloads::start_course_download,
            commands::downloads::cancel_course_download,
            commands::downloads::get_active_downloads,
            commands::downloads::detect_platform,
            commands::downloads::get_media_formats,
            commands::downloads::prefetch_media_info,
            commands::downloads::download_from_url,
            commands::downloads::cancel_generic_download,
            commands::downloads::pause_download,
            commands::downloads::resume_download,
            commands::downloads::retry_download,
            commands::downloads::remove_download,
            commands::downloads::get_queue_state,
            commands::downloads::update_max_concurrent,
            commands::downloads::clear_finished_downloads,
            commands::downloads::reveal_file,
            commands::integration::register_external_frontend,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::reset_settings,
            commands::settings::mark_onboarding_complete,
            commands::autostart::set_autostart,
            commands::autostart::get_autostart_status,
            commands::telegram::telegram_check_session,
            commands::telegram::telegram_qr_start,
            commands::telegram::telegram_qr_poll,
            commands::telegram::telegram_send_code,
            commands::telegram::telegram_verify_code,
            commands::telegram::telegram_verify_2fa,
            commands::telegram::telegram_logout,
            commands::telegram::telegram_list_chats,
            commands::telegram::telegram_list_media,
            commands::telegram::telegram_download_media,
            commands::telegram::telegram_download_batch,
            commands::telegram::telegram_cancel_batch,
            commands::telegram::telegram_get_thumbnail,
            commands::telegram::telegram_get_chat_photo,
            commands::telegram::telegram_search_media,
            commands::telegram::telegram_clear_thumbnail_cache,
            commands::convert::probe_file,
            commands::convert::convert_file,
            commands::convert::cancel_conversion,
            commands::convert::get_hwaccel_info,
            commands::dependencies::check_dependencies,
            commands::dependencies::check_ytdlp_available,
            commands::dependencies::install_dependency,
            commands::search::search_videos,
            commands::platform_auth::platform_auth_check,
            commands::platform_auth::platform_auth_login,
            commands::platform_auth::platform_auth_logout,
            commands::platform_auth::platform_auth_list,
            commands::udemy_auth::udemy_login,
            commands::udemy_auth::udemy_login_cookies,
            commands::udemy_auth::udemy_get_portal,
            commands::udemy_auth::udemy_check_session,
            commands::udemy_auth::udemy_logout,
            commands::udemy_courses::udemy_list_courses,
            commands::udemy_courses::udemy_refresh_courses,
            commands::udemy_downloads::start_udemy_course_download,
            commands::udemy_downloads::cancel_udemy_course_download,
            commands::p2p::p2p_send_file,
            commands::p2p::p2p_cancel_send,
            commands::p2p::p2p_pause_send,
            commands::p2p::p2p_resume_send,
            commands::p2p::p2p_get_active_sends,
            commands::p2p::p2p_validate_code,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
