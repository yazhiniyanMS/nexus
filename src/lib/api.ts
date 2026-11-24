// Typed bridge to the Rust core. Every native call in the app goes through this file.
import { invoke } from "@tauri-apps/api/core";

export interface LanguageStat { name: string; files: number; lines: number; bytes: number }
export interface Dependency { name: string; version: string; dev: boolean }
export interface DependencyGroup { manifest: string; ecosystem: string; deps: Dependency[] }
export interface Issue { severity: "high" | "medium" | "low" | "info"; kind: string; message: string; file: string | null; line: number | null }
export interface Analysis {
  file_count: number; total_lines: number; total_bytes: number;
  languages: LanguageStat[]; frameworks: string[]; dependencies: DependencyGroup[];
  issues: Issue[]; top_dirs: { name: string; files: number }[]; summary: string; truncated: boolean;
}
export interface Project {
  id: string; name: string; path: string; added_at: string;
  analyzed_at: string | null; analysis: Analysis | null; exists: boolean;
}
export interface DirEntry { name: string; path: string; is_dir: boolean; size: number }
export interface FilePreview { content: string; size: number; truncated: boolean }
export interface SearchMatch { kind: "file" | "content"; project_id: string; project_name: string; path: string; line: number; text: string }
export interface SearchResult { matches: SearchMatch[]; files_scanned: number; files_skipped: number; truncated: boolean }

export interface FileStatus { path: string; orig_path: string | null; index: string; worktree: string; untracked: boolean; conflicted: boolean }
export interface GitStatus {
  is_repo: boolean; branch: string | null; upstream: string | null; ahead: number; behind: number; files: FileStatus[];
  merging: boolean; rebasing: boolean; has_commits: boolean;
}
export interface Committed { hash: string; short: string; subject: string }
export interface Branch { name: string; commit: string; upstream: string | null; date: string; current: boolean; remote: boolean; subject: string }
export interface Commit { hash: string; short: string; author: string; email: string; date: string; subject: string; refs: string }

export type ProviderKind = "openai" | "anthropic" | "ollama" | "llamacpp";
export interface Provider {
  id: string; name: string; kind: ProviderKind; base_url: string; default_model: string | null; has_key: boolean;
  /** A disabled connection is kept but never contacted. */
  enabled: boolean;
  /** True when the endpoint is on this machine, so requests do not leave it. */
  local: boolean;
}
export interface ProviderInput { id?: string; name: string; kind: ProviderKind; base_url: string; default_model?: string; api_key?: string; enabled?: boolean }
export interface ConnectionTest { ok: boolean; latency_ms: number | null; version: string | null; models: number | null; detail: string }
export interface ModelInfo { id: string; size_bytes: number | null; family: string | null; parameter_size: string | null; quantization: string | null; modified_at: string | null }
export interface LocalRuntime { name: string; kind: ProviderKind; base_url: string; running: boolean; models: ModelInfo[] }
export interface ChatMessage { role: "user" | "assistant"; content: string }
export interface ChatResponse { content: string; model: string; input_tokens: number | null; output_tokens: number | null; duration_ms: number }
export interface UsageRow { provider: string; model: string; requests: number; input_tokens: number; output_tokens: number }

export type TaskStatus = "pending" | "running" | "awaiting_approval" | "completed" | "failed" | "cancelled";
export type StepStatus = TaskStatus | "skipped";
export interface Task {
  id: string; project_id: string | null; project_name: string | null; title: string; description: string;
  status: TaskStatus; created_at: string; updated_at: string; step_count: number; steps_done: number;
  /** This task's own time limit per step in seconds; null uses the default from Settings. */
  timeout_secs: number | null;
}
export interface Step {
  id: string; task_id: string; position: number; title: string; kind: "manual" | "command"; command: string | null;
  status: StepStatus; output: string; exit_code: number | null; started_at: string | null; finished_at: string | null;
  risk: "normal" | "high";
}
export interface TaskDetail { task: Task; steps: Step[] }
export type ExecLevel = "trusted" | "restricted" | "isolated";
export interface ExecSettings {
  task_level: ExecLevel; agent_level: ExecLevel; task_timeout_secs: number; agent_timeout_secs: number; output_limit_kb: number;
  allowed_env: string[]; max_file_mb: number; memory_mb: number; max_concurrent_agents: number; max_queued_agents: number;
  max_running_tasks: number;
}
export interface ExecCapabilities {
  backend: string | null; os: string;
  levels: { level: ExecLevel; available: boolean; description: string }[];
}
export interface ActivityEntry { id: number; at: string; kind: string; message: string; project_id: string | null; project_name: string | null }
export interface Stats {
  projects: number; analyzed_projects: number; tasks_total: number; tasks_active: number; tasks_completed: number;
  providers: number; input_tokens: number; output_tokens: number;
}

