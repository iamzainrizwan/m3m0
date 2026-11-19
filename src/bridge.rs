//! Bridge to the m3m0 RemNote plugin (plugin/). A plugin can't listen on a
//! port, so m3m0 is the server: it serves the built plugin over HTTP (RemNote's
//! "Develop from localhost" loads it from there) and accepts the plugin's
//! websocket. Ops are JSON {id, op, ...}; replies {id, ok, result|error}.
//! Protocol and findings: docs/bridge.md.

use serde_json::{json, Value};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use tungstenite::{Message, WebSocket};

pub const WS_PORT: u16 = 47830;
pub const HTTP_PORT: u16 = 47831;

pub fn plugin_dist() -> PathBuf {
    std::env::var("M3M0_PLUGIN_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("plugin/dist"))
}

/// Serve plugin/dist on 127.0.0.1:HTTP_PORT with CORS open (RemNote fetches it cross-origin).
pub fn serve_plugin() -> std::io::Result<()> {
    let dir = plugin_dist();
    if !dir.join("manifest.json").exists() {
        return Err(std::io::Error::other(format!("{} has no manifest.json; run `npm run build` in plugin/", dir.display())));
    }
    let server = tiny_http::Server::http(("127.0.0.1", HTTP_PORT)).map_err(std::io::Error::other)?;
    std::thread::spawn(move || {
        for req in server.incoming_requests() {
            let path = req.url().split('?').next().unwrap_or("/").trim_start_matches('/').to_owned();
            let path = if path.is_empty() { "index.html".to_owned() } else { path };
            let file = dir.join(&path);
            let ok = !path.contains("..") && file.is_file();
            let ctype = match file.extension().and_then(|e| e.to_str()) {
                Some("html") => "text/html",
                Some("js") => "application/javascript",
                Some("css") => "text/css",
                Some("json") => "application/json",
                _ => "application/octet-stream",
            };
            let cors = tiny_http::Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap();
            let resp = if ok {
                let body = std::fs::read(&file).unwrap_or_default();
                tiny_http::Response::from_data(body)
                    .with_header(tiny_http::Header::from_bytes("Content-Type", ctype).unwrap())
                    .with_header(cors)
                    .boxed()
            } else {
                tiny_http::Response::from_string("not found").with_status_code(404).with_header(cors).boxed()
            };
            let _ = req.respond(resp);
        }
    });
    Ok(())
}

pub struct Conn {
    ws: WebSocket<TcpStream>,
    next: u64,
}

impl Conn {
    /// Wait for the plugin to connect (it retries every 2 s while RemNote runs).
    pub fn accept(timeout: Duration) -> Result<Conn, String> {
        let l = TcpListener::bind(("127.0.0.1", WS_PORT)).map_err(|e| format!("bind {WS_PORT}: {e}"))?;
        l.set_nonblocking(true).map_err(|e| e.to_string())?;
        let t0 = Instant::now();
        loop {
            match l.accept() {
                Ok((s, _)) => {
                    s.set_nonblocking(false).map_err(|e| e.to_string())?;
                    s.set_read_timeout(Some(Duration::from_secs(20))).ok();
                    let mut ws = tungstenite::accept(s).map_err(|e| e.to_string())?;
                    let hello = read_json(&mut ws)?;
                    if hello.get("hello").and_then(Value::as_str) != Some("m3m0-bridge") {
                        return Err(format!("unexpected hello: {hello}"));
                    }
                    return Ok(Conn { ws, next: 1 });
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    if t0.elapsed() > timeout {
                        return Err("plugin didn't connect (is RemNote open with the m3m0 bridge plugin?)".into());
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
                Err(e) => return Err(e.to_string()),
            }
        }
    }

    pub fn call(&mut self, op: &str, mut args: Value) -> Result<Value, String> {
        let id = self.next;
        self.next += 1;
        args["id"] = json!(id);
        args["op"] = json!(op);
        self.ws.send(Message::text(args.to_string())).map_err(|e| e.to_string())?;
        loop {
            let v = read_json(&mut self.ws)?;
            if v.get("id").and_then(Value::as_u64) == Some(id) {
                return if v["ok"] == json!(true) { Ok(v["result"].clone()) } else { Err(format!("{op}: {}", v["error"])) };
            }
        }
    }
}

fn read_json(ws: &mut WebSocket<TcpStream>) -> Result<Value, String> {
    loop {
        match ws.read().map_err(|e| e.to_string())? {
            Message::Text(t) => return serde_json::from_str(t.as_str()).map_err(|e| e.to_string()),
            Message::Close(_) => return Err("plugin closed the connection".into()),
            _ => {}
        }
    }
}

/// `m3m0 bridge-test`: phase 0 spike. Builds a scratch tree with every card and
/// rich-text kind m3m0 needs, reads it back, reviews one scratch card, then
/// checks the raw DB doc for RemNote's own sync stamps.
/// `m3m0 bridge-test --cleanup ID` deletes the scratch tree.
pub fn test(args: &[String]) -> Result<(), String> {
    serve_plugin().map_err(|e| e.to_string())?;
    println!("serving plugin on http://localhost:{HTTP_PORT}  (RemNote: Settings > Plugins > Build > Develop from localhost)");
    println!("waiting for the plugin on ws://127.0.0.1:{WS_PORT} …");
    let mut c = Conn::accept(Duration::from_secs(600))?;
    println!("connected: {}", c.call("ping", json!({}))?);

    if let Some(i) = args.iter().position(|a| a == "--cleanup") {
        let id = args.get(i + 1).ok_or("--cleanup needs the scratch rem id")?;
        c.call("delete", json!({"rem": id}))?;
        println!("deleted {id}");
        return Ok(());
    }

    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    let root = c.call("create", json!({"parent": null, "text": [format!("m3m0 bridge test {stamp}")]}))?["id"].as_str().unwrap_or_default().to_owned();
    println!("scratch root {root}");
    let mut made = vec![];
    let mut mk = |c: &mut Conn, label: &str, v: Value| -> Result<String, String> {
        let mut v = v;
        v["parent"] = json!(root);
        let id = c.call("create", v)?["id"].as_str().unwrap_or_default().to_owned();
        println!("  {label}: {id}");
        made.push((label.to_owned(), id.clone()));
        Ok(id)
    };
    mk(&mut c, "heading H1", json!({"text": ["Heading"], "heading": "H1"}))?;
    let concept = mk(&mut c, "concept ::", json!({"text": ["Concept"], "back": ["its definition"], "remType": 1}))?;
    mk(&mut c, "descriptor ;;", json!({"text": ["Descriptor"], "back": ["its value"], "remType": 2}))?;
    mk(&mut c, "basic >>", json!({"text": ["Front"], "back": ["Back"], "practice": "forward"}))?;
    mk(&mut c, "both <>", json!({"text": ["Left"], "back": ["Right"], "practice": "both"}))?;
    mk(
        &mut c,
        "rich text",
        json!({"text": [
            "latex ", {"i": "x", "text": "\\frac{a}{b} \\le \\mathsf{E}[X]"},
            " code ", {"i": "m", "text": "inline()", "q": true},
            " bold ", {"i": "m", "text": "strong", "b": true},
            " ref ", {"i": "q", "_id": concept},
        ]}),
    )?;
    mk(&mut c, "cloze (cId)", json!({"text": ["the answer is ", {"i": "m", "text": "forty-two", "cId": format!("m3m0{stamp}")}]}))?;
    let multi = mk(&mut c, "multi-line >>>", json!({"text": ["List three things"], "practice": "forward"}))?;
    for t in ["one", "two", "three"] {
        let id = c.call("create", json!({"parent": multi, "text": [t], "cardItem": true}))?["id"].clone();
        println!("    item {t}: {id}");
    }

    std::thread::sleep(Duration::from_secs(2));
    println!("\nread back:");
    for (label, id) in &made {
        let g = c.call("get", json!({"rem": id}))?;
        let cards: Vec<String> = g["cards"].as_array().into_iter().flatten().map(|c| c["type"].to_string()).collect();
        println!("  {label}: type={} text={} back={} cards=[{}]", g["type"], g["text"], g["back"], cards.join(", "));
    }

    println!("\nreview (GOOD) on the concept card:");
    let g = c.call("get", json!({"rem": concept}))?;
    match g["cards"].get(0).and_then(|c| c["id"].as_str()) {
        Some(card) => {
            let r = c.call("review", json!({"card": card, "score": 1}))?;
            println!("  next={} history={}", r["next"], r["history"]);
        }
        None => println!("  concept rem has no cards yet (RemNote may generate them lazily)"),
    }

    println!("\nraw DB doc for the concept rem (look for <field>,u stamps):");
    let db = crate::kb::resolve_db(None).ok_or("no db")?;
    let kb = crate::kb::Kb::load(&db)?;
    match kb.q.get(&concept) {
        Some(d) => println!("  {d}"),
        None => println!("  not in the snapshot yet (RemNote flushes to SQLite in batches; rerun the read later)"),
    }
    println!("\nnow check your phone for \"m3m0 bridge test {stamp}\". clean up with: m3m0 bridge-test --cleanup {root}");
    Ok(())
}
