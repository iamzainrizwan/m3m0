//! Claude chat panel (ctrl-a): ask questions about the module.
//!
//! Shells out to `claude -p` (Zain's plan, no API key) in the module's uni-git
//! folder with stream-json output, resuming the same session for follow-ups.
//! Context: the open page's notes (deck syntax) go in the first message; the
//! module folder (slides, PDFs, decks) is the working directory. Tools: read
//! anything in the module; edit only Claude's own material under revision/.
//! Runs with --setting-sources "" so Zain's user hooks (Discord, screensaver)
//! don't fire for every reply.

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Receiver};

pub enum Ev {
    Text(String), // streamed delta
    Done { session: Option<String>, error: Option<String> },
    Tool(String), // a tool being used, for the status line
}

pub struct Msg {
    pub user: bool,
    pub text: String,
}

#[derive(Default)]
pub struct Chat {
    pub msgs: Vec<Msg>,
    pub input: String,
    pub session: Option<String>,
    pub module: Option<String>,
    pub rx: Option<Receiver<Ev>>,
    pub status: Option<String>,
    pub scroll_end: bool,
    pub focus_input: bool,
}

impl Chat {
    pub fn busy(&self) -> bool {
        self.rx.is_some()
    }

    /// Start a reply. `context` (the page's notes) is prepended on the first turn.
    pub fn send(&mut self, module: &str, context: &str, repaint: impl Fn() + Send + 'static) {
        let q = self.input.trim().to_owned();
        if q.is_empty() || self.busy() {
            return;
        }
        if self.module.as_deref() != Some(module) {
            // new module: new conversation
            self.session = None;
            self.module = Some(module.to_owned());
        }
        self.input.clear();
        self.msgs.push(Msg { user: true, text: q.clone() });
        self.msgs.push(Msg { user: false, text: String::new() });
        self.scroll_end = true;
        let dir: PathBuf = crate::pdf::uni_dir().join(module.to_lowercase());
        let prompt = if self.session.is_none() {
            format!("Notes on the page I'm looking at (RemNote deck syntax):\n\n{context}\n\n---\n\n{q}")
        } else {
            q
        };
        let system = format!(
            "You're answering Zain's study questions inside m3m0, his RemNote client, for the module in the current directory ({module}). \
             Slides, PDFs and Claude-made flashcard decks are in this folder (weeks/, sources/, revision/). Be concise and direct; \
             write maths as inline LaTeX between single dollar signs. You may edit only files under revision/ (Claude's own decks), \
             and only when asked; never touch anything else."
        );
        let mut cmd = Command::new("claude");
        cmd.current_dir(&dir)
            .args(["-p", "--output-format", "stream-json", "--verbose", "--include-partial-messages"])
            .args(["--setting-sources", ""])
            .args(["--append-system-prompt", &system])
            .args(["--allowedTools", "Read,Grep,Glob,Edit(revision/**),Write(revision/**)"])
            .args(["--disallowedTools", "Bash,WebFetch,WebSearch"])
            .env_remove("CLAUDECODE")
            // prompt on stdin: the tool flags are variadic and would swallow it
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(s) = &self.session {
            cmd.args(["--resume", s]);
        }
        let (tx, rx) = channel();
        self.rx = Some(rx);
        self.status = Some("thinking…".into());
        std::thread::spawn(move || {
            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    let _ = tx.send(Ev::Done { session: None, error: Some(format!("couldn't run claude: {e}")) });
                    repaint();
                    return;
                }
            };
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                let _ = stdin.write_all(prompt.as_bytes());
            }
            let out = child.stdout.take().unwrap();
            let mut session = None;
            let mut streamed = false;
            let mut final_text: Option<String> = None;
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(&line) else { continue };
                if let Some(s) = v.get("session_id").and_then(|s| s.as_str()) {
                    session = Some(s.to_owned());
                }
                match v.get("type").and_then(|t| t.as_str()) {
                    Some("stream_event") => {
                        let ev = &v["event"];
                        if ev["type"] == "content_block_delta" && ev["delta"]["type"] == "text_delta" {
                            if let Some(t) = ev["delta"]["text"].as_str() {
                                streamed = true;
                                let _ = tx.send(Ev::Text(t.to_owned()));
                            }
                        } else if ev["type"] == "content_block_start" && ev["content_block"]["type"] == "tool_use" {
                            let name = ev["content_block"]["name"].as_str().unwrap_or("tool").to_owned();
                            let _ = tx.send(Ev::Tool(name));
                        }
                    }
                    Some("assistant") if !streamed => {
                        for c in v["message"]["content"].as_array().into_iter().flatten() {
                            if c["type"] == "text" {
                                let _ = tx.send(Ev::Text(c["text"].as_str().unwrap_or("").to_owned()));
                            }
                        }
                    }
                    Some("result") => {
                        final_text = v["result"].as_str().map(str::to_owned);
                        if v["is_error"] == true {
                            let _ = tx.send(Ev::Done { session: session.clone(), error: Some(final_text.clone().unwrap_or_else(|| "error".into())) });
                            repaint();
                            return;
                        }
                    }
                    _ => {}
                }
                repaint();
            }
            let status = child.wait();
            let mut err = None;
            if final_text.is_none() && !streamed {
                let mut e = String::new();
                if let Some(mut s) = child.stderr.take() {
                    use std::io::Read;
                    let _ = s.read_to_string(&mut e);
                }
                err = Some(if e.trim().is_empty() { format!("claude exited ({status:?}) with no answer") } else { e.trim().chars().take(400).collect() });
            }
            let _ = tx.send(Ev::Done { session, error: err });
            repaint();
        });
    }

    pub fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        let mut done = false;
        while let Ok(ev) = rx.try_recv() {
            match ev {
                Ev::Text(t) => {
                    if let Some(m) = self.msgs.last_mut() {
                        m.text.push_str(&t);
                    }
                    self.status = Some("writing…".into());
                    self.scroll_end = true;
                }
                Ev::Tool(name) => self.status = Some(format!("using {name}…")),
                Ev::Done { session, error } => {
                    if session.is_some() {
                        self.session = session;
                    }
                    if let Some(e) = error {
                        if let Some(m) = self.msgs.last_mut() {
                            if m.text.is_empty() {
                                m.text = format!("(error) {e}");
                            }
                        }
                    }
                    done = true;
                }
            }
        }
        if done {
            self.rx = None;
            self.status = None;
            self.scroll_end = true;
        }
    }

    pub fn reset(&mut self) {
        if !self.busy() {
            self.msgs.clear();
            self.session = None;
        }
    }
}
