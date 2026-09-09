import { invoke } from "@tauri-apps/api/core";
import { emit, listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface ModelRef {
  provider: string;
  model: string;
}

export interface Appearance {
  theme: string;
  accent: string;
  font_size: number;
  mono_font: string;
  language: string;
  show_thinking: boolean;
  compact: boolean;
}

export interface Network {
  proxy_enabled: boolean;
  proxy_type: string;
  proxy_host: string;
  proxy_port: number;
  proxy_username: string;
  no_proxy: string;
  test_url: string;
  verify_tls: boolean;
  ca_cert_path: string;
  connect_timeout_ms: number;
}

// Sent to set_network / network_test. Adds the password (not stored in config).
export interface NetworkInput extends Network {
  password: string;
}

export interface NetworkTestResult {
  ok: boolean;
  detail: string;
  elapsed_ms: number;
  status: number | null;
}

export interface WebSearchConfig {
  user_agent: string;
  accept_language: string;
  extra_headers: string;
  timeout_ms: number;
}
export type WebSearchConfigInput = WebSearchConfig;

export interface AppConfig {
  config_version: number;
  appearance: Appearance;
  defaults: {
    mode: string;
    command_toggle: string;
    edit_toggle: string;
    main_model: ModelRef | null;
    secondary_model: ModelRef | null;
    embedding_model: ModelRef | null;
    rag_enabled: boolean;
    system_prompt: string;
    auto_collapse_context_pct: number;
    auto_pull_changes: boolean;
    delete_to_trash: boolean;
    add_environment_info: boolean;
    max_turns: number;
  };
  hotkeys: Record<string, string>;
  logging: {
    level: string;
    file_level: string;
    redact_secrets: boolean;
    max_bytes: number;
  };
  network: Network;
  web_search: WebSearchConfig;
}

export interface AppPaths {
  config_path: string;
  log_path: string;
  data_dir: string;
}

export interface Chat {
  id: string;
  project_id: string | null;
  title: string;
  provider_id: string | null;
  model_id: string | null;
  pinned: number;
  archived: number;
  meta: string | null;
  settings: string | null;
  sort_order: number;
  created_at: number;
  updated_at: number;
}

export interface ChatInfo {
  chat_id: string;
  title: string;
  project_id: string | null;
  project_name: string | null;
  provider_id: string | null;
  model_id: string | null;
  provider_name: string | null;
  model_display_name: string | null;
  model: string | null;
  created_at: number;
  updated_at: number;
  message_count: number;
  user_messages: number;
  assistant_messages: number;
  tool_messages: number;
  tool_calls: number;
  attachments: number;
  project_paths: number;
  chat_paths: number;
  rules: number;
  total_prompt_tokens: number;
  total_completion_tokens: number;
  total_tokens: number;
  total_duration_ms: number;
  total_ttft_ms: number;
  total_generation_ms: number;
  first_message_at: number | null;
  last_message_at: number | null;
}

export interface DayActivity {
  date: string;
  requests: number;
  total_tokens: number;
  messages: number;
}

export interface DayChatActivity {
  chat_id: string;
  title: string;
  project_id: string | null;
  project_name: string | null;
  requests: number;
  total_tokens: number;
  messages: number;
}

export interface DayProjectActivity {
  project_id: string | null;
  project_name: string | null;
  requests: number;
  total_tokens: number;
  messages: number;
}

export interface DayDetail {
  date: string;
  requests: number;
  total_tokens: number;
  messages: number;
  chats: DayChatActivity[];
  projects: DayProjectActivity[];
}

export interface Message {
  id: string;
  chat_id: string;
  parent_id: string | null;
  role: Role;
  content: string;
  content_parts?: string | null;
  model?: string | null;
  usage?: string | null;
  thinking_ms?: number | null;
  finish_reason?: string | null;
  is_branch_root: number;
  created_at: number;
}

export interface Project {
  id: string;
  name: string;
  description: string;
  system_prompt: string;
  default_provider_id: string | null;
  default_model_id: string | null;
  color: string;
  settings: string | null;
  pinned: number;
  sort_order: number;
  include_global_rules: number;
  created_at: number;
  updated_at: number;
}

export interface ProjectPath {
  id: string;
  project_id: string;
  path: string;
  kind: string;
  watch: number;
  exclude_globs: string | null;
  created_at: number;
}

export interface ChangedFileView {
  path: string;
  rel: string;
  kind: string;
  content: string | null;
  truncated: boolean;
}

export interface ChatPath {
  id: string;
  chat_id: string;
  path: string;
  kind: string;
  watch: number;
  exclude_globs: string | null;
  created_at: number;
}

export interface Rule {
  id: string;
  title: string;
  text: string;
  is_active: number;
  position: number;
  created_at: number;
  updated_at: number;
}

export interface ProjectRule {
  id: string;
  project_id: string;
  title: string;
  text: string;
  is_active: number;
  position: number;
  created_at: number;
  updated_at: number;
}

export interface Memory {
  id: string;
  chat_id: string;
  project_id: string | null;
  content: string;
  category: string | null;
  created_at: number;
  updated_at: number;
}

export interface Skill {
  id: string;
  title: string;
  body: string;
}

export interface PromptLaunchSettings {
  provider_id: string | null;
  model_id: string | null;
  mode: string | null;
  command_toggle: string | null;
  edit_toggle: string | null;
  thinking_enabled: boolean | null;
  thinking_effort: string | null;
}
export interface PromptThinkingInfo {
  supports: boolean;
  supports_effort: boolean;
}

export interface Prompt {
  id: string;
  title: string;
  body: string;
  project_id: string | null;
  attach_files: string[];
  skill_ids: string[];
  is_favorite: boolean;
  is_template: boolean;
  launch_settings: PromptLaunchSettings;
  position: number;
  created_at: number;
  updated_at: number;
}

export interface FtsHit {
  chat_id: string;
  message_id: string;
  rank: number;
  snippet: string;
}

export type TaskStatus = "pending" | "in_progress" | "completed" | "cancelled";

export type TaskSource = "internal" | "agent";

export interface TaskItem {
  id: string;
  chat_id: string;
  position: number;
  content: string;
  active_form: string | null;
  status: TaskStatus;
  message_id: string;
  updated_at: number;
  source: TaskSource;
  run_id: string | null;
}

export type ProjectTaskStatus = "backlog" | "todo" | "in_progress" | "review" | "done";

export type ProjectTaskPriority = "low" | "medium" | "high" | "urgent";

export interface ProjectTask {
  id: string;
  project_id: string;
  chat_id: string | null;
  title: string;
  description: string; // markdown
  status: ProjectTaskStatus;
  priority: ProjectTaskPriority;
  position: number;
  created_at: number;
  updated_at: number;
}

// --- agents (docs/07, docs/16) ---

export interface AgentRow {
  id: string;
  name: string;
  description: string;
  default_model: string;
  capabilities: string[];
  kind: string;
  command: string;
  args: string[];
  prompt_mode: string;
  cwd: string;
  env: Record<string, string>;
  output_format: string;
  event_schema: { text_key: string | null; diff_key: string | null; command_key: string | null } | null;
  mode_flags: Record<string, string[]>;
  resume_flag: string;
  timeout_ms: number;
  max_turns: number;
  is_active: boolean;
  position: number;
  created_at: number;
  updated_at: number;
}

export interface AgentInput {
  name: string;
  description: string;
  default_model: string;
  capabilities: string[];
  kind: string;
  command: string;
  args: string[];
  prompt_mode: string;
  cwd: string;
  env: Record<string, string>;
  output_format: string;
  event_schema: { text_key: string | null; diff_key: string | null; command_key: string | null } | null;
  mode_flags: Record<string, string[]>;
  resume_flag: string;
  timeout_ms: number;
  max_turns: number;
  is_active: boolean;
}

export interface AgentInfo {
  row: AgentRow;
  status: "ok" | "inactive" | "error";
  status_detail: string;
}

export interface AgentRun {
  id: string;
  chat_id: string;
  parent_tool_call_id: string | null;
  parent_id: string | null;
  agent_connection_id: string;
  subtask_index: number;
  subtask_prompt: string;
  cwd: string;
  worktree_branch: string | null;
  agent_session_id: string | null;
  status: string;
  result_summary: string | null;
  started_at: number | null;
  ended_at: number | null;
  created_at: number;
}

export type AgentEvent =
  | { kind: "text"; text: string }
  | { kind: "tool_action"; name: string; args: string }
  | { kind: "diff"; path: string; patch: string }
  | { kind: "progress"; text: string }
  | { kind: "error"; text: string };

export interface AgentProgressEvent {
  run_id: string;
  chat_id: string;
  parent_id: string | null;
  agent_id: string;
  event: AgentEvent;
}

export interface AgentStatusEvent {
  run_id: string;
  chat_id: string;
  parent_id: string | null;
  agent_id: string;
  status: string;
  result_summary: string | null;
}

export interface AgentRunFileDiff {
  path: string;
  status: string;
  additions: number;
  deletions: number;
  patch: string;
}

export interface AgentRunDiff {
  files: AgentRunFileDiff[];
  stat: string;
  total_additions: number;
  total_deletions: number;
}

export type Role = "user" | "assistant" | "system" | "tool";

export interface ContentBlock {
  id: string;
  type: "thinking" | "text" | "tool_use";
  text?: string;
  name?: string;
  tool_call_id?: string;
  input?: string;
  result?: string;
  is_error?: boolean;
  status?: "running" | "done" | "error";
}

export interface Message {
  id: string;
  chat_id: string;
  role: Role;
  content: string;
  content_parts?: string | null;
  model?: string | null;
  usage?: string | null;
  thinking_ms?: number | null;
  finish_reason?: string | null;
  created_at: number;
}

export interface ChatSession {
  id: string;
  chat_id: string;
  boundary_message_id: string;
  token_count: number;
  model: string | null;
  summary: string;
  created_at: number;
}

export interface ModelInfo {
  id: string;
  name: string;
  context_window: number | null;
}

export type ModelStatusState = "unknown" | "loaded" | "error" | "cloud";
export interface ModelStatus {
  state: ModelStatusState;
  detail: Record<string, unknown> | null;
}

export interface Usage {
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  total_duration_ms?: number | null;
  time_to_first_token_ms?: number | null;
  generation_duration_ms?: number | null;
}

export interface Attachment {
  id: string;
  chat_id: string;
  message_id: string | null;
  file_name: string;
  mime_type: string;
  file_size: number;
  storage_path: string;
  is_image: boolean;
  width: number | null;
  height: number | null;
  created_at: number;
}

// --- config ---

export const configGet = () => invoke<AppConfig>("config_get");
export const appPaths = () => invoke<AppPaths>("app_paths");
export const logsRead = (tailLines?: number) =>
  invoke<string>("logs_read", { tailLines: tailLines ?? null });
export const logsClear = () => invoke<void>("logs_clear");
export const setLogLevel = (level: string, fileLevel?: string | null) =>
  invoke<void>("set_log_level", { level, fileLevel: fileLevel ?? null });
export const setTheme = (theme: string) => invoke<void>("set_theme", { theme });
export const setNetwork = (input: NetworkInput) =>
  invoke<void>("set_network", { input });
export const networkTest = (input: NetworkInput) =>
  invoke<NetworkTestResult>("network_test", { input });
export const networkHasPassword = () => invoke<boolean>("network_has_password");
export const clearProxyPassword = () => invoke<void>("clear_proxy_password");
export const setWebSearch = (input: WebSearchConfigInput) =>
  invoke<void>("set_web_search", { input });

// --- updates ---

export interface CommitEntry {
  sha: string;
  message: string;
  author: string;
  date: string;
}

export const updateInstallSupported = () => invoke<boolean>("update_install_supported");
export const updateChangelog = (fromVersion: string, toVersion: string) =>
  invoke<CommitEntry[]>("update_changelog", { fromVersion, toVersion });

// --- chats ---

export const chatCreate = () => invoke<Chat>("chat_create");
export const chatList = () => invoke<Chat[]>("chat_list");
export const chatMessages = (chatId: string) =>
  invoke<Message[]>("chat_messages", { chatId });
export const chatRename = (chatId: string, title: string) =>
  invoke<void>("chat_rename", { chatId, title });
export const chatDelete = (chatId: string) =>
  invoke<void>("chat_delete", { chatId });
export const chatSetModel = (
  chatId: string,
  providerId: string | null,
  modelId: string | null,
) => invoke<void>("chat_set_model", { chatId, providerId, modelId });
export interface ThinkingInfo {
  supports: boolean;
  enabled: boolean;
  supports_effort: boolean;
  effort: string;
}

export const chatThinkingInfo = (chatId: string) =>
  invoke<ThinkingInfo>("chat_thinking_info", { chatId });

export const chatSetThinking = (chatId: string, enabled: boolean, effort: string) =>
  invoke<void>("chat_set_thinking", { chatId, enabled, effort });
export const chatSetMode = (chatId: string, mode: string) =>
  invoke<void>("chat_set_mode", { chatId, mode });
export const chatSetCommandToggle = (chatId: string, value: string) =>
  invoke<void>("chat_set_command_toggle", { chatId, value });
export const chatSetEditToggle = (chatId: string, value: string) =>
  invoke<void>("chat_set_edit_toggle", { chatId, value });
export const chatSend = (
  chatId: string,
  text: string,
  attachmentIds: string[],
  skillIds: string[],
) => invoke<void>("chat_send", { chatId, text, attachmentIds, skillIds });
export const chatCancel = (chatId: string) =>
  invoke<void>("chat_cancel", { chatId });
export const chatInfo = (chatId: string) => invoke<ChatInfo>("chat_info", { chatId });
export const activityDaily = () => invoke<DayActivity[]>("activity_daily");
export const activityDayDetail = (date: string) =>
  invoke<DayDetail>("activity_day_detail", { date });
export const chatSetPinned = (chatId: string, pinned: boolean) =>
  invoke<void>("chat_set_pinned", { chatId, pinned });
export const chatReorder = (orderedIds: string[]) =>
  invoke<void>("chat_reorder", { orderedIds });

// --- attachments ---

export const attachmentAdd = (chatId: string, filePath: string) =>
  invoke<Attachment>("attachment_add", { chatId, filePath });
export const attachmentRemove = (attachmentId: string) =>
  invoke<void>("attachment_remove", { attachmentId });
export const attachmentsForChat = (chatId: string) =>
  invoke<Attachment[]>("attachments_for_chat", { chatId });
export const attachmentsForMessage = (messageId: string) =>
  invoke<Attachment[]>("attachments_for_message", { messageId });
export const attachmentReadDataUrl = (attachmentId: string) =>
  invoke<string>("attachment_read_data_url", { attachmentId });
export const chatCreateInProject = (projectId: string) =>
  invoke<Chat>("chat_create_in_project", { projectId });
export const projectChats = (projectId: string) =>
  invoke<Chat[]>("project_chats", { projectId });
export const chatSetProject = (chatId: string, projectId: string | null) =>
  invoke<void>("chat_set_project", { chatId, projectId });
export const chatSetSystemPrompt = (chatId: string, systemPrompt: string | null) =>
  invoke<void>("chat_set_system_prompt", { chatId, systemPrompt });
export const chatRegenerate = (chatId: string, messageId: string) =>
  invoke<void>("chat_regenerate", { chatId, messageId });
export const chatEditMessage = (chatId: string, messageId: string, newText: string) =>
  invoke<void>("chat_edit_message", { chatId, messageId, newText });
export const chatSessionsList = (chatId: string) =>
  invoke<ChatSession[]>("chat_sessions_list", { chatId });
export const chatCompact = (chatId: string) =>
  invoke<ChatSession>("chat_compact", { chatId });
export const chatContextWindow = (chatId: string) =>
  invoke<number>("chat_context_window", { chatId });
export const chatExportMarkdown = (chatId: string) =>
  invoke<string>("chat_export_markdown", { chatId });
export const writeTextFile = (path: string, content: string) =>
  invoke<void>("write_text_file", { path, content });
export const chatBranches = (parentId: string) =>
  invoke<Message[]>("chat_branches", { parentId });
export const chatSetActiveLeaf = (chatId: string, leafId: string) =>
  invoke<void>("chat_set_active_leaf", { chatId, leafId });
export const askUserReply = (requestId: string, answer: string) =>
  invoke<boolean>("ask_user_reply", { requestId, answer });
export const taskList = (chatId: string) => invoke<TaskItem[]>("task_list", { chatId });
export const taskClear = (chatId: string) => invoke<void>("task_clear", { chatId });

// --- projects ---
export const projectCreate = (name: string, color: string) =>
  invoke<Project>("project_create", { name, color });
export const projectList = () => invoke<Project[]>("project_list");
export const projectGet = (id: string) => invoke<Project | null>("project_get", { id });
export const projectUpdate = (project: Project) =>
  invoke<void>("project_update", { project });
export const projectDelete = (id: string) => invoke<void>("project_delete", { id });
export const projectSetPinned = (projectId: string, pinned: boolean) =>
  invoke<void>("project_set_pinned", { projectId, pinned });
export const projectReorder = (orderedIds: string[]) =>
  invoke<void>("project_reorder", { orderedIds });
export interface ProjectContextSkill {
  id: string;
  title: string;
  description: string;
}
export interface ProjectContextSummary {
  rule_files: string[];
  skills: ProjectContextSkill[];
}
export const projectContextSummary = (projectId: string) =>
  invoke<ProjectContextSummary>("project_context_summary", { projectId });
export const projectPathsList = (projectId: string) =>
  invoke<ProjectPath[]>("project_paths_list", { projectId });
export const projectPathAdd = (
  projectId: string,
  path: string,
  kind: string,
  watch: boolean,
  excludeGlobs: string | null,
) => invoke<void>("project_path_add", { projectId, path, kind, watch, excludeGlobs });
export const projectPathDelete = (id: string, projectId: string) =>
  invoke<void>("project_path_delete", { id, projectId });
export const chatPathsList = (chatId: string) =>
  invoke<ChatPath[]>("chat_paths_list", { chatId });
export const chatPathAdd = (
  chatId: string,
  path: string,
  kind: string,
  watch: boolean,
  excludeGlobs: string | null,
) => invoke<void>("chat_path_add", { chatId, path, kind, watch, excludeGlobs });
export const chatPathDelete = (id: string) => invoke<void>("chat_path_delete", { id });
export const projectChangedFiles = (projectId: string) =>
  invoke<ChangedFileView[]>("project_changed_files", { projectId });
export const projectTaskList = (projectId: string) =>
  invoke<ProjectTask[]>("project_task_list", { projectId });
export const projectTaskCreate = (
  projectId: string,
  title: string,
  description: string,
  status: ProjectTaskStatus,
  priority: ProjectTaskPriority,
) => invoke<ProjectTask>("project_task_create", { projectId, title, description, status, priority });
export const projectTaskUpdate = (
  id: string,
  title: string,
  description: string,
  status: ProjectTaskStatus,
  priority: ProjectTaskPriority,
) => invoke<ProjectTask>("project_task_update", { id, title, description, status, priority });
export const projectTaskDelete = (id: string) => invoke<void>("project_task_delete", { id });
export const projectTaskMove = (id: string, toStatus: ProjectTaskStatus, toPosition: number) =>
  invoke<void>("project_task_move", { id, toStatus, toPosition });
export const projectTaskRun = (id: string) => invoke<Chat>("project_task_run", { id });

// --- prompts (reusable) ---
export const promptList = () => invoke<Prompt[]>("prompt_list");
export const promptListFavorites = () => invoke<Prompt[]>("prompt_list_favorites");
export const promptCreate = (
  title: string,
  body: string,
  projectId: string | null,
  attachFiles: string[],
  skillIds: string[],
  isFavorite: boolean,
  isTemplate: boolean,
  launchSettings: PromptLaunchSettings,
) =>
  invoke<Prompt>("prompt_create", {
    title,
    body,
    projectId,
    attachFiles,
    skillIds,
    isFavorite,
    isTemplate,
    launchSettings,
  });
export const promptUpdate = (
  id: string,
  title: string,
  body: string,
  projectId: string | null,
  attachFiles: string[],
  skillIds: string[],
  isFavorite: boolean,
  isTemplate: boolean,
  launchSettings: PromptLaunchSettings,
) =>
  invoke<Prompt>("prompt_update", {
    id,
    title,
    body,
    projectId,
    attachFiles,
    skillIds,
    isFavorite,
    isTemplate,
    launchSettings,
  });
export const promptDelete = (id: string) => invoke<void>("prompt_delete", { id });
export const promptSetFavorite = (id: string, isFavorite: boolean) =>
  invoke<void>("prompt_set_favorite", { id, isFavorite });
export const promptMove = (id: string, toPosition: number) =>
  invoke<void>("prompt_move", { id, toPosition });
export const promptRun = (id: string) => invoke<Chat>("prompt_run", { id });
export const promptThinkingInfo = (providerId: string | null, modelId: string | null) =>
  invoke<PromptThinkingInfo>("prompt_thinking_info", { providerId, modelId });

// --- rules ---
export const globalRulesList = () => invoke<Rule[]>("global_rules_list");
export const globalRuleCreate = (title: string, text: string) =>
  invoke<void>("global_rule_create", { title, text });
export const globalRuleUpdate = (id: string, title: string, text: string) =>
  invoke<void>("global_rule_update", { id, title, text });
export const globalRuleDelete = (id: string) => invoke<void>("global_rule_delete", { id });
export const globalRuleSetActive = (id: string, isActive: boolean) =>
  invoke<void>("global_rule_set_active", { id, isActive });
export const globalRuleReorder = (orderedIds: string[]) =>
  invoke<void>("global_rule_reorder", { orderedIds });

export const projectRulesList = (projectId: string) =>
  invoke<ProjectRule[]>("project_rules_list", { projectId });
export const projectRuleCreate = (projectId: string, title: string, text: string) =>
  invoke<void>("project_rule_create", { projectId, title, text });
export const projectRuleUpdate = (id: string, title: string, text: string) =>
  invoke<void>("project_rule_update", { id, title, text });
export const projectRuleDelete = (id: string) => invoke<void>("project_rule_delete", { id });
export const projectRuleSetActive = (id: string, isActive: boolean) =>
  invoke<void>("project_rule_set_active", { id, isActive });
export const projectRuleReorder = (projectId: string, orderedIds: string[]) =>
  invoke<void>("project_rule_reorder", { projectId, orderedIds });
export const projectSetIncludeGlobalRules = (projectId: string, include: boolean) =>
  invoke<void>("project_set_include_global_rules", { projectId, include });

export const memoryListChat = (chatId: string) =>
  invoke<Memory[]>("memory_list_chat", { chatId });
export const memoryListProject = (projectId: string) =>
  invoke<Memory[]>("memory_list_project", { projectId });
export const memoryDelete = (id: string) => invoke<void>("memory_delete", { id });
export const memoryClearChat = (chatId: string) =>
  invoke<void>("memory_clear_chat", { chatId });
export const memoryClearProject = (projectId: string) =>
  invoke<void>("memory_clear_project", { projectId });

export const setSystemPrompt = (text: string) => invoke<void>("set_system_prompt", { text });

export const environmentGet = () => invoke<string>("environment_get");
export const environmentSave = (text: string) =>
  invoke<void>("environment_save", { text });
export const environmentDetect = () => invoke<string>("environment_detect");
export const setAddEnvironmentInfo = (value: boolean) =>
  invoke<void>("set_add_environment_info", { value });

export type WebSearchProviderKind =
  | "scrape"
  | "brave_api"
  | "tavily_api"
  | "serper_api"
  | "exa_api"
  | "custom_api";

export interface WebSearchProvider {
  id: string;
  title: string;
  url: string;
  enabled: boolean;
  position: number;
  kind: WebSearchProviderKind;
  api_method: string;
  auth_scheme: string; // "none" | "header" | "bearer"
  auth_header: string;
  body_template: string;
  results_path: string;
  title_field: string;
  url_field: string;
  snippet_field: string;
  has_key: boolean;
}
export interface WebSearchProviderInput {
  id: string;
  title: string;
  url: string;
  enabled: boolean;
  kind: WebSearchProviderKind;
  api_method: string;
  auth_scheme: string;
  auth_header: string;
  body_template: string;
  results_path: string;
  title_field: string;
  url_field: string;
  snippet_field: string;
}
export const webSearchProvidersList = () =>
  invoke<WebSearchProvider[]>("web_search_providers_list");
export const webSearchProvidersSave = (providers: WebSearchProviderInput[]) =>
  invoke<void>("web_search_providers_save", { providers });
export const webSearchProviderSetKey = (id: string, key: string) =>
  invoke<void>("web_search_provider_set_key", { id, key });
export const webSearchProviderClearKey = (id: string) =>
  invoke<void>("web_search_provider_clear_key", { id });
export const webSearchProviderHasKey = (id: string) =>
  invoke<boolean>("web_search_provider_has_key", { id });

export const skillsList = () => invoke<Skill[]>("skills_list");
export const skillsSave = (skills: Skill[]) =>
  invoke<void>("skills_save", { skills });

// --- search ---
export const searchMessages = (query: string, projectId: string | null) =>
  invoke<FtsHit[]>("search_messages", { query, projectId });

// --- agents (DB-backed, multi) ---
export const agentList = () => invoke<AgentInfo[]>("agent_list");
export const agentCreate = (input: AgentInput) =>
  invoke<AgentRow>("agent_create", { input });
export const agentUpdate = (id: string, input: AgentInput) =>
  invoke<AgentRow>("agent_update", { id, input });
export const agentDelete = (id: string) => invoke<void>("agent_delete", { id });
export const agentReorder = (orderedIds: string[]) =>
  invoke<void>("agent_reorder", { orderedIds });
export const agentSetActive = (id: string, isActive: boolean) =>
  invoke<void>("agent_set_active", { id, isActive });
export const agentPresets = () => invoke<AgentInput[]>("agent_presets");
export const agentTest = (id: string) => invoke<string>("agent_test", { id });
export const agentTestInput = (input: AgentInput) =>
  invoke<string>("agent_test_input", { input });
export const agentDetect = (command: string) =>
  invoke<string | null>("agent_detect", { command });
export const agentRunsList = (chatId: string | null) =>
  invoke<AgentRun[]>("agent_runs_list", { chatId });
export const agentRunCancel = (runId: string) =>
  invoke<void>("agent_run_cancel", { runId });
export const agentRunApprove = (runId: string) =>
  invoke<string>("agent_run_approve", { runId });
export const agentRunReject = (runId: string) =>
  invoke<void>("agent_run_reject", { runId });
export const agentRunDiff = (runId: string) =>
  invoke<AgentRunDiff>("agent_run_diff", { runId });

export function onAgentsChanged(cb: () => void): Promise<UnlistenFn> {
  return listen("agents:changed", () => cb());
}

// --- providers (DB-backed, multi) ---

export interface ProviderRow {
  id: string;
  name: string;
  kind: string;
  base_url: string;
  api_key_ref: string;
  extra_headers: Record<string, string>;
  timeout_ms: number;
  is_active: boolean;
  position: number;
  created_at: number;
  updated_at: number;
}
export interface ProviderInput {
  name: string;
  kind: string;
  base_url: string;
  api_key_ref: string;
  extra_headers: Record<string, string>;
  timeout_ms: number;
  is_active: boolean;
}
export interface ProviderModel {
  id: string;
  provider_id: string;
  name: string;
  display_name: string;
  enabled: boolean;
  alias: string;
  capabilities: string[];
  context_window: number;
}
export interface ProviderModelInput {
  name: string;
  display_name: string;
  enabled: boolean;
  alias: string;
  capabilities: string[];
  context_window: number;
}
export type ProviderStatusState = "active" | "inactive" | "unreachable";
export interface ProviderStatus {
  state: ProviderStatusState;
  detail: string | null;
}
export interface ModelOption {
  provider_id: string;
  provider_name: string;
  model_id: string;
  model_name: string;
  display_name: string;
  is_active: boolean;
  enabled: boolean;
}

export const providersList = () => invoke<ProviderRow[]>("providers_list");
export const providersCreate = (input: ProviderInput, apiKey?: string) =>
  invoke<ProviderRow>("providers_create", { input, apiKey: apiKey ?? null });
export const providersUpdate = (id: string, input: ProviderInput, apiKey?: string) =>
  invoke<ProviderRow>("providers_update", { id, input, apiKey: apiKey ?? null });
export const providersDelete = (id: string) => invoke<void>("providers_delete", { id });
export const providersReorder = (orderedIds: string[]) =>
  invoke<void>("providers_reorder", { orderedIds });
export const providersSetActive = (id: string, isActive: boolean) =>
  invoke<void>("providers_set_active", { id, isActive });
export const providerModelsList = (providerId: string) =>
  invoke<ProviderModel[]>("provider_models_list", { providerId });
export const providerModelsFetch = (providerId: string) =>
  invoke<ModelInfo[]>("provider_models_fetch", { providerId });
export const providerModelsSave = (providerId: string, models: ProviderModelInput[]) =>
  invoke<void>("provider_models_save", { providerId, models });
export const providerStatus = (id: string) =>
  invoke<ProviderStatus>("provider_status", { id });
export const providerModelStatus = (providerId: string, modelId: string) =>
  invoke<ModelStatus>("provider_model_status", { providerId, modelId });
export const providersActiveModels = () =>
  invoke<ModelOption[]>("providers_active_models");
export const providersAllModels = () =>
  invoke<ModelOption[]>("providers_all_models");
export const setDefaultsModel = (
  field: "main_model" | "secondary_model" | "embedding_model",
  provider: string | null,
  model: string | null,
) => invoke<void>("set_defaults_model", { field, provider, model });
export const setRagEnabled = (value: boolean) =>
  invoke<void>("set_rag_enabled", { value });

export function onProvidersChanged(cb: () => void): Promise<UnlistenFn> {
  return listen("providers:changed", () => cb());
}

export interface RagStatus {
  chat_chunks: number;
  project_chunks: number;
  total_chunks: number;
}
export const ragReindexProject = (projectId: string) =>
  invoke<number>("rag_reindex_project", { projectId });
export const ragClearProject = (projectId: string) =>
  invoke<void>("rag_clear_project", { projectId });
export const ragStatus = (projectId: string | null, chatId: string | null) =>
  invoke<RagStatus>("rag_status", { projectId: projectId ?? null, chatId: chatId ?? null });
export const ragClearAll = () => invoke<void>("rag_clear_all");
export function onRagCleared(cb: () => void): Promise<UnlistenFn> {
  return listen("rag:cleared", () => cb());
}
export const setMode = (mode: string) => invoke<void>("set_mode", { mode });
export const setCommandToggle = (value: string) =>
  invoke<void>("set_command_toggle", { value });
export const setEditToggle = (value: string) =>
  invoke<void>("set_edit_toggle", { value });
export const setAutoCollapseContextPct = (value: number) =>
  invoke<void>("set_auto_collapse_context_pct", { value });
export const setMaxTurns = (value: number) =>
  invoke<void>("set_max_turns", { value });
export const setAutoPullChanges = (value: boolean) =>
  invoke<void>("set_auto_pull_changes", { value });
export const setDeleteToTrash = (value: boolean) =>
  invoke<void>("set_delete_to_trash", { value });
export const approveRequest = (requestId: string, approved: boolean) =>
  invoke<boolean>("approve_request", { requestId, approved });

// --- MCP (DB-backed) ---

export interface McpBody {
  command?: string;
  args?: string[];
  env?: Record<string, string>;
  url?: string;
  headers?: Record<string, string>;
}
export type McpStatus =
  | "disabled"
  | "connecting"
  | "connected"
  | "needs_auth"
  | { error: string };
export interface McpServerInfo {
  id: string; // UUID
  name: string; // slug, used for mcp__<name>__<tool>
  title: string; // display title
  is_active: boolean;
  transport: string; // "stdio" | "http"
  status: McpStatus;
  tool_count: number;
  tools: string[];
  body: McpBody;
  webui_url: string;
  webui_icon: string;
  position: number;
  needs_auth: boolean;
  auth_expired: boolean;
  oauth_authenticated: boolean;
}
export interface McpServerInput {
  title: string;
  name: string;
  body: McpBody;
  is_active: boolean;
}
export interface McpTestResult {
  ok: boolean;
  tool_count: number;
  tools: string[];
  error: string | null;
  needs_auth: boolean;
}
export const mcpList = () => invoke<McpServerInfo[]>("mcp_list");
export const mcpRefresh = () => invoke<void>("mcp_refresh");
export const mcpTestDef = (body: McpBody) => invoke<McpTestResult>("mcp_test_def", { body });
export const mcpCreate = (input: McpServerInput) => invoke<string>("mcp_create", { input });
export const mcpUpdate = (id: string, input: McpServerInput) =>
  invoke<void>("mcp_update", { id, input });
export const mcpDelete = (id: string) => invoke<void>("mcp_delete", { id });
export const mcpReorder = (orderedIds: string[]) =>
  invoke<void>("mcp_reorder", { orderedIds });
export const mcpSetActive = (id: string, isActive: boolean) =>
  invoke<void>("mcp_set_active", { id, isActive });

// --- MCP Web UI ---

export interface McpWebUiEntry { server_id: string; title: string; url: string; icon_data_url: string | null; }
export interface McpWebUiDetectResult { ok: boolean; url: string | null; }
export const mcpWebUiSave = (id: string, url: string, favicon: string | null) =>
  invoke<void>("mcp_webui_save", { id, url, favicon });
export const mcpWebUiDelete = (id: string) => invoke<void>("mcp_webui_delete", { id });
export const mcpWebUiList = () => invoke<McpWebUiEntry[]>("mcp_webui_list");
export const mcpWebUiOpen = (id: string, theme?: string) =>
  invoke<void>("mcp_webui_open", { id, theme: theme ?? null });
export const mcpWebUiDetectFavicon = (siteUrl: string, current?: string | null) =>
  invoke<McpWebUiDetectResult>("mcp_webui_detect_favicon", { siteUrl, current: current ?? null });
export function onMcpChanged(cb: () => void): Promise<UnlistenFn> {
  return listen("mcp:changed", () => cb());
}

// --- MCP OAuth ---

export interface McpOAuthStatus {
  authenticated: boolean;
  expired: boolean;
  has_refresh_token: boolean;
  expires_at: number; // unix ms, 0 = unknown
  auth_server_issuer: string;
}
export const mcpOAuthStart = (serverId: string) =>
  invoke<void>("mcp_oauth_start", { serverId });
export const mcpOAuthStatus = (serverId: string) =>
  invoke<McpOAuthStatus>("mcp_oauth_status", { serverId });
export const mcpOAuthRefresh = (serverId: string) =>
  invoke<void>("mcp_oauth_refresh", { serverId });
export const mcpOAuthRevoke = (serverId: string) =>
  invoke<void>("mcp_oauth_revoke", { serverId });

// --- Web Hooks (DB-backed) ---

export interface WebHookView {
  id: string;
  title: string;
  name: string; // slug, referenced by the model in web_hook_run
  description: string; // shown to the model in web_hook_list
  method: string; // "GET" | "POST" | ...
  url: string; // may contain {{secret}} / {{variables.KEY}} / {{payload}}
  headers: string; // JSON object as string, e.g. '{"Content-Type":"application/json"}'
  body_template: string; // empty = raw payload; otherwise template with {{payload}}/{{variables.KEY}}/{{secret}}
  auth_type: string; // "none" | "bearer" | "basic" | "api_key_header" | "api_key_query"
  auth_username: string;
  auth_header_name: string;
  auth_param_name: string;
  has_secret: boolean;
  timeout_ms: number;
  is_active: boolean;
  position: number;
}
export interface WebHookInput {
  title: string;
  name: string;
  description: string;
  method: string;
  url: string;
  headers: string;
  body_template: string;
  auth_type: string;
  auth_username: string;
  auth_header_name: string;
  auth_param_name: string;
  timeout_ms: number;
  is_active: boolean;
}
export interface WebHookTestResult {
  ok: boolean;
  status: number | null;
  detail: string;
  elapsed_ms: number;
  body: string;
}
export const webHooksList = () => invoke<WebHookView[]>("web_hooks_list");
export const webHooksCreate = (input: WebHookInput) =>
  invoke<void>("web_hooks_create", { input });
export const webHooksUpdate = (id: string, input: WebHookInput) =>
  invoke<void>("web_hooks_update", { id, input });
export const webHooksDelete = (id: string) => invoke<void>("web_hooks_delete", { id });
export const webHooksReorder = (orderedIds: string[]) =>
  invoke<void>("web_hooks_reorder", { orderedIds });
export const webHooksSetActive = (id: string, isActive: boolean) =>
  invoke<void>("web_hooks_set_active", { id, isActive });
export const webHooksSetSecret = (id: string, secret: string) =>
  invoke<void>("web_hooks_set_secret", { id, secret });
export const webHooksClearSecret = (id: string) =>
  invoke<void>("web_hooks_clear_secret", { id });
export const webHooksHasSecret = (id: string) =>
  invoke<boolean>("web_hooks_has_secret", { id });
export const webHooksTest = (id: string, payload: string | null) =>
  invoke<WebHookTestResult>("web_hooks_test", { id, payload });
export function onWebHooksChanged(cb: () => void): Promise<UnlistenFn> {
  return listen("web_hooks:changed", () => cb());
}

// --- Pending changes (snapshot + revert) ---

export type ChangeKind = "created" | "modified" | "deleted";
export interface ChangeInfo {
  path: string;
  kind: ChangeKind;
  diff: string;
}
export interface PendingUpdateEvent {
  chat_id: string;
  changes: ChangeInfo[];
}
export const pendingList = (chatId: string) =>
  invoke<ChangeInfo[]>("pending_list", { chatId });
export const pendingApprove = (chatId: string) =>
  invoke<void>("pending_approve", { chatId });
export const pendingReject = (chatId: string) =>
  invoke<void>("pending_reject", { chatId });

// --- Tools ---
export interface ToolInfo {
  name: string;
  category: string;
  description: string;
  enabled: boolean;
}
export const toolsList = () => invoke<ToolInfo[]>("tools_list");
export const toolsSetEnabled = (name: string, enabled: boolean) =>
  invoke<void>("tools_set_enabled", { name, enabled });
export function onChatPendingUpdate(cb: (e: PendingUpdateEvent) => void): Promise<UnlistenFn> {
  return listen<PendingUpdateEvent>("chat:pending_update", (e) => cb(e.payload));
}

export interface MemoryUpdateEvent {
  chat_id: string | null;
  project_id: string | null;
}
export function onChatMemoryUpdate(cb: (e: MemoryUpdateEvent) => void): Promise<UnlistenFn> {
  return listen<MemoryUpdateEvent>("chat:memory_update", (e) => cb(e.payload));
}

// --- PTY (interactive commands) ---
export interface PtyStartEvent {
  chat_id: string;
  message_id: string;
  block_id: string;
  session_id: string;
  command: string;
}
export interface PtyOutputEvent {
  chat_id: string;
  message_id: string;
  block_id: string;
  session_id: string;
  data: string;
}
export interface PtyDoneEvent {
  chat_id: string;
  message_id: string;
  block_id: string;
  session_id: string;
  code: number;
}
export function onChatPtyStart(cb: (e: PtyStartEvent) => void): Promise<UnlistenFn> {
  return listen<PtyStartEvent>("chat:pty_start", (e) => cb(e.payload));
}
export function onChatPtyOutput(cb: (e: PtyOutputEvent) => void): Promise<UnlistenFn> {
  return listen<PtyOutputEvent>("chat:pty_output", (e) => cb(e.payload));
}
export function onChatPtyDone(cb: (e: PtyDoneEvent) => void): Promise<UnlistenFn> {
  return listen<PtyDoneEvent>("chat:pty_done", (e) => cb(e.payload));
}
export const ptyInput = (sessionId: string, data: string) =>
  invoke<void>("pty_input", { sessionId, data });
export const setApiKey = (refId: string, key: string) =>
  invoke<void>("set_api_key", { refId, key });
export const deleteApiKey = (refId: string) =>
  invoke<void>("delete_api_key", { refId });

// --- events ---

export type ChatStatus = "idle" | "running" | "error" | "cancelled";

export interface StatusEvent {
  chat_id: string;
  status: ChatStatus;
  detail: string | null;
}
export interface BlockStartEvent {
  chat_id: string;
  message_id: string;
  block_id: string;
  block_type: "thinking" | "text" | "tool_use";
  info?: { name: string; tool_call_id: string } | null;
}
export interface BlockDeltaEvent {
  chat_id: string;
  message_id: string;
  block_id: string;
  text?: string | null;
  partial_json?: string | null;
}
export interface BlockStopEvent {
  chat_id: string;
  message_id: string;
  block_id: string;
}
export interface ToolResultEvent {
  chat_id: string;
  message_id: string;
  block_id: string;
  result: string;
  is_error: boolean;
}
export interface ApprovalRequestEvent {
  request_id: string;
  chat_id: string;
  message_id: string;
  block_id: string;
  tool_name: string;
  summary: string;
  preview: string | null;
  path: string | null;
  symlink_target: string | null;
  escape: boolean | null;
  destructive_mode: string | null;
}
export interface MessageDoneEvent {
  chat_id: string;
  message_id: string;
  usage: Usage | null;
  finish_reason: string;
}

export function onConfigReloaded(cb: () => void): Promise<UnlistenFn> {
  return listen("config:reloaded", () => cb());
}
export function onSkillsReloaded(cb: () => void): Promise<UnlistenFn> {
  return listen("skills:reloaded", () => cb());
}
export function onChatStatus(cb: (e: StatusEvent) => void): Promise<UnlistenFn> {
  return listen<StatusEvent>("chat:status", (e) => cb(e.payload));
}
export function onChatBlockStart(cb: (e: BlockStartEvent) => void): Promise<UnlistenFn> {
  return listen<BlockStartEvent>("chat:block_start", (e) => cb(e.payload));
}
export function onChatBlockDelta(cb: (e: BlockDeltaEvent) => void): Promise<UnlistenFn> {
  return listen<BlockDeltaEvent>("chat:block_delta", (e) => cb(e.payload));
}
export function onChatBlockStop(cb: (e: BlockStopEvent) => void): Promise<UnlistenFn> {
  return listen<BlockStopEvent>("chat:block_stop", (e) => cb(e.payload));
}
export function onChatToolResult(cb: (e: ToolResultEvent) => void): Promise<UnlistenFn> {
  return listen<ToolResultEvent>("chat:tool_result", (e) => cb(e.payload));
}
export function onChatApprovalRequest(
  cb: (e: ApprovalRequestEvent) => void,
): Promise<UnlistenFn> {
  return listen<ApprovalRequestEvent>("chat:approval_request", (e) => cb(e.payload));
}
export function onChatMessageDone(cb: (e: MessageDoneEvent) => void): Promise<UnlistenFn> {
  return listen<MessageDoneEvent>("chat:message_done", (e) => cb(e.payload));
}
export interface TurnErrorEvent {
  chat_id: string;
  message_id: string;
  kind: string;
  message: string;
  retryable: boolean;
}
export function onChatTurnError(cb: (e: TurnErrorEvent) => void): Promise<UnlistenFn> {
  return listen<TurnErrorEvent>("chat:turn_error", (e) => cb(e.payload));
}
export interface StreamRetryEvent {
  chat_id: string;
  message_id: string;
  attempt: number;
  max_attempts: number;
  detail: string;
}
export function onChatStreamRetry(cb: (e: StreamRetryEvent) => void): Promise<UnlistenFn> {
  return listen<StreamRetryEvent>("chat:stream_retry", (e) => cb(e.payload));
}
export function onCompacted(cb: (s: ChatSession) => void): Promise<UnlistenFn> {
  return listen<ChatSession>("chat:compacted", (e) => cb(e.payload));
}

export interface ChatRenamedEvent {
  chat_id: string;
  title: string;
}
export function onChatRenamed(cb: (e: ChatRenamedEvent) => void): Promise<UnlistenFn> {
  return listen<ChatRenamedEvent>("chat:renamed", (e) => cb(e.payload));
}

export interface AskUserEvent {
  request_id: string;
  chat_id: string;
  message_id: string;
  block_id: string;
  question: string;
  options: string[];
  multi_select: boolean;
  context: string | null;
}
export function onChatAskUser(cb: (e: AskUserEvent) => void): Promise<UnlistenFn> {
  return listen<AskUserEvent>("chat:ask_user", (e) => cb(e.payload));
}

export interface TasksUpdateEvent {
  chat_id: string;
  tasks: TaskItem[];
}
export function onChatTasksUpdate(cb: (e: TasksUpdateEvent) => void): Promise<UnlistenFn> {
  return listen<TasksUpdateEvent>("chat:tasks_update", (e) => cb(e.payload));
}

export interface ProjectTaskChangedEvent {
  project_id: string;
}
export function onProjectTaskChanged(cb: (e: ProjectTaskChangedEvent) => void): Promise<UnlistenFn> {
  return listen<ProjectTaskChangedEvent>("project_task_changed", (e) => cb(e.payload));
}

export interface FileChangedEvent {
  project_id: string;
  path: string;
  kind: string;
}
export function onProjectFileChanged(cb: (e: FileChangedEvent) => void): Promise<UnlistenFn> {
  return listen<FileChangedEvent>("project:file_changed", (e) => cb(e.payload));
}

export function onAgentProgress(
  cb: (e: AgentProgressEvent) => void,
): Promise<UnlistenFn> {
  return listen<AgentProgressEvent>("agent:progress", (e) => cb(e.payload));
}

export function onAgentStatus(
  cb: (e: AgentStatusEvent) => void,
): Promise<UnlistenFn> {
  return listen<AgentStatusEvent>("agent:status", (e) => cb(e.payload));
}

export interface AgentBatchSummary {
  run_id: string;
  subtask_index: number;
  status: string;
  summary: string;
}
export interface AgentBatchCompleteEvent {
  parent_id: string;
  chat_id: string;
  agent_id: string;
  run_ids: string[];
  any_error: boolean;
  summaries: AgentBatchSummary[];
}
export function onAgentBatchComplete(
  cb: (e: AgentBatchCompleteEvent) => void,
): Promise<UnlistenFn> {
  return listen<AgentBatchCompleteEvent>("agent:batch_complete", (e) => cb(e.payload));
}

// --- settings window ---

export async function openSettingsWindow(tab?: string): Promise<void> {
  await invoke("open_settings_window", { tab: tab ?? null });
}

export async function takeSettingsTab(): Promise<string | null> {
  return invoke<string | null>("take_settings_tab");
}

export function onSettingsNavigate(cb: (tab: string) => void): Promise<UnlistenFn> {
  return listen<string>("settings:navigate", (e) => cb(e.payload));
}

export function onLangChanged(cb: () => void): Promise<UnlistenFn> {
  return listen("lang:changed", () => cb());
}

export function emitLangChanged(): Promise<void> {
  return emit("lang:changed");
}