export interface IndexStatus { indexed_at: string | null; files: number; symbols: number; imports: number }
export interface SymbolHit { project_id: string; project_name: string; path: string; name: string; kind: string; line: number; signature: string }
export interface Reference { path: string; line: number; text: string; is_definition: boolean }
export interface GraphNode { id: string; label: string; kind: "module" | "file" | "external"; files: number; lines: number; fan_in: number; fan_out: number; in_cycle: boolean }
export interface GraphEdge { from: string; to: string; weight: number }
export interface Graph { nodes: GraphNode[]; edges: GraphEdge[]; cycles: string[][]; truncated: boolean }
export interface FileContext { symbols: SymbolHit[]; imports: { target: string; external: boolean; line: number }[]; imported_by: string[] }
export interface Source { n: number; path: string; start_line: number; end_line: number; text: string; reason: string }
export interface Answer { answer: string; sources: Source[]; model: string; input_tokens: number | null; output_tokens: number | null; duration_ms: number }

export interface RoleInfo { id: string; name: string; responsibilities: string[]; can_read: boolean; can_write_workspace: boolean; can_run_commands: boolean }
export type AgentRunStatus = "queued" | "running" | "paused" | "completed" | "failed" | "cancelled" | "rejected" | "discarded" | "interrupted";
export type AgentStepStatus = "pending" | "running" | "awaiting_approval" | "completed" | "failed" | "skipped" | "rejected" | "cancelled";
export interface AgentRun {
  id: string; project_id: string; project_name: string | null; title: string; outcome: string; provider_id: string;
  provider_name: string | null; model: string | null; status: AgentRunStatus; error: string | null;
  created_at: string; updated_at: string; finished_at: string | null; pending_approvals: number;
}
export interface AgentStep {
  id: string; run_id: string; position: number; role: string;
  kind: "research" | "plan" | "plan_approval" | "develop" | "discover" | "command" | "review" | "apply";
  title: string; status: AgentStepStatus; detail: string; output: string; error: string | null; attempts: number;
  depends_on: string[]; started_at: string | null; finished_at: string | null;
}
export interface AgentEvent { id: number; step_id: string | null; at: string; role: string; level: string; message: string }
export interface AgentRunDetail { run: AgentRun; steps: AgentStep[]; events: AgentEvent[] }
export interface AgentDiff { patch: string; files: { status: "added" | "modified" | "deleted"; path: string }[] }

export const AGENT_CHANGED = "nexus://agent-changed";
export const AGENT_OUTPUT = "nexus://agent-output";
export const INDEX_PROGRESS = "nexus://index-progress";
export const TASK_CHANGED = "nexus://task-changed";
export const TASK_OUTPUT = "nexus://task-output";

export interface TermSession {
  id: string; project_id: string; title: string; shell: string; cwd: string;
  status: "running" | "exited" | "ended"; exit_code: number | null; pid: number | null;
  created_at: string; ended_at: string | null;
}
export interface TermScrollback { data: string; end: number }
export interface TermOutput { id: string; data: string; end: number }
export interface TermMatch { session_id: string; title: string; line: number; text: string }
export const TERM_OUTPUT = "nexus://term-output";
export const TERM_CHANGED = "nexus://term-changed";

// ---------------------------------------------------------------- Model Lab

/** Generation settings. A null field is not sent, so the runtime's own default applies. */
export interface GenParams {
  temperature: number | null; top_p: number | null; top_k: number | null; max_tokens: number | null;
  seed: number | null; repeat_penalty: number | null; think: boolean | null;
}
export type ParamName = keyof GenParams;
export interface KindInfo { kind: ProviderKind; label: string; default_url: string; needs_key: boolean; params: ParamName[]; summary: string }
export interface LabSettings { default_provider_id: string | null; default_model: string | null; storage_dir: string | null; params: GenParams }
export interface Hardware { os: string; os_version: string | null; arch: string; cpu: string | null; cores: number | null; memory_bytes: number | null }

