use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use sqlx::SqlitePool;

use crate::config::{EventSchema, Skill};
use crate::db::models;
use crate::db::providers::{merge_provider_models, ProviderModelInput};

async fn setup() -> SqlitePool {
    let opts = SqliteConnectOptions::new()
        .filename(":memory:")
        .create_if_missing(true)
        .foreign_keys(true);
    let pool = SqlitePoolOptions::new()
        .max_connections(1) // single connection so :memory: is shared across queries
        .connect_with(opts)
        .await
        .expect("connect :memory:");
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("migrate");
    pool
}

// `table` is only ever passed a hard-coded literal at call sites.
async fn count(pool: &SqlitePool, table: &str) -> i64 {
    let q = format!("SELECT COUNT(*) FROM {table}");
    sqlx::query_scalar(&q).fetch_one(pool).await.unwrap()
}

#[tokio::test]
async fn delete_chat_cleans_all_related_rows() {
    let pool = setup().await;
    let now: i64 = 1700000000;

    sqlx::query("INSERT INTO chats (id, project_id, title, provider_id, model_id, created_at, updated_at) VALUES ('c1', NULL, 't', 'p', 'm', ?1, ?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO messages (id, chat_id, parent_id, role, content, model, usage, is_branch_root, created_at) VALUES ('m1','c1',NULL,'user','hi',NULL,NULL,0,?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO messages (id, chat_id, parent_id, role, content, model, usage, is_branch_root, created_at) VALUES ('m2','c1','m1','assistant','ok','m','{\"prompt_tokens\":10,\"completion_tokens\":20,\"total_tokens\":30}',0,?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    // tool_calls has no FK to chats; cleaned explicitly by delete_chat.
    sqlx::query("INSERT INTO tool_calls (id, chat_id, message_id, tool_name, source, status, started_at) VALUES ('tc1','c1','m2','read','builtin','done',?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO chat_paths (id, chat_id, path, kind, watch, created_at) VALUES ('cp1','c1','/x','dir',0,?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO agent_runs (id, chat_id, parent_id, agent_connection_id, subtask_index, subtask_prompt, cwd, status, created_at) VALUES ('ar1','c1','','a',0,'p','/x','done',?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO agent_sessions (id, chat_id, agent_connection_id, agent_session_id, created_at, updated_at) VALUES ('as1','c1','a','s',?1,?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO chat_sessions (id, chat_id, summary, boundary_message_id, token_count, created_at) VALUES ('cs1','c1','sum','m1',5,?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO attachments (id, chat_id, message_id, file_name, mime_type, file_size, storage_path, is_image, created_at) VALUES ('at1','c1','m2','f.txt','text/plain',10,'/x/f',0,?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(count(&pool, "chats").await, 1);
    assert_eq!(count(&pool, "messages").await, 2);
    assert_eq!(count(&pool, "tool_calls").await, 1);

    crate::db::models::delete_chat(&pool, "c1").await.unwrap();

    assert_eq!(count(&pool, "chats").await, 0);
    assert_eq!(count(&pool, "messages").await, 0);
    assert_eq!(count(&pool, "tool_calls").await, 0);
    assert_eq!(count(&pool, "chat_paths").await, 0);
    assert_eq!(count(&pool, "agent_runs").await, 0);
    assert_eq!(count(&pool, "agent_sessions").await, 0);
    assert_eq!(count(&pool, "chat_sessions").await, 0);
    assert_eq!(count(&pool, "attachments").await, 0);

    let fts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages_fts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(fts, 0);
}

#[tokio::test]
async fn delete_project_cascades_to_chats_and_all_data() {
    let pool = setup().await;
    let now: i64 = 1700000000;

    sqlx::query("INSERT INTO projects (id, name, description, system_prompt, color, created_at, updated_at) VALUES ('proj','P','','','x',?1,?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO project_paths (id, project_id, path, kind, watch, created_at) VALUES ('pp1','proj','/proj','dir',1,?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO project_rules (id, project_id, title, text, is_active, position, created_at, updated_at) VALUES ('pr1','proj','t','prule',1,0,?1,?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

    for cid in ["cA", "cB"] {
        sqlx::query("INSERT INTO chats (id, project_id, title, provider_id, model_id, created_at, updated_at) VALUES (?1,'proj','t','p','m',?2,?2)")
            .bind(cid)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        let mid = format!("{cid}_m1");
        sqlx::query("INSERT INTO messages (id, chat_id, role, content, is_branch_root, created_at) VALUES (?1,?2,'assistant','x',0,?3)")
            .bind(&mid)
            .bind(cid)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO tool_calls (id, chat_id, message_id, tool_name, source, status, started_at) VALUES (?1,?2,?3,'t','builtin','done',?4)")
            .bind(format!("{cid}_tc"))
            .bind(cid)
            .bind(&mid)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO chat_paths (id, chat_id, path, kind, watch, created_at) VALUES (?1,?2,'/x','dir',0,?3)")
            .bind(format!("{cid}_cp"))
            .bind(cid)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO agent_runs (id, chat_id, parent_id, agent_connection_id, subtask_index, subtask_prompt, cwd, status, created_at) VALUES (?1,?2,'','a',0,'p','/x','done',?3)")
            .bind(format!("{cid}_ar"))
            .bind(cid)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO chat_sessions (id, chat_id, summary, boundary_message_id, token_count, created_at) VALUES (?1,?2,'s',?3,1,?4)")
            .bind(format!("{cid}_cs"))
            .bind(cid)
            .bind(&mid)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO attachments (id, chat_id, message_id, file_name, mime_type, file_size, storage_path, is_image, created_at) VALUES (?1,?2,?3,'f','t',1,'/x',0,?4)")
            .bind(format!("{cid}_at"))
            .bind(cid)
            .bind(&mid)
            .bind(now)
            .execute(&pool)
            .await
            .unwrap();
    }
    // Task row must come after the chats loop: chat_id references cA (FK).
    sqlx::query("INSERT INTO project_tasks (id, project_id, chat_id, title, description, status, priority, position, created_at, updated_at) VALUES ('pt1','proj','cA','do thing','desc','todo','high',0,?1,?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO project_task_changelog (id, task_id, from_status, to_status, changed_at) VALUES ('ptc1','pt1','backlog','todo',?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

    // Standalone chat outside the project — must survive the delete.
    sqlx::query("INSERT INTO chats (id, project_id, title, provider_id, model_id, created_at, updated_at) VALUES ('cOut',NULL,'out','p','m',?1,?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

    assert_eq!(count(&pool, "chats").await, 3);
    assert_eq!(count(&pool, "projects").await, 1);
    assert_eq!(count(&pool, "project_tasks").await, 1);

    crate::db::models::delete_project(&pool, "proj")
        .await
        .unwrap();

    assert_eq!(count(&pool, "projects").await, 0);
    assert_eq!(count(&pool, "project_paths").await, 0);
    assert_eq!(
        count(&pool, "project_rules").await,
        0,
        "project rules must cascade-delete with the project"
    );
    assert_eq!(
        count(&pool, "chats").await,
        1,
        "only the standalone chat remains"
    );
    assert_eq!(count(&pool, "messages").await, 0);
    assert_eq!(count(&pool, "tool_calls").await, 0);
    assert_eq!(count(&pool, "chat_paths").await, 0);
    assert_eq!(count(&pool, "agent_runs").await, 0);
    assert_eq!(count(&pool, "agent_sessions").await, 0);
    assert_eq!(count(&pool, "chat_sessions").await, 0);
    assert_eq!(count(&pool, "attachments").await, 0);
    assert_eq!(
        count(&pool, "project_tasks").await,
        0,
        "project tasks must be deleted with the project"
    );
    assert_eq!(
        count(&pool, "project_task_changelog").await,
        0,
        "task changelog must cascade-delete with tasks"
    );

    let fts: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM messages_fts")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(fts, 0);

    let out: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM chats WHERE id='cOut'")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(out, 1);
}

#[tokio::test]
async fn foreign_keys_pragma_is_enabled() {
    let pool = setup().await;
    let on: i64 = sqlx::query_scalar("PRAGMA foreign_keys")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(on, 1);
}

#[tokio::test]
async fn skills_save_list_roundtrip() {
    let pool = setup().await;

    models::save_skills(
        &pool,
        &[Skill {
            id: "s1".into(),
            title: "T".into(),
            body: "B".into(),
        }],
    )
    .await
    .unwrap();

    let skills = models::list_skills(&pool).await.unwrap();
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].id, "s1");
    assert_eq!(skills[0].title, "T");
    assert_eq!(skills[0].body, "B");

    models::save_skills(&pool, &[]).await.unwrap();
    let skills = models::list_skills(&pool).await.unwrap();
    assert!(skills.is_empty());
    assert_eq!(count(&pool, "skills").await, 0);
}

#[tokio::test]
async fn global_rules_crud_and_reorder() {
    let pool = setup().await;

    models::add_global_rule(&pool, "g1", "First", "alpha", 1000)
        .await
        .unwrap();
    models::add_global_rule(&pool, "g2", "Second", "beta", 2000)
        .await
        .unwrap();

    let rules = models::list_global_rules(&pool).await.unwrap();
    assert_eq!(rules.len(), 2);
    assert_eq!(rules[0].id, "g1");
    assert_eq!(rules[0].title, "First");
    assert_eq!(rules[0].text, "alpha");
    assert_eq!(rules[0].position, 1);
    assert_eq!(rules[0].is_active, 1);
    assert_eq!(rules[1].id, "g2");
    assert_eq!(rules[1].text, "beta");
    assert_eq!(rules[1].position, 2);

    models::update_global_rule(&pool, "g1", "First edited", "gamma", 3000)
        .await
        .unwrap();
    models::set_global_rule_active(&pool, "g2", false, 3000)
        .await
        .unwrap();
    let rules = models::list_global_rules(&pool).await.unwrap();
    assert_eq!(rules[0].title, "First edited");
    assert_eq!(rules[0].text, "gamma");
    assert_eq!(rules[0].updated_at, 3000);
    assert_eq!(rules[1].is_active, 0);

    models::reorder_global_rules(&pool, &["g2".into(), "g1".into()])
        .await
        .unwrap();
    let rules = models::list_global_rules(&pool).await.unwrap();
    assert_eq!(rules[0].id, "g2");
    assert_eq!(rules[0].position, 0);
    assert_eq!(rules[1].id, "g1");
    assert_eq!(rules[1].position, 1);

    models::delete_global_rule(&pool, "g1").await.unwrap();
    let rules = models::list_global_rules(&pool).await.unwrap();
    assert_eq!(rules.len(), 1);
    assert_eq!(rules[0].id, "g2");
}

#[tokio::test]
async fn project_rules_crud_and_independence_from_global() {
    let pool = setup().await;
    let now: i64 = 1700000000;
    sqlx::query("INSERT INTO projects (id, name, color, created_at, updated_at) VALUES ('proj','P','x',?1,?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();

    models::add_global_rule(&pool, "g1", "G", "global rule", now)
        .await
        .unwrap();
    models::add_project_rule(&pool, "p1", "proj", "P1", "project rule", now)
        .await
        .unwrap();

    let project_rules = models::list_project_rules(&pool, "proj").await.unwrap();
    assert_eq!(project_rules.len(), 1);
    assert_eq!(project_rules[0].id, "p1");
    assert_eq!(project_rules[0].project_id, "proj");
    assert_eq!(project_rules[0].title, "P1");
    assert_eq!(project_rules[0].text, "project rule");
    assert_eq!(project_rules[0].position, 1);
    assert_eq!(project_rules[0].is_active, 1);

    // Global and project rules live in separate tables.
    assert_eq!(count(&pool, "rules").await, 1);
    assert_eq!(count(&pool, "project_rules").await, 1);

    models::update_project_rule(&pool, "p1", "P1 edited", "edited", now + 1)
        .await
        .unwrap();
    models::set_project_rule_active(&pool, "p1", false, now + 1)
        .await
        .unwrap();
    let project_rules = models::list_project_rules(&pool, "proj").await.unwrap();
    assert_eq!(project_rules[0].title, "P1 edited");
    assert_eq!(project_rules[0].text, "edited");
    assert_eq!(project_rules[0].is_active, 0);

    // Reordering must be scoped to the given project only.
    sqlx::query("INSERT INTO projects (id, name, color, created_at, updated_at) VALUES ('proj2','P2','x',?1,?1)")
        .bind(now)
        .execute(&pool)
        .await
        .unwrap();
    models::add_project_rule(&pool, "p2", "proj2", "P2", "other", now)
        .await
        .unwrap();
    models::add_project_rule(&pool, "p3", "proj2", "P3", "other2", now)
        .await
        .unwrap();
    models::reorder_project_rules(&pool, "proj2", &["p3".into(), "p2".into()])
        .await
        .unwrap();
    let proj2 = models::list_project_rules(&pool, "proj2").await.unwrap();
    assert_eq!(proj2.len(), 2);
    assert_eq!(proj2[0].id, "p3");
    assert_eq!(proj2[1].id, "p2");
    let proj1 = models::list_project_rules(&pool, "proj").await.unwrap();
    assert_eq!(proj1.len(), 1);
    assert_eq!(proj1[0].id, "p1");

    models::delete_project_rule(&pool, "p1").await.unwrap();
    assert_eq!(count(&pool, "project_rules").await, 2);
}

#[tokio::test]
async fn project_include_global_rules_flag() {
    let pool = setup().await;

    let p = models::create_project(&pool, "proj", "P", "x", 1000)
        .await
        .unwrap();
    assert_eq!(p.include_global_rules, 1);

    let listed = models::list_projects(&pool).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].include_global_rules, 1);

    models::set_project_include_global_rules(&pool, "proj", false, 2000)
        .await
        .unwrap();
    let listed = models::list_projects(&pool).await.unwrap();
    assert_eq!(listed[0].include_global_rules, 0);
    let got = models::get_project(&pool, "proj").await.unwrap().unwrap();
    assert_eq!(got.include_global_rules, 0);

    // update_project round-trips the flag.
    let mut updated = got;
    updated.include_global_rules = 1;
    models::update_project(&pool, &updated, 3000).await.unwrap();
    let got = models::get_project(&pool, "proj").await.unwrap().unwrap();
    assert_eq!(got.include_global_rules, 1);
}

#[tokio::test]
async fn agents_crud_roundtrip() {
    use crate::db::agents::{self, AgentInput};

    let pool = setup().await;
    let input = AgentInput {
        name: "First".into(),
        description: "desc".into(),
        default_model: "m1".into(),
        capabilities: vec!["code_edit".into()],
        command: "claude".into(),
        args: vec!["-m".into(), "{model}".into(), "{prompt}".into()],
        prompt_mode: "stdin_json".into(),
        env: std::collections::HashMap::from([("K".into(), "v".into())]),
        output_format: "stream_json".into(),
        event_schema: Some(EventSchema {
            text_key: Some("text".into()),
            ..Default::default()
        }),
        mode_flags: std::collections::HashMap::from([("plan".into(), vec!["--plan".into()])]),
        resume_flag: "--session {session}".into(),
        timeout_ms: 60_000,
        max_turns: 10,
        ..Default::default()
    };
    let first = agents::create(&pool, input).await.unwrap();
    assert_eq!(first.name, "First");
    assert_eq!(first.default_model, "m1");
    assert_eq!(first.capabilities, vec!["code_edit".to_string()]);
    assert_eq!(
        first.args,
        vec![
            "-m".to_string(),
            "{model}".to_string(),
            "{prompt}".to_string()
        ]
    );
    assert_eq!(first.env.get("K").map(|s| s.as_str()), Some("v"));
    assert_eq!(
        first.mode_flags.get("plan").map(|v| v.as_slice()),
        Some(["--plan".to_string()].as_slice())
    );
    assert_eq!(
        first
            .event_schema
            .as_ref()
            .and_then(|s| s.text_key.as_deref()),
        Some("text")
    );
    assert_eq!(first.timeout_ms, 60_000);
    assert_eq!(first.max_turns, 10);
    assert!(first.is_active);
    assert_eq!(first.position, 1);

    let second = agents::create(
        &pool,
        AgentInput {
            name: "Second".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(second.position, 2);

    agents::reorder(&pool, &[second.id.clone(), first.id.clone()])
        .await
        .unwrap();
    let all = agents::list(&pool).await.unwrap();
    assert_eq!(
        all.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        [second.id.as_str(), first.id.as_str()]
    );
    assert_eq!(all[0].position, 0);

    agents::set_active(&pool, &first.id, false).await.unwrap();
    let active = agents::list_active(&pool).await.unwrap();
    assert_eq!(active.len(), 1);
    assert_eq!(active[0].id, second.id);

    let updated = agents::update(
        &pool,
        &second.id,
        AgentInput {
            name: "Renamed".into(),
            default_model: "gpt-5".into(),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(updated.name, "Renamed");
    let contract = updated.to_contract();
    assert_eq!(contract.id, second.id);
    assert_eq!(contract.default_model, "gpt-5");

    agents::delete(&pool, &first.id).await.unwrap();
    assert!(agents::get(&pool, &first.id).await.unwrap().is_none());
    assert_eq!(count(&pool, "agents").await, 1);
}

#[tokio::test]
async fn merge_provider_models_preserves_ids_and_cleans_refs() {
    let pool = setup().await;
    let now: i64 = 1700000000;

    sqlx::query(
        "INSERT INTO providers (id, name, kind, base_url, api_key_ref, extra_headers, timeout_ms, \
         is_active, position, created_at, updated_at) \
         VALUES ('prov1', 'Prov', 'openai', 'http://localhost', '', '{}', 30000, 1, 0, ?1, ?1)",
    )
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO provider_models (id, provider_id, name, display_name, enabled, alias, \
         capabilities, context_window, created_at, updated_at) \
         VALUES ('m1', 'prov1', 'glm-5.2', 'Old GLM', 1, '', '[]', 0, ?1, ?1)",
    )
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO provider_models (id, provider_id, name, display_name, enabled, alias, \
         capabilities, context_window, created_at, updated_at) \
         VALUES ('m2', 'prov1', 'gemma4', 'Gemma', 1, '', '[]', 0, ?1, ?1)",
    )
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO models_cache (id, provider_id, name, context_window, meta, fetched_at) \
         VALUES ('prov1::gemma4', 'prov1', 'gemma4', NULL, NULL, ?1)",
    )
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chats (id, project_id, title, provider_id, model_id, created_at, updated_at) \
         VALUES ('c1', NULL, 't', 'prov1', 'm1', ?1, ?1)",
    )
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO chats (id, project_id, title, provider_id, model_id, created_at, updated_at) \
         VALUES ('c2', NULL, 't2', 'prov1', 'm2', ?1, ?1)",
    )
    .bind(now)
    .execute(&pool)
    .await
    .unwrap();

    let input = [
        ProviderModelInput {
            name: "glm-5.2".into(),
            display_name: "GLM".into(),
            enabled: true,
            alias: String::new(),
            capabilities: vec![],
            context_window: 0,
        },
        ProviderModelInput {
            name: "kimi-k3".into(),
            display_name: "Kimi".into(),
            enabled: true,
            alias: String::new(),
            capabilities: vec![],
            context_window: 0,
        },
    ];
    let deleted = merge_provider_models(&pool, "prov1", &input).await.unwrap();

    assert_eq!(deleted.len(), 1);
    assert!(deleted.contains(&"m2".to_string()));

    let (id, display_name): (String, String) = sqlx::query_as(
        "SELECT id, display_name FROM provider_models WHERE name = 'glm-5.2' AND provider_id = 'prov1'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(id, "m1");
    assert_eq!(display_name, "GLM");

    assert_eq!(
        count(
            &pool,
            "provider_models WHERE name = 'kimi-k3' AND provider_id = 'prov1'"
        )
        .await,
        1
    );
    assert_eq!(
        count(&pool, "provider_models WHERE name = 'gemma4'").await,
        0
    );
    assert_eq!(count(&pool, "models_cache WHERE name = 'gemma4'").await, 0);

    let (model_id, provider_id): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT model_id, provider_id FROM chats WHERE id = 'c1'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(model_id.as_deref(), Some("m1"));
    assert_eq!(provider_id.as_deref(), Some("prov1"));

    let (model_id, provider_id): (Option<String>, Option<String>) =
        sqlx::query_as("SELECT model_id, provider_id FROM chats WHERE id = 'c2'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(model_id, None);
    assert_eq!(provider_id.as_deref(), Some("prov1"));
}
