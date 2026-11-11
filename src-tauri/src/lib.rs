//! NEXUS native core. Each module owns one feature area and exposes Tauri commands.

mod activity;
mod ai;
mod db;
mod error;
mod git;
mod projects;
mod search;
mod tasks;

use tauri::Manager;

pub struct AppState {
    pub db: db::Db,
    pub engine: tasks::Engine,
    pub http: reqwest::Client,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = db::Db::open(&dir.join("nexus.db"))?;
            tasks::recover_interrupted(&db)?;
            let http = reqwest::Client::builder()
                .user_agent(concat!("nexus/", env!("CARGO_PKG_VERSION")))
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()?;
            app.manage(AppState { db, engine: tasks::Engine::default(), http });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            projects::list_projects,
            projects::add_project,
            projects::remove_project,
            projects::analyze_project,
            projects::list_dir,
            projects::read_file,
            search::search,
            git::git_status,
            git::git_branches,
            git::git_log,
            git::git_diff,
            git::git_show,
            ai::list_providers,
            ai::save_provider,
            ai::delete_provider,
            ai::list_models,
            ai::detect_local,
            ai::chat,
            ai::usage_summary,
            tasks::list_tasks,
            tasks::get_task,
            tasks::create_task,
            tasks::delete_task,
            tasks::add_step,
            tasks::delete_step,
            tasks::suggest_steps,
            tasks::plan_with_ai,
            tasks::run_task,
            tasks::cancel_task,
            tasks::approve_step,
            tasks::reset_task,
            activity::list_activity,
            activity::clear_activity,
            activity::get_stats,
        ])
        .run(tauri::generate_context!())
        .expect("error while running NEXUS");
}