export interface ModelMeta {
  display_name: string | null; family: string | null; architecture: string | null; parameter_count: number | null;
  parameter_label: string | null; quantization: string | null; context_length: number | null; max_output_tokens: number | null;
  capabilities: string[]; format: string | null; size_bytes: number | null; license: string | null; license_text: string | null;
  source_url: string | null; modified_at: string | null; digest: string | null; runner: string | null;
  runtime_defaults: string | null; path: string | null; notes: string[];
}
export interface ModelRecord {
  key: string; provider_id: string | null; provider_name: string | null; kind: ProviderKind | "file"; model_id: string;
  local: boolean; status: "installed" | "remote" | "file" | "disabled" | "missing"; favorite: boolean; managed: boolean; removable: boolean;
  meta: ModelMeta; meta_source: string; compatibility: string[]; first_seen: string; last_seen: string;
}
export interface SourceStatus { provider_id: string | null; name: string; kind: string; ok: boolean; error: string | null; models: number }
export interface Library { models: ModelRecord[]; sources: SourceStatus[] }

export interface Installed { path: string; version: string | null }
export interface Endpoint {
  url: string; running: boolean; version: string | null; models: number | null; detail: string | null;
  provider_id: string | null; provider_name: string | null; provider_enabled: boolean | null; managed_id: string | null;
}
export interface SetupStep { title: string; detail: string; command: string | null; url: string | null }
export interface RuntimeStatus {
  id: "ollama" | "llamacpp" | "openai"; name: string; kind: ProviderKind; installed: Installed | null;
  endpoints: Endpoint[]; can_start: boolean; setup: SetupStep[];
}
export interface ManagedInfo {
  id: string; runtime: "ollama" | "llamacpp"; url: string; port: number; model: string | null;
  status: "starting" | "running" | "unresponsive" | "exited"; exit_code: number | null; pid: number | null; started_at: string; command: string;
}
export interface RuntimeReport { runtimes: RuntimeStatus[]; managed: ManagedInfo[] }
export interface LoadedModel { name: string; size_bytes: number | null; size_vram_bytes: number | null; context_length: number | null; expires_at: string | null }

export interface StoredFile { name: string; size_bytes: number; modified_at: string | null; managed: boolean; partial: boolean }
export interface StorageInfo { dir: string; is_default: boolean; exists: boolean; free_bytes: number | null; total_bytes: number | null; used_bytes: number; files: StoredFile[] }

export interface Download {
  id: string; kind: "file" | "ollama"; source: string; name: string; provider_id: string | null; dest: string | null;
  total_bytes: number | null; received_bytes: number; sha256: string | null; verified: boolean;
  status: "downloading" | "verifying" | "completed" | "failed" | "cancelled" | "interrupted";
  detail: string; error: string | null; created_at: string; updated_at: string; finished_at: string | null;
}
export interface HfModel { id: string; downloads: number | null; likes: number | null; license: string | null; gated: boolean; updated_at: string | null }
export interface HfFile { path: string; size_bytes: number | null; sha256: string | null; url: string }
export interface HfRepo { id: string; license: string | null; gated: boolean; architecture: string | null; context_length: number | null; updated_at: string | null; page_url: string; files: HfFile[] }

/** Measurements of one request. Null means the runtime did not report it; it never means zero. */
export interface Metrics {
  ttft_ms: number | null; total_ms: number; input_tokens: number | null; output_tokens: number | null;
  tokens_per_second: number | null; tps_source: "runtime" | "derived" | null;
  load_ms: number | null; prompt_eval_ms: number | null; eval_ms: number | null; stop_reason: string | null;
}
export interface InferRequest {
  id: string; provider_id: string; model: string; system: string | null; messages: ChatMessage[]; params: GenParams; allow_remote: boolean;
}
export interface InferOutcome { text: string; thinking: string; metrics: Metrics; model: string; provider_name: string; runtime: ProviderKind; local: boolean; cancelled: boolean }
export interface InferEvent { id: string; kind: "delta" | "done" | "error"; text: string | null; thinking: string | null; outcome: InferOutcome | null; message: string | null }

