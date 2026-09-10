//! End-to-end `canvas bridge` tests against a real broker process.
//!
//! The extension side is spoken here by hand — native-messaging framing on
//! the child's pipes — because the framing and the message vocabulary are the
//! contract Chrome sees. The consumer side is the shipped CLI, so what these
//! tests exercise is exactly what a person runs.
//!
//! Every message that crosses either pipe is recorded, so one test can assert
//! that no secret, cookie file, or token appears anywhere on the wire.

#![cfg(unix)]

mod support;

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use support::{
    EXTENSION, Fixture, ORIGIN, OTHER_EXTENSION, PAGE_SECRET, TOKEN, exists, observation, wait_for,
};

// -------------------------------------------------------------------- tests

/// M7-a acceptance: only the configured extension, from a real extension
/// origin, may start the broker.
#[test]
fn a_wrong_extension_id_or_a_wrong_origin_is_refused() {
    let f = Fixture::new();

    let wrong = f.host(OTHER_EXTENSION).stop();
    assert_eq!(wrong, Some(8), "another extension started the host");

    // A caller that is not an extension origin at all.
    let output = f
        .command()
        .args(["bridge", "host", "https://evil.test/"])
        .output()
        .expect("run");
    assert_eq!(output.status.code(), Some(8));

    // And no argument at all: Chrome always supplies one.
    let output = f.command().args(["bridge", "host"]).output().expect("run");
    assert_eq!(output.status.code(), Some(8));

    // Nothing above left an endpoint behind.
    assert!(!exists(&f.endpoint()), "a refused host bound the endpoint");
}

/// The whole path: the person attaches a tab, the CLI reads it, and a second
/// consumer sees nothing until it opts in.
#[test]
fn an_attached_tab_is_served_to_the_cli_and_only_to_an_opted_in_consumer() {
    let f = Fixture::new();
    let mut host = f.host(EXTENSION);
    let ready = host.recv_type("ready");
    assert_eq!(ready["protocol"], "bridge-native@1");
    assert_eq!(ready["identity_key"], f.doc.key.as_str());

    host.hello(EXTENSION);
    host.attach(&observation());
    assert_eq!(host.recv_type("attached")["state"], "attached");
    wait_for("the endpoint", || exists(&f.endpoint()));

    // `bridge status` names the attachment without naming the page.
    let (status, code) = f.json(&["bridge", "status"]);
    assert_eq!(code, 0, "{status}");
    assert_eq!(status["schema"], "canvas-cli/bridge@1");
    assert_eq!(status["result"]["owner"]["live"], true);
    let attachment = &status["result"]["attachments"][0];
    assert_eq!(attachment["state"], "attached");
    assert_eq!(attachment["origin"], ORIGIN);
    assert!(
        !status.to_string().contains("Essay 1"),
        "the listing named the page: {status}"
    );

    // The CLI is the person: it reads the sole attachment.
    let (here, code) = f.json(&["--offline", "here"]);
    assert_eq!(code, 0, "{here}");
    assert_eq!(here["schema"], "canvas-cli/here@1");
    assert_eq!(here["result"]["state"], "attached");
    assert_eq!(here["result"]["consumer"], Value::Null);
    let browser = &here["result"]["browser"];
    assert_eq!(browser["zone"], "open");
    assert_eq!(browser["course_id"], "45679");
    assert_eq!(browser["assignment_id"], "98765");
    assert_eq!(browser["title"], "Essay 1");
    assert_eq!(browser["account"]["user_id"], "123");
    // Browser context is an observation, never a cached dataset.
    assert_eq!(browser["ttl_ms"], 0);
    assert_eq!(here["freshness"], json!([]));
    // Metadata alone never asks the page for text.
    assert_eq!(browser["text"], Value::Null);
    assert_eq!(browser["selection"], Value::Null);

    host.stop();
}

