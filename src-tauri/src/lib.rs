//! NEXUS native core. Each module owns one feature area and exposes Tauri commands.

mod activity;
mod agents;
mod ai;
mod db;
#[cfg(debug_assertions)]
mod e2e;
mod error;
mod exec;
mod git;
mod intel;
mod jobs;
mod lab;
mod platform;
mod projects;
mod search;
mod secrets;
mod settings;
#[cfg(test)]
mod stress_tests;
mod tasks;
mod terminal;

use tauri::Manager;

pub struct AppState {
    pub db: db::Db,
    pub engine: tasks::Engine,
    pub agents: agents::AgentEngine,
    pub terminal: terminal::Manager,
    pub lab: lab::Lab,
    pub http: reqwest::Client,
    /// Where NEXUS keeps its own files (database, agent workspaces).
    pub data_dir: std::path::PathBuf,
}

#[cfg(test)]
impl AppState {
    /// In-memory state with a private scratch directory, for tests.
    pub fn for_tests() -> Self {
        let data_dir = std::env::temp_dir().join(format!("nexus-data-{}", db::new_id()));
        std::fs::create_dir_all(&data_dir).unwrap();
        AppState {
            db: db::Db::in_memory().unwrap(),
            engine: tasks::Engine::default(),
            agents: agents::AgentEngine::default(),
            terminal: terminal::Manager::default(),
            lab: lab::Lab::default(),
            http: ai::http_client().unwrap(),
            data_dir,
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            platform::env::warm_up();
            let dir = platform::dirs::data_dir(app.handle())?;
            let db = match db::Db::open(&dir.join("nexus.db")) {
                Ok(db) => db,
                Err(e) => {
                    // Without this the app would quit with the reason only on stderr.
                    use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
                    app.dialog().message(e.to_string()).title("NEXUS cannot start").kind(MessageDialogKind::Error).blocking_show();
                    return Err(e.into());
                }
            };
            tasks::recover_interrupted(&db)?;
            agents::recover_interrupted(&db)?;
            terminal::recover_interrupted(&db, &dir)?;
            lab::recover_interrupted(&db)?;
            lab::runtime::reap_leftovers(&dir);
            // Commands a previous run could not stop (it crashed or was killed) are stopped now.
            jobs::init(&dir);
            let http = ai::http_client()?;
            app.manage(AppState {
                db,
                engine: tasks::Engine::default(),
                agents: agents::AgentEngine::default(),
                terminal: terminal::Manager::default(),
                lab: lab::Lab::default(),
                http,
                data_dir: dir,
            });
            #[cfg(debug_assertions)]
            e2e::start(app.handle());
            // A termination signal (logout, `kill`, Ctrl+C in a terminal) ends the app the same
            // way as quitting it, so servers it started are stopped too.
            #[cfg(unix)]
            {
                let handle = app.handle().clone();
                tauri::async_runtime::spawn(async move {
                    use tokio::signal::unix::{signal, SignalKind};
                    let (Ok(mut term), Ok(mut int), Ok(mut hup)) =
                        (signal(SignalKind::terminate()), signal(SignalKind::interrupt()), signal(SignalKind::hangup()))
                    else {
                        return;
                    };
                    tokio::select! {
                        _ = term.recv() => {}
                        _ = int.recv() => {}
                        _ = hup.recv() => {}
                    }
                    jobs::stop_all();
                    if let Some(state) = handle.try_state::<AppState>() {
                        lab::shutdown(&state);
                    }
                    handle.exit(0);
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            platform::platform_info,
            platform::feedback::feedback_info,
            platform::feedback::open_feedback,
            platform::feedback::onboarding_done,
            platform::feedback::onboarding_finish,
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
            git::git_stage,
            git::git_unstage,
            git::git_commit,
            intel::rebuild_index,
            intel::index_status,
            intel::search_symbols,
            intel::find_references,
            intel::dependency_graph,
            intel::file_context,
            intel::find_context,
            intel::ask_codebase,
            intel::architecture_doc,
            intel::export_text,
            intel::save_project_report,
            agents::agent_roles,
            agents::list_agent_runs,
            agents::get_agent_run,
            agents::create_agent_run,
            agents::agent_decide,
            agents::agent_pause,
            agents::agent_resume,
            agents::agent_cancel,
            agents::delete_agent_run,
            agents::agent_diff,
            terminal::term_list,
            terminal::term_open,
            terminal::term_shells,
            terminal::term_write,
            terminal::term_resize,
            terminal::term_kill,
            terminal::term_close,
            terminal::term_rename,
            terminal::term_scrollback,
            terminal::term_search,
            terminal::term_layout_get,
            terminal::term_layout_set,
            settings::app_prefs_get,
            settings::app_prefs_set_start,
            settings::app_prefs_set_last_view,
            settings::onboarding_reset,
            exec::exec_capabilities,
            exec::exec_settings_get,
            exec::exec_settings_set,
            lab::lab_kinds,
            lab::lab_settings_get,
            lab::lab_settings_set,
            lab::hardware::lab_hardware,
            lab::registry::lab_models_list,
            lab::registry::lab_models_refresh,
            lab::registry::lab_model_favorite,
            lab::registry::lab_model_remove,
            lab::runtime::lab_runtimes,
            lab::runtime::lab_runtime_start,
            lab::runtime::lab_runtime_stop,
            lab::runtime::lab_runtime_logs,
            lab::runtime::lab_runtime_dismiss,
            lab::runtime::lab_runtime_loaded,
            lab::runtime::lab_runtime_unload,
            lab::storage::lab_storage_info,
            lab::storage::lab_storage_choose,
            lab::storage::lab_storage_reset,
            lab::storage::lab_storage_delete,
            lab::download::lab_hf_search,
            lab::download::lab_hf_files,
            lab::download::lab_downloads,
            lab::download::lab_download_file,
            lab::download::lab_download_ollama,
            lab::download::lab_download_resume,
            lab::download::lab_download_cancel,
            lab::download::lab_download_remove,
            lab::infer::lab_infer_start,
            lab::infer::lab_infer_cancel,
            lab::infer::lab_sessions,
            lab::infer::lab_session_get,
            lab::infer::lab_session_save,
            lab::infer::lab_session_delete,
            lab::bench::lab_bench_start,
            lab::bench::lab_bench_cancel,
            lab::bench::lab_bench_list,
            lab::bench::lab_bench_get,
            lab::bench::lab_bench_delete,
            lab::bench::lab_bench_export,
            lab::bench::lab_bench_export_text,
            ai::list_providers,
            ai::save_provider,
            ai::delete_provider,
            ai::set_provider_enabled,
            ai::test_provider,
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
            tasks::set_task_timeout,
            tasks::approve_step,
            tasks::reset_task,
            activity::list_activity,
            activity::clear_activity,
            activity::get_stats,
        ])
        .build(tauri::generate_context!())
        .expect("error while building NEXUS")
        .run(|app, event| {
            // Stop servers, downloads and requests the Model Lab started before the app goes away.
            if let tauri::RunEvent::Exit = event {
                // Task and agent commands end with the app; the next start marks their runs
                // interrupted so they can be resumed.
                jobs::stop_all();
                if let Some(state) = app.try_state::<AppState>() {
                    lab::shutdown(&state);
                }
            }
        });
}