/** One turn as the Playground stores it. */
export interface LabTurn {
  role: "user" | "assistant"; content: string; thinking?: string;
  metrics?: Metrics; model?: string; provider_name?: string; runtime?: string; local?: boolean; cancelled?: boolean; error?: string;
}
export interface LabSession {
  id: string; title: string; provider_id: string | null; model: string | null; system: string; params: GenParams;
  messages: LabTurn[]; created_at: string; updated_at: string;
}
export interface LabSessionSummary { id: string; title: string; provider_id: string | null; model: string | null; turns: number; updated_at: string }
export interface LabSessionInput { id?: string; title?: string; provider_id: string | null; model: string | null; system: string; params: GenParams; messages: LabTurn[] }

export interface BenchTarget { provider_id: string; model: string }
export interface BenchPrompt { label: string; text: string }
export interface BenchInput {
  name: string | null; targets: BenchTarget[]; prompts: BenchPrompt[]; system: string | null; params: GenParams;
  repeats: number; warmup: boolean; cold_start: boolean; allow_remote: boolean;
}
export interface BenchTargetInfo {
  provider_id: string; provider_name: string; runtime: string; runtime_version: string | null; endpoint: string; local: boolean;
  model: string; quantization: string | null; parameters: string | null; context_length: number | null; format: string | null;
}
export interface BenchConfig { targets: BenchTargetInfo[]; prompts: BenchPrompt[]; system: string | null; params: GenParams; repeats: number; warmup: boolean; cold_start: boolean }
export interface BenchRun {
  id: number; target: number; prompt: number; iteration: number; warmup: boolean; status: "ok" | "error" | "cancelled";
  error: string | null; metrics: Metrics; memory_bytes: number | null; memory_vram_bytes: number | null; output: string; at: string;
}
export interface Stat { n: number; mean: number; median: number; min: number; max: number; stddev: number }
export interface BenchTargetSummary {
  target: number; runs: number; errors: number; ttft_ms: Stat | null; total_ms: Stat | null; tokens_per_second: Stat | null;
  tps_source: "runtime" | "derived" | "mixed" | null; prompt_eval_ms: Stat | null; input_tokens: Stat | null; output_tokens: Stat | null;
  load_ms: number | null; memory_bytes: number | null; memory_vram_bytes: number | null;
}
export interface BenchSuite {
  id: string; name: string; config: BenchConfig; hardware: Hardware;
  status: "running" | "completed" | "cancelled" | "failed" | "interrupted"; error: string | null;
  created_at: string; finished_at: string | null; planned_runs: number; runs: BenchRun[]; summary: BenchTargetSummary[];
}
export interface BenchSuiteSummary { id: string; name: string; status: BenchSuite["status"]; created_at: string; models: string[]; prompts: number; repeats: number; runs: number }

export const LAB_INFER = "nexus://lab-infer";
export const LAB_DOWNLOAD = "nexus://lab-download";
export const LAB_BENCH = "nexus://lab-bench";
export const LAB_RUNTIME = "nexus://lab-runtime";

export interface PlatformInfo {
  app_version: string; os: "macos" | "windows" | "linux" | string; os_version: string | null; arch: string;
  data_dir: string; shell: string; git: string | null; git_version: string | null;
  secure_storage: string; data_dir_overridden: boolean;
}
export interface AppPrefs { start_view: "dashboard" | "last"; last_view: string | null }
export interface FeedbackInfo { app_version: string; diagnostics: string; links_available: boolean; repository: string | null }
export type FeedbackPage = "bug" | "feature" | "install" | "issues" | "docs" | "releases";

