//! Adversarial M3-a command regressions against a local API.
use canvas_core::{
    identity::{IdentityDocument, Paths},
    store::OpenIdentity,
};
use serde_json::{Value, json};
use std::process::Command;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::path};

struct Fixture {
    dir: tempfile::TempDir,
    doc: IdentityDocument,
}
impl Fixture {
    fn new(origin: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let doc = IdentityDocument::new(origin, 123, "2026-01-01T00:00:00Z");
        let paths = Paths::for_identity(dir.path().join("data"), &doc.key);
        doc.write(&paths.identity_json()).unwrap();
        OpenIdentity::open(&paths, &doc).unwrap();
        Self { dir, doc }
    }
    fn command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_canvas"));
        cmd.env("CANVAS_DATA_ROOT", self.dir.path().join("data"))
            .env("CANVAS_IDENTITY_KEY", self.doc.key.as_str())
            .env("XDG_CONFIG_HOME", self.dir.path().join("config"))
            .env("CANVAS_NOW", "2026-09-09T17:05:12Z")
            .env("CANVAS_TOKEN", "review-test-token")
            .env_remove("CANVAS_HOST")
            .env_remove("CANVAS_PROFILE");
        cmd
    }
    async fn run(&self, args: &[&str], exit: i32) -> Value {
        let mut cmd = self.command();
        cmd.args(args).args(["--json", "--color", "never"]);
        let output = tokio::task::spawn_blocking(move || cmd.output().unwrap())
            .await
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(exit),
            "{} {}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let value: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(value["exit"], exit);
        value
    }
}

#[tokio::test]
async fn folder_only_denials_are_partial_and_cached_independently() {
    for status in [403, 404] {
        let server = MockServer::start().await;
        let f = Fixture::new(&server.uri());
        Mock::given(path("/api/v1/users/self"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":123})))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/api/v1/courses/5/folders"))
            .respond_with(ResponseTemplate::new(status))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/api/v1/courses/5/files"))
            .respond_with(ResponseTemplate::new(200).set_body_json(
                json!([{"id":50,"display_name":"notes.pdf","hidden":true,"locked_for_user":false}]),
            ))
            .expect(1)
            .mount(&server)
            .await;
        Mock::given(path("/api/v1/courses/5/modules"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .expect(1)
            .mount(&server)
            .await;
        for args in [
            vec!["files", "5"],
            vec!["files", "5"],
            vec!["files", "5", "--offline"],
        ] {
            let v = f.run(&args, 12).await;
            assert_eq!(v["result"]["listing"]["available"], true);
            assert_eq!(v["partial"].as_array().unwrap().len(), 1);
            assert_eq!(v["partial"][0]["scope"], "folders:course:5");
            assert_eq!(v["partial"][0]["http_status"], status);
            assert_eq!(v["result"]["files"][0]["hidden"], true);
            assert_eq!(v["result"]["files"][0]["locked"], false);
        }
    }
}

#[tokio::test]
async fn actual_files_401_aborts_with_auth_and_request_telemetry() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    Mock::given(path("/api/v1/users/self"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":123})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/courses/5/folders"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/courses/5/files"))
        .respond_with(ResponseTemplate::new(401))
        .expect(1)
        .mount(&server)
        .await;
    let v = f.run(&["files", "5"], 3).await;
    assert_eq!(v["schema"], "canvas-cli/error@1");
    assert_eq!(v["result"]["http_status"], 401);
    assert_eq!(v["requests"]["api"], 3);
}

#[tokio::test]
async fn files_sort_numeric_ids_preserve_unknown_access_and_render_nested_tree() {
    let server = MockServer::start().await;
    let f = Fixture::new(&server.uri());
    Mock::given(path("/api/v1/users/self"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id":123})))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/courses/5/folders")).respond_with(ResponseTemplate::new(200).set_body_json(json!([
        {"id":1,"name":"course files"}, {"id":2,"name":"Slides","parent_folder_id":1}, {"id":3,"name":"Week 1","parent_folder_id":2}
    ]))).mount(&server).await;
    Mock::given(path("/api/v1/courses/5/files"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {"id":10,"display_name":"notes.pdf","folder_id":3,"locked":true,"locked_for_user":null},
            {"id":2,"display_name":"notes.pdf","folder_id":3,"hidden":true,"locked_for_user":false}
        ])))
        .mount(&server)
        .await;
    Mock::given(path("/api/v1/courses/5/modules"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;
    let v = f.run(&["files", "5"], 0).await;
    assert_eq!(v["result"]["files"][0]["id"], "2");
    assert_eq!(v["result"]["files"][1]["id"], "10");
    assert_eq!(v["result"]["files"][1]["locked"], Value::Null);
    let mut command = f.command();
    command.args(["files", "5", "--offline", "--tree", "--color", "never"]);
    let output = tokio::task::spawn_blocking(move || command.output().unwrap())
        .await
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "Slides/\n  Week 1/\n    notes.pdf\n    notes.pdf\n"
    );
}
