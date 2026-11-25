//! The OpenAI-compatible backend against a loopback server.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

mod support;

use std::sync::{Arc, Mutex};

use ai_pool::AiClient;
use mareader_assist::{Compat, Explain, run};
use mareader_study::{Why, WordInfo};
use support::{Answer, QUICK, Server};

const FOX: Explain<'static> = Explain {
    word: "quick",
    context: "The quick brown fox",
    language: None,
    hint: None,
};

async fn keyless(url: &str) -> Arc<AiClient> {
    Arc::new(
        AiClient::builder()
            .default_url(url)
            .keyless()
            .build()
            .await
            .unwrap(),
    )
}

#[tokio::test]
async fn answers_stream_in_and_the_schema_is_sent() {
    let pieces = [
        r#"{"pos":"adjective","mean"#,
        r#"ing":"Fa"#,
        r#"st.","synonyms":["swift","rapid"],"#,
        r#""usages":["Be quick.","A quick look."]}"#,
    ];
    let server = Server::start(vec![Answer::stream(&pieces)]).await;
    let compat = Compat::new(keyless(&server.url).await, "m1");
    let seen = Mutex::new(Vec::<WordInfo>::new());
    let push = |i| seen.lock().unwrap().push(i);
    let info = run(&compat, &FOX, &push).await.unwrap();
    assert_eq!(info.meaning, "Fast.");
    assert_eq!(info.synonyms, ["swift", "rapid"]);
    let seen = seen.into_inner().unwrap();
    assert!(seen.iter().any(|i| i.meaning == "Fa"), "{seen:?}");
    assert_eq!(seen.first().map(|i| i.pos.as_str()), Some("adjective"));
    let sent = &server.seen()[0];
    assert_eq!(sent.body["model"], "m1");
    assert_eq!(sent.body["stream"], true);
    assert_eq!(sent.body["response_format"]["type"], "json_schema");
    assert_eq!(sent.auth, None, "keyless sends no key");
}

#[tokio::test]
async fn a_refused_schema_falls_back_to_a_json_object() {
    let refusal = r#"{"error":{"message":"response_format json_schema is not supported"}}"#;
    let answers = vec![Answer::json(400, refusal), Answer::stream(&[QUICK])];
    let server = Server::start(answers).await;
    let compat = Compat::new(keyless(&server.url).await, "m1");
    let info = run(&compat, &FOX, &|_| {}).await.unwrap();
    assert_eq!(info.pos, "adjective");
    let seen = server.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1].body["response_format"]["type"], "json_object");
    let system = seen[1].body["messages"][0]["content"]
        .as_str()
        .unwrap_or_default();
    assert!(system.contains("JSON Schema"), "{system}");
}

#[tokio::test]
async fn an_ignored_schema_is_asked_for_again_plainly() {
    // As Ollama Cloud does: the schema is accepted, then ignored.
    let drift = r#"{"term":"quick","notes":"fast"}"#;
    let answers = vec![Answer::stream(&[drift]), Answer::stream(&[QUICK])];
    let server = Server::start(answers).await;
    let compat = Compat::new(keyless(&server.url).await, "gemma-cloud");
    let info = run(&compat, &FOX, &|_| {}).await.unwrap();
    assert_eq!(info.pos, "adjective");
    let seen = server.seen();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[1].body["response_format"]["type"], "json_object");

    // The next word starts with the mode that worked.
    run(&compat, &FOX, &|_| {}).await.unwrap();
    let seen = server.seen();
    assert_eq!(seen[2].body["response_format"]["type"], "json_object");
}

#[tokio::test]
async fn an_unreadable_answer_names_what_came() {
    let drift = r#"{"term":"quick","notes":"fast"}"#;
    let server = Server::start(vec![Answer::stream(&[drift])]).await;
    let compat = Compat::new(keyless(&server.url).await, "m");
    let failed = run(&compat, &FOX, &|_| {}).await.unwrap_err();
    assert_eq!(failed.why, Why::BadAnswer);
    let detail = failed.detail.unwrap_or_default();
    assert!(
        detail.contains("notes") && detail.contains("term"),
        "{detail}"
    );
    assert_eq!(
        server.seen().len(),
        3,
        "schema, JSON mode, then a plain prompt"
    );
}

#[tokio::test]
async fn a_blank_model_takes_the_server_s_first() {
    let models = r#"{"data":[{"id":"llama3.2"},{"id":"qwen3"}]}"#;
    let answers = vec![Answer::json(200, models), Answer::stream(&[QUICK])];
    let server = Server::start(answers).await;
    let compat = Compat::new(keyless(&server.url).await, "  ");
    run(&compat, &FOX, &|_| {}).await.unwrap();
    let seen = server.seen();
    assert!(
        seen[0].line.starts_with("GET /v1/models"),
        "{}",
        seen[0].line
    );
    assert_eq!(seen[1].body["model"], "llama3.2");
}

#[tokio::test]
async fn failures_say_why() {
    let gone = r#"{"error":{"message":"model `nope` not found"}}"#;
    let server = Server::start(vec![Answer::json(404, gone)]).await;
    let compat = Compat::new(keyless(&server.url).await, "nope");
    let failed = run(&compat, &FOX, &|_| {}).await.unwrap_err();
    assert_eq!(failed.why, Why::Setup);
    assert_eq!(failed.detail.as_deref(), Some("model `nope` not found"));

    let prose = Server::start(vec![Answer::stream(&["I can't help with that."])]).await;
    let compat = Compat::new(keyless(&prose.url).await, "m");
    let failed = run(&compat, &FOX, &|_| {}).await.unwrap_err();
    assert_eq!(failed.why, Why::BadAnswer);

    let keyed = AiClient::builder()
        .default_url(&server.url)
        .build()
        .await
        .unwrap();
    let compat = Compat::new(Arc::new(keyed), "m");
    let failed = run(&compat, &FOX, &|_| {}).await.unwrap_err();
    assert_eq!(failed.why, Why::NoKey);
}
