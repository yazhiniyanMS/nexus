# Bugs found in the audit

Each entry: what happened, how it was reproduced (always on temporary fixtures, never on a real project or the real database), the fix, and the regression test. Security-relevant entries are explained further in [SECURITY_AUDIT.md](SECURITY_AUDIT.md), reliability ones in [RELIABILITY_AUDIT.md](RELIABILITY_AUDIT.md).

| ID | Sev | Symptom | Reproduction | Fix | Test |
| --- | --- | --- | --- | --- | --- |
| GIT-1 | P1 | Opening a project whose `.git/config` sets `core.fsmonitor`, `diff.external` or a `textconv` driver runs that program | Temp repository with all three pointing at a script that writes a marker; status/diff/show through NEXUS | `-c core.fsmonitor=false`, `--no-ext-diff --no-textconv` | `git::tests::repository_config_cannot_make_reading_run_programs` |
| GIT-2 | P2 | Untracked-file preview reads `../x` or `/abs/path` | `diff_text(root, "../secret.txt", _, true)` returned the file | `resolve_inside` | `git::tests::untracked_previews_stay_inside_the_project` |
| NET-1 | P2 | `x-api-key` sent to the server a redirect points at | Local server A redirects to server B; B received the request | Same-origin redirect policy | `ai::tests::credentials_do_not_follow_redirects_to_another_server` |
| SEC-1 | P2 | Hard-coded credentials in excerpts sent to models | Excerpt with `API_KEY = "…"`, AWS/GitHub keys and a private-key block | `mask_secrets` in read-only prompts | `projects::tests::credentials_are_masked_before_text_goes_to_a_model` |
| AGENT-2 | P2 | Starting an agent task runs the project's `post-checkout` hook | Temp repository with a marker-writing hook; `workspace::create` | Hooks disabled for `worktree add` | `agents::workspace::tests::creating_a_workspace_runs_no_repository_hooks` |
| AGENT-1 | P2 | Removing a project under a running agent; worktrees leaked and stayed registered | Project with a failed run (worktree on disk) and a running run; remove | Refuse while active; remove worktrees first; stop shells | `projects::tests::removing_a_project_cleans_up_agent_workspaces_and_refuses_while_work_runs` |
| TASK-1 | P2 | Approved command runs in `/` when the project folder is gone | Task on a temp project; delete the folder; approve a `touch` step | Fail the step; home folder for project-less tasks | `tasks::tests::a_missing_project_folder_stops_the_command_and_early_approvals_are_refused` |
| TASK-2 | P3 | `approve_step` accepted decisions for steps that were not waiting | Approve step 2 while step 1 waits | Check the stored status | same test |
| PROC-1a | P2 | 20 MB of output without a newline buffered whole before the cap | `head -c 20000000 /dev/zero \| tr '\0' x` | 8 KB chunked reads | `jobs::tests::output_without_newlines_is_bounded` |
| PROC-1b | P2 | `sleep 30 & echo started` keeps the step running 30 s; a daemon would hang it forever | As written | Grace period, then stop the group | `jobs::tests::a_command_that_leaves_a_background_process_still_finishes` |
| DB-1a | P2 | A newer schema opened by an older NEXUS without a check | `user_version` set above the known migrations | Refuse, leave untouched | `db::tests::a_newer_schema_is_refused_and_left_alone` |
| DB-1b | P2 | No backup before migrating | Database at the previous schema with a project | `VACUUM INTO nexus.db.before-v<N>` | `db::tests::an_upgrade_backs_up_the_database_first` |
| DB-1c | P2 | Damaged database: app exits, reason only on stderr | Random bytes as `nexus.db` in a throwaway `NEXUS_DATA_DIR` | Readable error + native dialog; file untouched | `db::tests::a_damaged_file_is_reported_not_overwritten`; launched manually |
| NET-2 | P3 | Reply bodies read without a size limit | 32 MB + 1 KB JSON body; 320 KB error body | `read_body` / `read_body_prefix` | `ai::tests::oversized_replies_are_refused_without_reading_them_whole` |
| NET-3 | P4 | A refused redirect showed reqwest's bare "error following redirect" | As NET-1 | Readable message naming the fix | as NET-1 |
| LINT-1 | P4 | 15 clippy warnings, so clippy could not gate CI | `cargo clippy --all-targets` | Fixed or annotated with a reason; CI runs clippy with `-D warnings` | CI |

## Edge cases exercised that showed no bug

Unicode, space and emoji file names; 60-level nesting; link loops; self-links; links out of the project; broken links; unreadable files; invalid UTF-8 inside source; empty, binary and multi-megabyte files; repositories with no commits; detached HEAD; a damaged `.git`; non-repositories (`stress_tests`). Cancel during a command kills its children (`cancelling_stops_the_command_and_its_children`).
