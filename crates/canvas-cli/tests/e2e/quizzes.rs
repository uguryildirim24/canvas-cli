//! Quiz taking against the fixture server (SPEC §12.7).
//!
//! The listing and the detail name quiz 101 in course 100; the session
//! fixtures carry the live session (quiz submission 501, attempt 1) with its
//! four censored questions. Every write runs with `--yes`, the way the other
//! end-to-end writes do, and the journal id is masked before snapshotting.

use serde_json::json;

use crate::harness::{COURSE_ID, CanvasServer, E2e};

const QUIZ_ID: &str = "101";

#[tokio::test]
async fn quizzes_lists() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let run = env.run(&["quizzes", &COURSE_ID.to_string(), "--json"]);
    run.assert_code(0);
    let value = run.json();
    assert_eq!(value["schema"], "canvas-cli/quizzes@1");
    assert_eq!(value["result"]["course_id"], "100");
    assert_eq!(value["result"]["quizzes"].as_array().unwrap().len(), 1);
    env.snapshot_json("quizzes_json", &run);
}

#[tokio::test]
async fn quizzes_table() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let run = env.run(&["quizzes", &COURSE_ID.to_string()]);
    run.assert_code(0);
    env.snapshot("quizzes_table", &run);
}

#[tokio::test]
async fn quiz_show() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let run = env.run(&["quiz", &COURSE_ID.to_string(), QUIZ_ID, "--json"]);
    run.assert_code(0);
    let value = run.json();
    assert_eq!(value["schema"], "canvas-cli/quiz@1");
    assert_eq!(value["result"]["quiz"]["id"], "101");
    env.snapshot_json("quiz_json", &run);
}

#[tokio::test]
async fn quiz_show_table() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let run = env.run(&["quiz", &COURSE_ID.to_string(), "Week 3 Reading Quiz"]);
    run.assert_code(0);
    env.snapshot("quiz_table", &run);
}

#[tokio::test]
async fn quiz_questions_joins_the_live_session() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let run = env.run(&[
        "quiz",
        "questions",
        &COURSE_ID.to_string(),
        QUIZ_ID,
        "--json",
    ]);
    run.assert_code(0);
    let value = run.json();
    assert_eq!(value["schema"], "canvas-cli/quiz_questions@1");
    assert_eq!(value["result"]["attempt"], 1);
    assert_eq!(value["result"]["questions"].as_array().unwrap().len(), 4);
    env.snapshot_json("quiz_questions_json", &run);
}

#[tokio::test]
async fn quiz_questions_table() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let run = env.run(&["quiz", "questions", &COURSE_ID.to_string(), QUIZ_ID]);
    run.assert_code(0);
    env.snapshot("quiz_questions_table", &run);
}

#[tokio::test]
async fn quiz_questions_starts_a_session_with_yes() {
    let server = CanvasServer::start().await;
    // No session in progress: the submission route answers empty.
    server
        .override_get(
            &format!("/api/v1/courses/{COURSE_ID}/quizzes/{QUIZ_ID}/submission"),
            200,
            json!({ "quiz_submissions": [] }),
        )
        .await;
    let env = E2e::with_server(&server);
    let run = env.run(&[
        "quiz",
        "questions",
        &COURSE_ID.to_string(),
        QUIZ_ID,
        "--yes",
        "--json",
    ]);
    run.assert_code(0);
    let value = run.json();
    assert_eq!(value["result"]["started"], true);
    assert_eq!(value["result"]["attempt"], 1);
}

#[tokio::test]
async fn quiz_questions_refuses_to_start_without_yes() {
    let server = CanvasServer::start().await;
    server
        .override_get(
            &format!("/api/v1/courses/{COURSE_ID}/quizzes/{QUIZ_ID}/submission"),
            200,
            json!({ "quiz_submissions": [] }),
        )
        .await;
    let env = E2e::with_server(&server);
    // No TTY here, so the start confirmation cannot be answered.
    let run = env.run(&[
        "quiz",
        "questions",
        &COURSE_ID.to_string(),
        QUIZ_ID,
        "--json",
    ]);
    run.assert_code(2);
    assert_eq!(run.json()["result"]["code"], "usage");
}

#[tokio::test]
async fn quiz_submit_answers_and_completes() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let answers = env.write_file(
        "answers.json",
        br#"[{"id":901,"answer":4811},{"id":902,"answer":"A wet flask dilutes the titrant."},{"id":903,"answer":4901},{"id":904,"answer":[5101,5102]}]"#,
    );
    let run = env.run(&[
        "quiz",
        "submit",
        &COURSE_ID.to_string(),
        QUIZ_ID,
        "--answers",
        answers.to_str().unwrap(),
        "--yes",
        "--json",
    ]);
    run.assert_code(0);
    let value = run.json();
    assert_eq!(value["schema"], "canvas-cli/operation@1");
    assert_eq!(value["result"]["kind"], "quiz_submit");
    assert_eq!(value["result"]["state"], "posted");
    assert_eq!(value["result"]["response"]["id"], "501");
    env.mask_ids_from_journals();
    env.snapshot_json("quiz_submit_json", &run);
}

