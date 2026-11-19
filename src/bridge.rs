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

// --- long-lived bridge for the app ------------------------------------------------

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;

type Reply = Sender<Result<Value, String>>;

/// Handle to the bridge worker: serves the plugin, keeps the plugin's websocket,
/// runs calls one at a time. Cheap to clone.
#[derive(Clone)]
pub struct Bridge {
    tx: Sender<(String, Value, Reply)>,
    pub online: Arc<AtomicBool>,
    pub error: Arc<std::sync::Mutex<Option<String>>>,
}

impl Bridge {
    pub fn start(repaint: impl Fn() + Send + 'static) -> Bridge {
        let (tx, rx) = channel();
        let online = Arc::new(AtomicBool::new(false));
        let error = Arc::new(std::sync::Mutex::new(None));
        // previews never take the ports from the real app (or write anything)
        if std::env::var("M3M0_SHOTS").is_ok() {
            drop(rx);
            return Bridge { tx, online, error };
        }
        if let Err(e) = serve_plugin() {
            *error.lock().unwrap() = Some(format!("plugin server: {e}"));
        }
        let (o, er) = (online.clone(), error.clone());
        std::thread::spawn(move || worker(rx, o, er, repaint));
        Bridge { tx, online, error }
    }

    pub fn is_online(&self) -> bool {
        self.online.load(Ordering::Relaxed)
    }

    /// Blocking call (use from a background thread).
    pub fn call(&self, op: &str, args: Value) -> Result<Value, String> {
        let (rtx, rrx) = channel();
        self.tx.send((op.to_owned(), args, rtx)).map_err(|_| "bridge worker stopped".to_string())?;
        rrx.recv_timeout(Duration::from_secs(60)).map_err(|_| format!("{op}: no reply from RemNote within 60 s"))?
    }

    /// Wait until the plugin is connected, up to `secs`.
    pub fn wait_online(&self, secs: u64) -> bool {
        let t0 = Instant::now();
        while !self.is_online() {
            if t0.elapsed() > Duration::from_secs(secs) {
                return false;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
        true
    }
}

fn handshake(s: TcpStream) -> Result<Conn, String> {
    s.set_nonblocking(false).map_err(|e| e.to_string())?;
    s.set_read_timeout(Some(Duration::from_secs(30))).ok();
    let mut ws = tungstenite::accept(s).map_err(|e| e.to_string())?;
    let hello = read_json(&mut ws)?;
    if hello.get("hello").and_then(Value::as_str) != Some("m3m0-bridge") {
        return Err(format!("unexpected hello: {hello}"));
    }
    Ok(Conn { ws, next: 1 })
}

fn worker(rx: Receiver<(String, Value, Reply)>, online: Arc<AtomicBool>, error: Arc<std::sync::Mutex<Option<String>>>, repaint: impl Fn()) {
    let l = match TcpListener::bind(("127.0.0.1", WS_PORT)) {
        Ok(l) => l,
        Err(e) => {
            *error.lock().unwrap() = Some(format!("port {WS_PORT} busy ({e}); is another m3m0 running?"));
            repaint();
            for (_, _, r) in rx {
                let _ = r.send(Err(format!("bridge unavailable: port {WS_PORT} busy")));
            }
            return;
        }
    };
    l.set_nonblocking(true).ok();
    let mut conn: Option<Conn> = None;
    let mut last_ping = Instant::now();
    loop {
        // newest connection wins (RemNote reloading the plugin reconnects)
        if let Ok((s, _)) = l.accept() {
            if let Ok(c) = handshake(s) {
                conn = Some(c);
                online.store(true, Ordering::Relaxed);
                repaint();
            }
        }
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok((op, args, reply)) => {
                let r = match conn.as_mut() {
                    Some(c) => c.call(&op, args),
                    None => Err("bridge offline: RemNote isn't connected".into()),
                };
                if r.as_ref().is_err_and(|e| !e.contains(&format!("{op}:"))) {
                    // transport error, not an error reported by the plugin
                    conn = None;
                    online.store(false, Ordering::Relaxed);
                    repaint();
                }
                let _ = reply.send(r);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return,
            Err(_) => {}
        }
        if conn.is_some() && last_ping.elapsed() > Duration::from_secs(10) {
            last_ping = Instant::now();
            if conn.as_mut().unwrap().call("ping", json!({})).is_err() {
                conn = None;
                online.store(false, Ordering::Relaxed);
                repaint();
            }
        }
    }
}

/// Is the RemNote desktop app running? (flatpak or native)
pub fn remnote_running() -> bool {
    std::process::Command::new("pgrep")
        .args(["-f", "remnote/remnote|com.remnote.RemNote|/opt/RemNote"])
        .output()
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false)
}

/// Start RemNote (it loads the bridge plugin from our server at startup).
pub fn launch_remnote() -> Result<(), String> {
    let try_run = |cmd: &str, args: &[&str]| {
        std::process::Command::new(cmd)
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .is_ok()
    };
    if try_run("flatpak", &["run", "com.remnote.RemNote"]) || try_run("remnote", &[]) {
        Ok(())
    } else {
        Err("couldn't start RemNote (flatpak com.remnote.RemNote)".into())
    }
}

/// Quit RemNote (flatpak) so it can be relaunched and load the plugin from m3m0.
pub fn stop_remnote() -> bool {
    std::process::Command::new("flatpak").args(["kill", "com.remnote.RemNote"]).status().map(|s| s.success()).unwrap_or(false)
}