/// M7-a acceptance: no secret, cookie file, or token appears on either pipe.
///
/// The secrets are planted, not merely absent. The companion is made to
/// report a URL carrying a capability parameter, so the assertion has
/// something to catch, and the host's own half of the wire is checked apart
/// from the extension's, because only the host's half is this package's to
/// promise.
#[test]
fn nothing_on_the_wire_carries_a_secret() {
    let f = Fixture::new();
    let mut host = f.host(EXTENSION);
    host.recv_type("ready");
    host.hello(EXTENSION);

    // A page whose URL carries capabilities a careless companion would send.
    let mut page = observation();
    page["url"] = json!(format!(
        "{ORIGIN}/courses/45679/files/9/download?verifier={PAGE_SECRET}\
         &X-Amz-Signature=deadbeef&wrap=1"
    ));
    host.attach(&page);
    host.recv_type("attached");
    wait_for("the endpoint", || exists(&f.endpoint()));
    let (here, code) = f.json(&["--offline", "here"]);
    assert_eq!(code, 0, "{here}");
    f.json(&["bridge", "status"]);

    // The host stripped them before it stored the URL, so nothing downstream
    // of the broker carries them either.
    let bundle = here.to_string();
    for forbidden in [PAGE_SECRET, "verifier", "X-Amz-Signature", "deadbeef"] {
        assert!(
            !bundle.contains(forbidden),
            "{forbidden} reached the bundle:\n{bundle}"
        );
    }
    assert!(
        here["result"]["browser"]["url"]
            .as_str()
            .expect("a sanitized url")
            .contains("wrap=1"),
        "sanitizing the URL threw the route away: {here}"
    );

    // The whole wire, and the host's half of it on its own.
    let wire = host.wire();
    let answered = host.answered();
    for forbidden in [
        TOKEN,
        "Cookie",
        "cookie",
        "authenticity_token",
        "Authorization",
        "Bearer",
        "cookies.sqlite",
        "Cookies",
    ] {
        assert!(
            !wire.contains(forbidden),
            "{forbidden} crossed the pipe:\n{wire}"
        );
    }
    for forbidden in [PAGE_SECRET, "verifier", "X-Amz-Signature", "deadbeef"] {
        assert!(
            !answered.contains(forbidden),
            "the host echoed {forbidden}:\n{answered}"
        );
    }
    host.stop();
}

/// M7-a acceptance: a stale socket is cleaned only by an owner, and a live
/// endpoint is never unlinked.
#[test]
fn a_restarted_broker_clears_only_a_stale_endpoint() {
    let f = Fixture::new();

    // A stale socket from a host that died without cleaning up.
    let bridge = f.data_root().join("bridge");
    std::fs::create_dir_all(&bridge).expect("dir");
    std::fs::write(f.endpoint(), b"").expect("stale socket");

    let mut host = f.host(EXTENSION);
    host.recv_type("ready");
    host.hello(EXTENSION);
    host.attach(&observation());
    host.recv_type("attached");
    wait_for("the endpoint", || exists(&f.endpoint()));
    // The stale file was replaced by a socket that answers.
    let (status, code) = f.json(&["bridge", "status"]);
    assert_eq!(code, 0, "{status}");
    assert_eq!(status["result"]["owner"]["live"], true);

    // A second host for the same identity reports the owner and exits 8. It
    // must not unlink the live endpoint on its way out.
    let second = f.host(EXTENSION).stop();
    assert_eq!(second, Some(8), "a second host took the identity");
    assert!(exists(&f.endpoint()), "the live endpoint was unlinked");
    let (status, code) = f.json(&["bridge", "status"]);
    assert_eq!(code, 0, "{status}");
    assert_eq!(status["result"]["owner"]["live"], true);

    host.stop();
    // The owner cleans up after itself, and the ownership lock stays.
    wait_for("the endpoint to go", || !exists(&f.endpoint()));
    assert!(exists(&f.owner_lock()), "the ownership lock was removed");
}

/// M7-a acceptance: `identity remove` with a live host completes after the
/// cooperative release, and the root identity lock survives.
#[test]
fn identity_remove_releases_a_live_host() {
    let f = Fixture::new();
    let mut host = f.host(EXTENSION);
    host.recv_type("ready");
    host.hello(EXTENSION);
    host.attach(&observation());
    host.recv_type("attached");
    wait_for("the endpoint", || exists(&f.endpoint()));

    let key = f.doc.key.to_string();
    let (removed, code) = f.json(&["identity", "remove", &key, "--yes"]);
    assert_eq!(code, 0, "{removed}");
    assert_eq!(removed["result"]["removed"], true);

    // The host was told to let go, and it did.
    let detach = host.recv_type("detach");
    assert_eq!(detach["reason"], "identity_released");
    assert_eq!(host.stop(), Some(0));

    // The endpoint and the ownership lock of that identity are gone; the
    // broker directory and the root lock directory are not.
    assert!(!exists(&f.endpoint()));
    assert!(!exists(&f.owner_lock()));
    assert!(f.data_root().join("bridge").is_dir());
    assert!(f.data_root().join("locks").is_dir());
}

