#[test]
fn only_instruction_context_bypasses_runtime_access_lease() {
    let instructions = serde_json::from_value::<rmcp::model::JsonObject>(serde_json::json!({
        "action": "instructions"
    }))
    .expect("instruction arguments");
    let status = serde_json::from_value::<rmcp::model::JsonObject>(serde_json::json!({
        "action": "status"
    }))
    .expect("status arguments");
    assert!(super::is_read_only_instruction_call(
        "context",
        Some(&instructions)
    ));
    assert!(!super::is_read_only_instruction_call(
        "context",
        Some(&status)
    ));
    assert!(!super::is_read_only_instruction_call(
        "practice",
        Some(&instructions)
    ));
    assert!(!super::is_read_only_instruction_call("context", None));
}

use super::*;
use rmcp::transport::Transport;
use std::sync::Mutex;
use tokio::io::AsyncBufReadExt;

pub(super) static SERIAL: Mutex<()> = Mutex::new(());

#[test]
fn a_frame_too_long_or_unreadable_with_an_id_is_answered_and_garbage_is_not() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        let (mut client, server_in) = tokio::io::duplex(64 * 1024);
        let (server_out, client_out) = tokio::io::duplex(64 * 1024);
        let mut transport = BoundedStdio::with(server_in, server_out);
        let mut replies = tokio::io::BufReader::new(client_out);
        let writer = tokio::spawn(async move {
            let mut oversized = vec![b'x'; MAX_FRAME_BYTES + 1];
            oversized.push(b'\n');
            client.write_all(&oversized).await.unwrap();
            client.write_all(b"not json\n").await.unwrap();
            client
                .write_all(
                    b"{\"jsonrpc\":\"2.0\",\"id\":7,\"method\":\"tools/call\",\"params\":3}\n",
                )
                .await
                .unwrap();
            client
                .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
                .await
                .unwrap();
            client
        });
        let message = transport.receive().await.expect("the valid frame after");
        assert_eq!(
            serde_json::to_value(&message).unwrap()["method"],
            "notifications/initialized"
        );
        let mut lines = Vec::new();
        for _ in 0..2 {
            let mut line = String::new();
            tokio::time::timeout(
                std::time::Duration::from_secs(5),
                replies.read_line(&mut line),
            )
            .await
            .expect("a reply within the deadline")
            .unwrap();
            lines.push(serde_json::from_str::<serde_json::Value>(&line).unwrap());
        }
        assert_eq!(lines[0]["error"]["code"], -32600, "{}", lines[0]);
        assert!(lines[0].get("id").is_none_or(|id| id.is_null()));
        assert_eq!(lines[1]["error"]["code"], -32600, "{}", lines[1]);
        assert_eq!(lines[1]["id"], 7);
        drop(writer.await.unwrap());
        drop(transport);
        let mut rest = String::new();
        replies.read_line(&mut rest).await.unwrap();
        assert_eq!(rest, "", "no reply to bytes that are not JSON");
    });
}

#[test]
fn request_cancellation_bridge_signals_running_work() {
    let _serial = SERIAL.lock().expect("serial worker tests");
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .expect("test runtime");
    runtime.block_on(async {
        // A cancellation signal sets the flag.
        let (sender, receiver) = tokio::sync::oneshot::channel::<()>();
        let (flag, watcher) = bridge_cancellation(async move {
            let _ = receiver.await;
        });
        tokio::task::yield_now().await;
        assert!(
            !flag.load(Ordering::Relaxed),
            "an uncancelled request must not signal work"
        );
        sender.send(()).expect("signal request cancellation");
        let signalled = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while !flag.load(Ordering::Relaxed) {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .is_ok();
        assert!(signalled, "request cancellation must signal running work");
        drop(CancelOnDrop {
            flag: Arc::clone(&flag),
            watcher,
        });

        // A completed request releases a still-parked watcher instead of
        // leaving it alive until process exit. `CancelOnDrop` performs the
        // abort; normal MCP completion never cancels the request token.
        let baseline = tokio::runtime::Handle::current()
            .metrics()
            .num_alive_tasks();
        let (flag, watcher) = bridge_cancellation(std::future::pending());
        let guard = CancelOnDrop {
            flag: Arc::clone(&flag),
            watcher,
        };
        tokio::task::yield_now().await;
        assert!(
            tokio::runtime::Handle::current()
                .metrics()
                .num_alive_tasks()
                > baseline,
            "the watcher must be alive before the guard drops"
        );
        drop(guard);
        let released = tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while tokio::runtime::Handle::current()
                .metrics()
                .num_alive_tasks()
                > baseline
            {
                tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            }
        })
        .await
        .is_ok();
        assert!(
            released,
            "completed requests must release the cancellation watcher"
        );
    });
}
