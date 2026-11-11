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
export interface SearchMatch { project_id: string; project_name: string; path: string; line: number; text: string }
export interface SearchResult { matches: SearchMatch[]; files_scanned: number; truncated: boolean }

export interface FileStatus { path: string; orig_path: string | null; index: string; worktree: string; untracked: boolean; conflicted: boolean }
export interface GitStatus { is_repo: boolean; branch: string | null; upstream: string | null; ahead: number; behind: number; files: FileStatus[] }
export interface Branch { name: string; commit: string; upstream: string | null; date: string; current: boolean; remote: boolean; subject: string }
export interface Commit { hash: string; short: string; author: string; email: string; date: string; subject: string; refs: string }

export type ProviderKind = "openai" | "anthropic" | "ollama";
export interface Provider { id: string; name: string; kind: ProviderKind; base_url: string; default_model: string | null; has_key: boolean }
export interface ProviderInput { id?: string; name: string; kind: ProviderKind; base_url: string; default_model?: string; api_key?: string }
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
}
export interface Step {
  id: string; task_id: string; position: number; title: string; kind: "manual" | "command"; command: string | null;
  status: StepStatus; output: string; exit_code: number | null; started_at: string | null; finished_at: string | null;
  risk: "normal" | "high";
}
export interface TaskDetail { task: Task; steps: Step[] }
export interface ActivityEntry { id: number; at: string; kind: string; message: string; project_id: string | null; project_name: string | null }
export interface Stats {
  projects: number; analyzed_projects: number; tasks_total: number; tasks_active: number; tasks_completed: number;
  providers: number; input_tokens: number; output_tokens: number;
}

export const TASK_CHANGED = "nexus://task-changed";
export const TASK_OUTPUT = "nexus://task-output";

export const api = {
  listProjects: () => invoke<Project[]>("list_projects"),
  addProject: (path: string) => invoke<Project>("add_project", { path }),
  removeProject: (id: string) => invoke<void>("remove_project", { id }),
  analyzeProject: (id: string) => invoke<Project>("analyze_project", { id }),
  listDir: (projectId: string, path: string) => invoke<DirEntry[]>("list_dir", { projectId, path }),
  readFile: (projectId: string, path: string) => invoke<string>("read_file", { projectId, path }),
  search: (query: string, regex: boolean, caseSensitive: boolean, projectIds: string[] | null) =>
    invoke<SearchResult>("search", { query, regex, caseSensitive, projectIds }),

  gitStatus: (projectId: string) => invoke<GitStatus>("git_status", { projectId }),
  gitBranches: (projectId: string) => invoke<Branch[]>("git_branches", { projectId }),
  gitLog: (projectId: string, limit = 150) => invoke<Commit[]>("git_log", { projectId, limit }),
  gitDiff: (projectId: string, path: string, staged: boolean, untracked: boolean) =>
    invoke<string>("git_diff", { projectId, path, staged, untracked }),
  gitShow: (projectId: string, hash: string) => invoke<string>("git_show", { projectId, hash }),

  listProviders: () => invoke<Provider[]>("list_providers"),
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
  approveStep: (taskId: string, stepId: string, approved: boolean) =>
    invoke<void>("approve_step", { taskId, stepId, approved }),
  resetTask: (id: string) => invoke<void>("reset_task", { id }),

  listActivity: (limit = 200) => invoke<ActivityEntry[]>("list_activity", { limit }),
  clearActivity: () => invoke<void>("clear_activity"),
  getStats: () => invoke<Stats>("get_stats"),
};