/// A refusal is an answer: no broker means exit 8 with a named reason.
#[test]
fn an_absent_broker_refuses_with_a_reason() {
    let f = Fixture::new();
    let (here, code) = f.json(&["--offline", "here"]);
    assert_eq!(code, 8, "{here}");
    assert_eq!(here["outcome"], "refused");
    assert_eq!(here["result"]["reason"], "bridge_unavailable");
    assert_eq!(here["result"]["state"], "not_attached");
    assert_eq!(here["result"]["browser"], Value::Null);

    let (detached, code) = f.json(&["bridge", "detach"]);
    assert_eq!(code, 8, "{detached}");
    assert_eq!(detached["result"]["detached"], false);
    assert_eq!(detached["result"]["reason"], "bridge_unavailable");

    // `bridge status` is setup information: it answers without a broker.
    let (status, code) = f.json(&["bridge", "status"]);
    assert_eq!(code, 0, "{status}");
    assert_eq!(status["result"]["owner"]["live"], false);
    assert_eq!(status["result"]["attachments"], json!([]));
}

/// `bridge.pause_hidden_after` is validated, and the host tells the companion
/// what it says: only the extension can see whether the tab is hidden.
#[test]
fn the_hidden_tab_timeout_is_configured_and_sent_to_the_companion() {
    let f = Fixture::new();

    // The default reaches the companion when nothing is configured.
    let mut host = f.host(EXTENSION);
    assert_eq!(host.recv_type("ready")["pause_hidden_after_ms"], 600_000);
    host.stop();

    for good in ["30s", "90s", "600s", "5m", "1h"] {
        f.run(
            &["config", "set", "bridge.pause_hidden_after", good],
            Some(0),
        );
    }
    // A count with no unit, a unit with no count, and zero are not durations.
    for bad in ["10", "soon", "m", "0m", "1d"] {
        let output = f
            .command()
            .args(["config", "set", "bridge.pause_hidden_after", bad])
            .output()
            .expect("run");
        assert_eq!(output.status.code(), Some(2), "`{bad}` was accepted");
    }

    f.run(
        &["config", "set", "bridge.pause_hidden_after", "90s"],
        Some(0),
    );
    let mut host = f.host(EXTENSION);
    assert_eq!(host.recv_type("ready")["pause_hidden_after_ms"], 90_000);
    host.stop();
}

/// `bridge install` writes a private manifest naming this binary, and the
/// human output tells a person what to do next.
#[test]
fn install_writes_the_manifest_and_the_steps() {
    let f = Fixture::new();
    let (installed, code) = f.json(&["bridge", "install", "--extension-id", EXTENSION]);
    assert_eq!(code, 0, "{installed}");
    let result = &installed["result"];
    assert_eq!(result["written"], true);
    assert_eq!(result["host_name"], "com.canvas_cli.bridge");
    assert_eq!(result["extension_id"], EXTENSION);
    let path = PathBuf::from(result["manifest_path"].as_str().expect("a path"));
    let manifest: Value =
        serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
    assert_eq!(manifest["type"], "stdio");
    assert_eq!(
        manifest["allowed_origins"],
        json!([format!("chrome-extension://{EXTENSION}/")])
    );
    assert!(
        Path::new(manifest["path"].as_str().expect("a path")).is_absolute(),
        "{manifest}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "the manifest is not private");
    }

    let human = f.run(&["bridge", "install", "--extension-id", EXTENSION], Some(0));
    assert!(human.contains("Load unpacked"), "{human}");
}