export const api = {
  platformInfo: () => invoke<PlatformInfo>("platform_info"),
  feedbackInfo: () => invoke<FeedbackInfo>("feedback_info"),
  /** Opens one of the project's fixed public pages in the browser; returns the address opened. */
  openFeedback: (kind: FeedbackPage) => invoke<string>("open_feedback", { kind }),
  onboardingDone: () => invoke<boolean>("onboarding_done"),
  onboardingReset: () => invoke<void>("onboarding_reset"),
  appPrefs: () => invoke<AppPrefs>("app_prefs_get"),
  appPrefsSetStart: (startView: AppPrefs["start_view"]) => invoke<AppPrefs>("app_prefs_set_start", { startView }),
  appPrefsSetLastView: (view: string) => invoke<void>("app_prefs_set_last_view", { view }),
  onboardingFinish: () => invoke<void>("onboarding_finish"),
  termShells: () => invoke<string[]>("term_shells"),

  listProjects: () => invoke<Project[]>("list_projects"),
  addProject: (path: string) => invoke<Project>("add_project", { path }),
  removeProject: (id: string) => invoke<void>("remove_project", { id }),
  analyzeProject: (id: string) => invoke<Project>("analyze_project", { id }),
  listDir: (projectId: string, path: string) => invoke<DirEntry[]>("list_dir", { projectId, path }),
  readFile: (projectId: string, path: string) => invoke<FilePreview>("read_file", { projectId, path }),
  search: (query: string, regex: boolean, caseSensitive: boolean, projectIds: string[] | null) =>
    invoke<SearchResult>("search", { query, regex, caseSensitive, projectIds }),

  gitStatus: (projectId: string) => invoke<GitStatus>("git_status", { projectId }),
  gitBranches: (projectId: string) => invoke<Branch[]>("git_branches", { projectId }),
  gitLog: (projectId: string, limit = 150) => invoke<Commit[]>("git_log", { projectId, limit }),
  gitDiff: (projectId: string, path: string, staged: boolean, untracked: boolean) =>
    invoke<string>("git_diff", { projectId, path, staged, untracked }),
  gitStage: (projectId: string, paths: string[]) => invoke<void>("git_stage", { projectId, paths }),
  gitUnstage: (projectId: string, paths: string[]) => invoke<void>("git_unstage", { projectId, paths }),
  /** Commits what is staged. Refuses with conflicts or nothing staged; never stages by itself. */
  gitCommit: (projectId: string, message: string) => invoke<Committed>("git_commit", { projectId, message }),
  gitShow: (projectId: string, hash: string) => invoke<string>("git_show", { projectId, hash }),

  rebuildIndex: (projectId: string) => invoke<IndexStatus>("rebuild_index", { projectId }),
  indexStatus: (projectId: string) => invoke<IndexStatus>("index_status", { projectId }),
  searchSymbols: (projectId: string | null, query: string, kind: string | null, limit = 100) =>
    invoke<SymbolHit[]>("search_symbols", { projectId, query, kind, limit }),
  findReferences: (projectId: string, name: string) => invoke<Reference[]>("find_references", { projectId, name }),
  dependencyGraph: (projectId: string, level: "module" | "file", depth: number, externals: boolean) =>
    invoke<Graph>("dependency_graph", { projectId, level, depth, externals }),
  fileContext: (projectId: string, path: string) => invoke<FileContext>("file_context", { projectId, path }),
  findContext: (projectId: string, question: string, focusPath: string | null) =>
    invoke<Source[]>("find_context", { projectId, question, focusPath }),
  /** `allowRemote` must be true to send project excerpts to a connection that is not on this machine. */
  askCodebase: (projectId: string, question: string, providerId: string, model: string | null, focusPath: string | null, allowRemote = false) =>
    invoke<Answer>("ask_codebase", { projectId, question, providerId, model, focusPath, allowRemote }),
  architectureDoc: (projectId: string, providerId: string | null, model: string | null, allowRemote = false) =>
    invoke<string>("architecture_doc", { projectId, providerId, model, allowRemote }),
  exportText: (suggestedName: string, content: string) => invoke<string | null>("export_text", { suggestedName, content }),
  /** Make the shareable HTML report, save it where the user picks, and open it if asked. */
  saveProjectReport: (projectId: string, open: boolean) => invoke<string | null>("save_project_report", { projectId, open }),

  agentRoles: () => invoke<RoleInfo[]>("agent_roles"),
  listAgentRuns: () => invoke<AgentRun[]>("list_agent_runs"),
  getAgentRun: (id: string) => invoke<AgentRunDetail>("get_agent_run", { id }),
  /** The task is pinned to this connection and model. `allowRemote` must be true for a connection that is not on this machine. */
  createAgentRun: (projectId: string, outcome: string, providerId: string, model: string | null, allowRemote = false) =>
    invoke<AgentRun>("create_agent_run", { projectId, outcome, providerId, model, allowRemote }),
  agentDecide: (runId: string, stepId: string, approved: boolean) => invoke<void>("agent_decide", { runId, stepId, approved }),
  agentPause: (runId: string) => invoke<void>("agent_pause", { runId }),
  agentResume: (runId: string) => invoke<void>("agent_resume", { runId }),
  agentCancel: (runId: string) => invoke<void>("agent_cancel", { runId }),
  deleteAgentRun: (id: string) => invoke<void>("delete_agent_run", { id }),
  agentDiff: (runId: string) => invoke<AgentDiff>("agent_diff", { runId }),

  termList: (projectId: string) => invoke<TermSession[]>("term_list", { projectId }),
  termOpen: (projectId: string, cwd: string | null, cols: number, rows: number, shell: string | null = null) =>
    invoke<TermSession>("term_open", { projectId, cwd, cols, rows, shell }),
  /** Keystrokes and pastes from the Terminal screen. Nothing else may call this. */
  termWrite: (id: string, data: string) => invoke<void>("term_write", { id, data }),
  termResize: (id: string, cols: number, rows: number) => invoke<void>("term_resize", { id, cols, rows }),
  termKill: (id: string) => invoke<void>("term_kill", { id }),
  termClose: (id: string) => invoke<void>("term_close", { id }),
  termRename: (id: string, title: string) => invoke<void>("term_rename", { id, title }),
  termScrollback: (id: string) => invoke<TermScrollback>("term_scrollback", { id }),
  termSearch: (projectId: string, query: string) => invoke<TermMatch[]>("term_search", { projectId, query }),
  termLayoutGet: (projectId: string) => invoke<string | null>("term_layout_get", { projectId }),
  termLayoutSet: (projectId: string, layout: string) => invoke<void>("term_layout_set", { projectId, layout }),

  labKinds: () => invoke<KindInfo[]>("lab_kinds"),
  labSettings: () => invoke<LabSettings>("lab_settings_get"),
  labSettingsSet: (defaultProviderId: string | null, defaultModel: string | null, params: GenParams) =>
    invoke<LabSettings>("lab_settings_set", { defaultProviderId, defaultModel, params }),
  labHardware: () => invoke<Hardware>("lab_hardware"),
  labModels: () => invoke<Library>("lab_models_list"),
  labModelsRefresh: () => invoke<Library>("lab_models_refresh"),
  labModelFavorite: (key: string, favorite: boolean) => invoke<void>("lab_model_favorite", { key, favorite }),
  labModelRemove: (key: string) => invoke<void>("lab_model_remove", { key }),
  labRuntimes: () => invoke<RuntimeReport>("lab_runtimes"),
  labRuntimeStart: (spec: { runtime: "ollama" | "llamacpp"; port: number; model_file: string | null; context: number | null }) =>
    invoke<ManagedInfo>("lab_runtime_start", { spec }),
  labRuntimeStop: (id: string) => invoke<void>("lab_runtime_stop", { id }),
  labRuntimeLogs: (id: string) => invoke<string>("lab_runtime_logs", { id }),
  labRuntimeDismiss: (id: string) => invoke<void>("lab_runtime_dismiss", { id }),
  labRuntimeLoaded: (providerId: string) => invoke<LoadedModel[]>("lab_runtime_loaded", { providerId }),
  labRuntimeUnload: (providerId: string, model: string) => invoke<void>("lab_runtime_unload", { providerId, model }),
  labStorage: () => invoke<StorageInfo>("lab_storage_info"),
  /** Opens a native folder picker in the core; resolves to null if the user cancels. */
  labStorageChoose: () => invoke<StorageInfo | null>("lab_storage_choose"),
  labStorageReset: () => invoke<StorageInfo>("lab_storage_reset"),
  labStorageDelete: (name: string) => invoke<StorageInfo>("lab_storage_delete", { name }),
  labHfSearch: (query: string) => invoke<HfModel[]>("lab_hf_search", { query }),
  labHfFiles: (repo: string) => invoke<HfRepo>("lab_hf_files", { repo }),
  labDownloads: () => invoke<Download[]>("lab_downloads"),
  labDownloadFile: (url: string, name: string | null, sha256: string | null) => invoke<Download>("lab_download_file", { url, name, sha256 }),
  labDownloadOllama: (providerId: string, model: string) => invoke<Download>("lab_download_ollama", { providerId, model }),
  labDownloadResume: (id: string) => invoke<Download>("lab_download_resume", { id }),
  labDownloadCancel: (id: string) => invoke<void>("lab_download_cancel", { id }),
  labDownloadRemove: (id: string) => invoke<void>("lab_download_remove", { id }),
  /** Starts a streamed request; text and the result arrive as LAB_INFER events with the request's id. */
  labInferStart: (request: InferRequest) => invoke<void>("lab_infer_start", { request }),
  labInferCancel: (id: string) => invoke<boolean>("lab_infer_cancel", { id }),
  labSessions: () => invoke<LabSessionSummary[]>("lab_sessions"),
  labSessionGet: (id: string) => invoke<LabSession>("lab_session_get", { id }),
  labSessionSave: (session: LabSessionInput) => invoke<LabSession>("lab_session_save", { session }),
  labSessionDelete: (id: string) => invoke<void>("lab_session_delete", { id }),
  labBenchStart: (input: BenchInput) => invoke<string>("lab_bench_start", { input }),
  labBenchCancel: (id: string) => invoke<boolean>("lab_bench_cancel", { id }),
  labBenchList: () => invoke<BenchSuiteSummary[]>("lab_bench_list"),
  labBenchGet: (id: string) => invoke<BenchSuite>("lab_bench_get", { id }),
  labBenchDelete: (id: string) => invoke<void>("lab_bench_delete", { id }),
  /** Saves through a native dialog opened by the core; resolves to the path, or null if cancelled. */
  labBenchExport: (id: string, format: "json" | "csv") => invoke<string | null>("lab_bench_export", { id, format }),
  labBenchExportText: (id: string, format: "json" | "csv") => invoke<string>("lab_bench_export_text", { id, format }),

  listProviders: () => invoke<Provider[]>("list_providers"),
  setProviderEnabled: (id: string, enabled: boolean) => invoke<Provider>("set_provider_enabled", { id, enabled }),
  testProvider: (id: string) => invoke<ConnectionTest>("test_provider", { id }),
  saveProvider: (input: ProviderInput) => invoke<Provider>("save_provider", { input }),
  deleteProvider: (id: string) => invoke<void>("delete_provider", { id }),
  listModels: (providerId: string) => invoke<ModelInfo[]>("list_models", { providerId }),
  detectLocal: () => invoke<LocalRuntime[]>("detect_local"),
  chat: (providerId: string, model: string | null, system: string | null, messages: ChatMessage[]) =>
    invoke<ChatResponse>("chat", { providerId, model, system, messages }),
  usageSummary: () => invoke<UsageRow[]>("usage_summary"),

  listTasks: () => invoke<Task[]>("list_tasks"),
  getTask: (id: string) => invoke<TaskDetail>("get_task", { id }),
  createTask: (title: string, description: string, projectId: string | null) =>
    invoke<Task>("create_task", { title, description, projectId }),
  deleteTask: (id: string) => invoke<void>("delete_task", { id }),
  addStep: (taskId: string, title: string, command: string | null) => invoke<void>("add_step", { taskId, title, command }),
  deleteStep: (taskId: string, stepId: string) => invoke<void>("delete_step", { taskId, stepId }),
  suggestSteps: (taskId: string) => invoke<number>("suggest_steps", { taskId }),
  planWithAi: (taskId: string, providerId: string, model: string | null) =>
    invoke<number>("plan_with_ai", { taskId, providerId, model }),
  runTask: (id: string) => invoke<void>("run_task", { id }),
  cancelTask: (id: string) => invoke<void>("cancel_task", { id }),
  setTaskTimeout: (id: string, timeoutSecs: number | null) => invoke<Task>("set_task_timeout", { id, timeoutSecs }),
  execCapabilities: () => invoke<ExecCapabilities>("exec_capabilities"),
  execSettings: () => invoke<ExecSettings>("exec_settings_get"),
  execSettingsSet: (settings: ExecSettings) => invoke<ExecSettings>("exec_settings_set", { settings }),
  approveStep: (taskId: string, stepId: string, approved: boolean) =>
    invoke<void>("approve_step", { taskId, stepId, approved }),
  resetTask: (id: string) => invoke<void>("reset_task", { id }),

  listActivity: (limit = 200) => invoke<ActivityEntry[]>("list_activity", { limit }),
  clearActivity: () => invoke<void>("clear_activity"),
  getStats: () => invoke<Stats>("get_stats"),
};
