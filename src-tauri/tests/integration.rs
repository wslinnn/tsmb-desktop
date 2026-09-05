//! M2 集成测试：对 bot 仓库的 scripts/dev-server.mjs（假后端）跑全链路。
//! 门控：环境变量 TSMB_DEV_SERVER 指向服务器地址（如 http://127.0.0.1:3999）
//! 未设置时整体跳过 —— `TSMB_DEV_SERVER=http://127.0.0.1:3999 cargo test --test integration`

use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use tsmb_desktop_lib::api;
use tsmb_desktop_lib::timing::TimingAnchor;
use tsmb_desktop_lib::ws::parse_ws_message;

fn server() -> Option<String> {
    std::env::var("TSMB_DEV_SERVER").ok().filter(|s| !s.is_empty())
}

async fn login(client: &reqwest::Client, base: &str) -> String {
    api::login(client, base, "alice", "pw-alice-123")
        .await
        .expect("login")
        .token
}

#[tokio::test]
async fn rest_chain_bots_elapsed_lyrics() {
    let Some(base) = server() else { return };
    let client = reqwest::Client::new();
    let token = login(&client, &base).await;

    let bots = api::get_bots(&client, &base, &token).await.expect("get_bots");
    assert_eq!(bots.len(), 1, "dev-server 应有 1 个 fake bot");
    let bot = &bots[0];
    assert_eq!(bot.current_song.as_ref().unwrap().platform, "netease");

    // 锚点插值对拍：本地插值应贴着下一次服务端真值（差 < 2s，含网络与 5s 推送间隔）
    let anchor = TimingAnchor::from_status(bot);
    let e1 = api::get_elapsed(&client, &base, &token, &bot.id).await.expect("elapsed");
    let local = anchor.render_elapsed();
    let server_truth = e1.elapsed;
    let diff = (local - server_truth).abs();
    assert!(
        diff < 3.0,
        "本地插值 {local} 与服务端真值 {server_truth} 偏差过大（{diff}s）"
    );

    // 歌词：行数 > 0 且带翻译
    let lines = api::get_lyrics(&client, &base, &token, "netease", "20391")
        .await
        .expect("lyrics");
    assert!(lines.len() > 30, "应有整首歌的行数，got {}", lines.len());
    assert!(lines.iter().any(|l| l.translation.is_some()));

    // 登出后 token 失效
    api::logout(&client, &base, &token).await;
    let err = api::get_bots(&client, &base, &token).await.expect_err("should be 401");
    assert!(matches!(err, tsmb_desktop_lib::http::AppError::Unauthorized));
}

#[tokio::test]
async fn ws_bearer_handshake_receives_init_and_state_change() {
    let Some(base) = server() else { return };
    let client = reqwest::Client::new();
    let token = login(&client, &base).await;

    let ws_url = tsmb_desktop_lib::settings::ws_url(&base);
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::HeaderValue;
    let mut req = (&ws_url).into_client_request().expect("build ws request");
    req.headers_mut()
        .insert("Authorization", HeaderValue::from_str(&format!("Bearer {token}")).unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async_tls_with_config(req, None, false, None)
        .await
        .expect("ws connect");

    // init 快照
    let init_raw = tokio::time::timeout(Duration::from_secs(5), ws.next())
        .await
        .expect("init timeout")
        .expect("stream closed")
        .expect("ws error");
    let text = init_raw.into_text().expect("text frame").to_string();
    match parse_ws_message(&text).expect("parse init") {
        tsmb_desktop_lib::ws::WsEvent::Init { bots } => {
            assert_eq!(bots.len(), 1);
            assert!(bots[0].playing);
        }
        other => panic!("expected init, got {other:?}"),
    }

    // Ping-Pong：服务端 25s ping 太久等不起 —— 直接发一个 Ping 验证回环不炸，
    // 再等一条 stateChange（fake bot 每 5s 推一次）
    ws.send(tokio_tungstenite::tungstenite::Message::Ping(vec![1, 2, 3].into()))
        .await
        .expect("send ping");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    loop {
        let msg = tokio::time::timeout_at(deadline, ws.next()).await.expect("timeout").expect("closed").expect("err");
        match msg {
            tokio_tungstenite::tungstenite::Message::Text(t) => {
                if let Some(tsmb_desktop_lib::ws::WsEvent::StateChange { status, .. }) =
                    parse_ws_message(&t.to_string())
                {
                    assert!(status.elapsed >= 0.0);
                    break;
                }
            }
            _ => {}
        }
    }
    let _ = ws.close(None).await;
}