#[tokio::test]
async fn quiz_submit_table() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let answers = env.write_file(
        "answers.json",
        br#"[{"id":901,"answer":4811},{"id":902,"answer":"A wet flask dilutes the titrant."},{"id":903,"answer":4901},{"id":904,"answer":[5101,5102]}]"#,
    );
    let run = env.run(&[
        "quiz",
        "submit",
        &COURSE_ID.to_string(),
        QUIZ_ID,
        "--answers",
        answers.to_str().unwrap(),
        "--yes",
    ]);
    run.assert_code(0);
    env.mask_ids_from_journals();
    env.snapshot("quiz_submit_table", &run);
}

#[tokio::test]
async fn quiz_submit_locked_quiz_is_exit_8() {
    let server = CanvasServer::start().await;
    let mut quiz = crate::harness::Fixtures::quiz();
    quiz["locked_for_user"] = json!(true);
    quiz["lock_explanation"] = json!("the quiz closed at midnight");
    server
        .override_get(
            &format!("/api/v1/courses/{COURSE_ID}/quizzes/{QUIZ_ID}"),
            200,
            quiz,
        )
        .await;
    let env = E2e::with_server(&server);
    let answers = env.write_file("answers.json", b"[]");
    let run = env.run(&[
        "quiz",
        "submit",
        &COURSE_ID.to_string(),
        QUIZ_ID,
        "--answers",
        answers.to_str().unwrap(),
        "--yes",
        "--json",
    ]);
    run.assert_code(8);
    let value = run.json();
    assert_eq!(value["result"]["details"]["reason"], "locked");
    env.snapshot_json("quiz_submit_locked", &run);
}

#[tokio::test]
async fn quiz_submit_without_a_session_is_exit_8() {
    let server = CanvasServer::start().await;
    server
        .override_get(
            &format!("/api/v1/courses/{COURSE_ID}/quizzes/{QUIZ_ID}/submission"),
            200,
            json!({ "quiz_submissions": [] }),
        )
        .await;
    let env = E2e::with_server(&server);
    let answers = env.write_file("answers.json", br#"[{"id":901,"answer":4811}]"#);
    let run = env.run(&[
        "quiz",
        "submit",
        &COURSE_ID.to_string(),
        QUIZ_ID,
        "--answers",
        answers.to_str().unwrap(),
        "--yes",
        "--json",
    ]);
    run.assert_code(8);
    let value = run.json();
    assert_eq!(value["result"]["details"]["reason"], "no_session");
    env.snapshot_json("quiz_submit_no_session", &run);
}

#[tokio::test]
async fn quiz_submit_unknown_question_is_exit_8() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let answers = env.write_file("answers.json", br#"[{"id":9999,"answer":1}]"#);
    let run = env.run(&[
        "quiz",
        "submit",
        &COURSE_ID.to_string(),
        QUIZ_ID,
        "--answers",
        answers.to_str().unwrap(),
        "--yes",
        "--json",
    ]);
    run.assert_code(8);
    let value = run.json();
    assert_eq!(value["result"]["details"]["reason"], "unresolved");
}

#[tokio::test]
async fn new_quizzes_lists() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let run = env.run(&["new-quizzes", &COURSE_ID.to_string(), "--json"]);
    run.assert_code(0);
    let value = run.json();
    assert_eq!(value["schema"], "canvas-cli/new-quizzes@1");
    assert_eq!(value["result"]["course_id"], "100");
    assert_eq!(value["result"]["quizzes"].as_array().unwrap().len(), 1);
    env.snapshot_json("new_quizzes_json", &run);
}

#[tokio::test]
async fn new_quizzes_table() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let run = env.run(&["new-quizzes", &COURSE_ID.to_string()]);
    run.assert_code(0);
    env.snapshot("new_quizzes_table", &run);
}

#[tokio::test]
async fn new_quiz_show() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let run = env.run(&["new-quiz", &COURSE_ID.to_string(), "9013", "--json"]);
    run.assert_code(0);
    let value = run.json();
    assert_eq!(value["schema"], "canvas-cli/new-quiz@1");
    assert_eq!(value["result"]["quiz"]["id"], "201");
    env.snapshot_json("new_quiz_json", &run);
}

#[tokio::test]
async fn new_quiz_show_table() {
    let server = CanvasServer::start().await;
    let env = E2e::with_server(&server);
    let run = env.run(&["new-quiz", &COURSE_ID.to_string(), "9013"]);
    run.assert_code(0);
    env.snapshot("new_quiz_table", &run);
}