/// M7-a acceptance: two consumers, and only the one that opted in sees the
/// bundle. The resource attaches nobody.
#[test]
fn only_the_consumer_that_attached_reads_the_bundle() {
    let f = Fixture::new();
    let mut host = f.host(EXTENSION);
    host.recv_type("ready");
    host.hello(EXTENSION);
    host.attach(&observation());
    host.recv_type("attached");
    wait_for("the endpoint", || exists(&f.endpoint()));

    let mut alpha = f.mcp("alpha");
    let mut beta = f.mcp("beta");
    let prefix = format!(
        "canvas://{}/{}/context",
        f.doc.key.as_str(),
        f.doc.generation
    );

    // Nobody has opted in, so nobody reads the page.
    let before = alpha.tool("context.here", &json!({}));
    assert_eq!(before["outcome"], "refused", "{before}");
    assert_eq!(before["result"]["reason"], "not_attached");
    assert_eq!(before["result"]["browser"], Value::Null);

    // Reading the resource is not an opt-in either.
    let read = alpha.call(
        "resources/read",
        json!({ "uri": format!("{prefix}/mcp:alpha") }),
    );
    let text = read["result"]["contents"][0]["text"]
        .as_str()
        .expect("text");
    let document: Value = serde_json::from_str(text).expect("json");
    assert_eq!(document["result"]["reason"], "not_attached", "{document}");

    // Alpha opts in. The handle comes back; the page does not.
    let attached = alpha.tool("context.attach", &json!({}));
    assert_eq!(attached["outcome"], "ok", "{attached}");
    assert_eq!(attached["result"]["state"], "attached");
    assert_eq!(attached["result"]["consumer"], "mcp:alpha");
    assert_eq!(attached["result"]["browser"], Value::Null);
    let handle = attached["result"]["attachment"]
        .as_str()
        .expect("an attachment handle")
        .to_owned();

    // Now alpha reads the page, through the tool and through its resource.
    let here = alpha.tool("context.here", &json!({}));
    assert_eq!(here["outcome"], "ok", "{here}");
    assert_eq!(here["result"]["consumer"], "mcp:alpha");
    assert_eq!(here["result"]["browser"]["title"], "Essay 1");
    let read = alpha.call(
        "resources/read",
        json!({ "uri": format!("{prefix}/mcp:alpha") }),
    );
    let text = read["result"]["contents"][0]["text"]
        .as_str()
        .expect("text");
    let document: Value = serde_json::from_str(text).expect("json");
    assert_eq!(document["result"]["browser"]["title"], "Essay 1");
    assert_eq!(read["result"]["ttlMs"], 0);

    // Beta never opted in. Alpha's handle does not serve beta either.
    let refused = beta.tool("context.here", &json!({}));
    assert_eq!(refused["result"]["reason"], "not_attached", "{refused}");
    // Nor does alpha's *resource*: a consumer handle is not a name anyone may
    // read under (REPORT section 3.2).
    let borrowed = beta.call(
        "resources/read",
        json!({ "uri": format!("{prefix}/mcp:alpha") }),
    );
    let text = borrowed["result"]["contents"][0]["text"]
        .as_str()
        .expect("text");
    let document: Value = serde_json::from_str(text).expect("json");
    assert_eq!(
        document["result"]["reason"], "not_attached",
        "beta read alpha's context: {document}"
    );
    assert!(document["result"]["browser"].is_null(), "{document}");
    assert!(
        !text.contains("Essay 1"),
        "beta read alpha's page: {document}"
    );
    let stolen = beta.tool("context.here", &json!({ "attachment_id": handle }));
    assert_eq!(stolen["result"]["reason"], "not_attached", "{stolen}");
    let read = beta.call(
        "resources/read",
        json!({ "uri": format!("{prefix}/mcp:beta") }),
    );
    let text = read["result"]["contents"][0]["text"]
        .as_str()
        .expect("text");
    let document: Value = serde_json::from_str(text).expect("json");
    assert_eq!(document["result"]["reason"], "not_attached", "{document}");

    // Alpha lets go. The tab stays attached for the person.
    let detached = alpha.tool("context.detach", &json!({}));
    assert_eq!(detached["outcome"], "ok", "{detached}");
    assert_eq!(detached["result"]["detached"], true);
    let after = alpha.tool("context.here", &json!({}));
    assert_eq!(after["result"]["reason"], "not_attached", "{after}");
    let (still, code) = f.json(&["--offline", "here"]);
    assert_eq!(code, 0, "{still}");
    assert_eq!(still["result"]["browser"]["title"], "Essay 1");

    alpha.stop();
    beta.stop();
    host.stop();
}
