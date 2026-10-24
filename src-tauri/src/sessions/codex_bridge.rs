//! One private app-server per run, with a real Raw TUI attached to the same thread.
use std::{collections::HashMap, os::windows::{io::AsRawHandle, process::CommandExt}, process::{Child, Command, Stdio}, sync::{Arc, Mutex, atomic::{AtomicBool, AtomicU64, Ordering}, mpsc}, time::Duration};
use serde_json::{Value, json};
use chorus_core::{codex_stream::{Stream, Update}};
use crate::{agents::{Launch, codex_socket::Socket}, store::now_ms};
use super::{Job, Run, SessionManager};

pub const TOKEN_VAR: &str = "CHORUS_CODEX_TOKEN";
struct Process(Child);
impl Drop for Process { fn drop(&mut self) { let _=self.0.kill(); let _=self.0.wait(); } }
type Pending = Arc<Mutex<HashMap<String, mpsc::SyncSender<Result<Value,String>>>>>;
pub struct Bridge {
    _process: Mutex<Process>, socket: Arc<Socket>, pending: Pending,
    events: Mutex<Option<mpsc::Receiver<(i64,Value)>>>, sequence: AtomicU64,
    pub connected: Arc<AtomicBool>, pub ready: AtomicBool, pub resume: Option<String>, pub url: String,
    failure: Arc<Mutex<Option<String>>>,
}
impl Drop for Bridge { fn drop(&mut self) { self.socket.close(); } }
impl Bridge {
    pub fn start(launch: &Launch, extra: &[String], env: &std::collections::BTreeMap<String,String>, cwd: &str, resume: Option<&str>, job: &Job, token: &str) -> Result<Arc<Self>,String> {
        let listener=std::net::TcpListener::bind(("127.0.0.1",0)).map_err(|e|e.to_string())?;
        let port=listener.local_addr().map_err(|e|e.to_string())?.port(); drop(listener);
        let url=format!("ws://127.0.0.1:{port}");
        let mut digest=[0u8;32];
        unsafe { windows::Win32::Security::Cryptography::BCryptHash(windows::Win32::Security::Cryptography::BCRYPT_SHA256_ALG_HANDLE,None,token.as_bytes(),&mut digest) }.ok().map_err(|_|"Could not prepare Codex authentication.")?;
        let digest:String=digest.iter().map(|b|format!("{b:02x}")).collect();
        let mut cmd=Command::new(&launch.program);
        cmd.args(&launch.args).args(extra).args(["app-server","--listen",&url,"--ws-auth","capability-token","--ws-token-sha256",&digest,"--enable","default_mode_request_user_input"])
            .current_dir(cwd).env_clear().envs(env).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).creation_flags(0x08000000);
        let process=Process(cmd.spawn().map_err(|_|"Could not start the private Codex server.")?);
        job.assign(windows::Win32::Foundation::HANDLE(process.0.as_raw_handle())).map_err(|_|"Could not contain the Codex server in its run.")?;
        job.adopt_strays(process.0.id());
        let mut socket=None;
        for _ in 0..30 {
            if let Ok(s)=Socket::connect(port,token) { socket=Some(Arc::new(s)); break; }
            std::thread::sleep(Duration::from_millis(100));
        }
        let socket=socket.ok_or("The private Codex server did not accept an authenticated connection.")?;
        let pending:Pending=Arc::new(Mutex::new(HashMap::new()));
        let connected=Arc::new(AtomicBool::new(true));
        let failure=Arc::new(Mutex::new(None));
        let reader_failure=failure.clone();
        let (events_tx,events_rx)=mpsc::sync_channel(256);
        let (reader,pending_reader,connected_reader)=(socket.clone(),pending.clone(),connected.clone());
        std::thread::spawn(move|| {
            loop {
                let value=match reader.receive() { Ok(value)=>value, Err(reason)=> { *reader_failure.lock().expect("connection failure")=Some(reason); break; } };
                if value.get("method").is_none() {
                    if let Some(id)=value.get("id").and_then(Value::as_str)
                        && let Some(reply)=pending_reader.lock().expect("rpc pending").remove(id) {
                        let result=if value.get("error").is_some() { Err("Codex refused the request. Check Raw for details.".into()) } else { Ok(value["result"].clone()) };
                        let _=reply.send(result);
                    }
                } else if chorus_core::codex_stream::relevant(&value) && events_tx.try_send((now_ms(),value)).is_err() {
                    *reader_failure.lock().expect("connection failure")=Some("Codex event consumer stopped or could not keep up.".into());
                    break;
                }
            }
            connected_reader.store(false,Ordering::SeqCst);
            for (_,reply) in pending_reader.lock().expect("rpc pending").drain() { let _=reply.send(Err("Codex controller disconnected; no request was retried.".into())); }
            reader.close();
        });
        let bridge=Self { _process:Mutex::new(process),socket,pending,events:Mutex::new(Some(events_rx)),sequence:AtomicU64::new(0),connected,ready:AtomicBool::new(false),resume:resume.map(str::to_owned),url,failure };
        bridge.rpc("initialize",json!({"clientInfo":{"name":"chorus_v2","version":"0.1.0"},"capabilities":{"experimentalApi":true}}))?;
        bridge.socket.send(&json!({"method":"initialized"}))?;
        Ok(Arc::new(bridge))
    }
    fn rpc(&self, method:&str, params:Value)->Result<Value,String> {
        let id=format!("chorus-{}",self.sequence.fetch_add(1,Ordering::SeqCst));
        let (tx,rx)=mpsc::sync_channel(1); self.pending.lock().expect("rpc pending").insert(id.clone(),tx);
        let result=self.socket.send(&json!({"id":id,"method":method,"params":params})).and_then(|()|rx.recv_timeout(Duration::from_secs(15)).map_err(|_|"Codex request timed out; it was not retried.".to_owned())?);
        self.pending.lock().expect("rpc pending").remove(&id); result
    }
    pub fn bind(self:&Arc<Self>, manager:&Arc<SessionManager>, session:u32, run:&Arc<Run>) {
        let Some(events)=self.events.lock().expect("bridge events").take() else { return };
        let (manager,run,bridge)=(Arc::downgrade(manager),Arc::downgrade(run),Arc::downgrade(self));
        std::thread::spawn(move|| {
            // The TUI creates new threads. Resuming a zero-turn thread fails in Codex 0.163.0.
            // The first user prompt materializes its rollout; then resume subscribes us.
            let (thread, initial) = loop {
                let (Some(manager),Some(run),Some(bridge))=(manager.upgrade(),run.upgrade(),bridge.upgrade()) else { return };
                if run.exit.lock().expect("exit lock").is_some() || !bridge.connected.load(Ordering::SeqCst) { return; }
                if let Ok(loaded) = bridge.rpc("thread/loaded/list",json!({})) {
                    let id = bridge.resume.as_deref().or_else(|| loaded["data"].as_array().filter(|a|a.len()==1).and_then(|a|a[0].as_str()));
                    if let Some(id) = id.filter(|id|chorus_core::agent::is_uuid(id)) {
                        bridge.ready.store(true,Ordering::SeqCst);
                        if let Ok(resumed) = bridge.rpc("thread/resume",json!({"threadId":id})) {
                            let _=crate::store::sessions::set_agent_session_id(&manager.store,session,id);
                            manager.notify_id(session);
                            break (id.to_owned(),chorus_core::codex_stream::hydrate(id,&resumed,bridge.resume.is_none()));
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(500));
            };
            let mut stream = Stream::default();
            let attached_at=now_ms();
            for (received_at,event) in initial.into_iter().map(|e|(0,e)).chain(events) {
                let (Some(manager),Some(run),Some(bridge))=(manager.upgrade(),run.upgrade(),bridge.upgrade()) else { break };
                if run.exit.lock().expect("exit lock").is_some() { break; }
                if chorus_core::codex_stream::changed_root(&thread,&event,received_at,attached_at) {
                    bridge.ready.store(false,Ordering::SeqCst);
                    let _=crate::store::sessions::set_note(&manager.store,session,Some("Another Codex conversation started. Continue in Raw, or restart Chorus's session to reconnect Friendly."));
                    manager.notify_id(session);
                    run.recorder.lock().expect("recorder lock").replies.clear();
                    bridge.socket.close();
                    return;
                }
                let updates=stream.accept(&thread, &event, now_ms());
                if updates.is_empty() { continue; }
                let mut r=run.recorder.lock().expect("recorder lock");
                for update in updates {
                    match update {
                        Update::Event(event_normalized) => r.record(&manager.store,"codex-server",&event,&[event_normalized],now_ms()),
                        Update::Progress { turn,key,content } => r.progress(&manager.store,Some(&turn),&key,content,now_ms()),
                        Update::Prompt { turn,text } => r.server_prompt(&manager.store,&turn,&text,now_ms()),
                        Update::Resolved(id) => r.resolved_request(&manager.store,&id),
                        Update::Request { id,request,observed } => {
                            r.record(&manager.store,"codex-server",&event,&[observed],now_ms());
                            if let Some(receiver)=r.hold_request(&manager.store,request) {
                                let socket=bridge.socket.clone();
                                tauri::async_runtime::spawn(async move {
                                    if let Ok(Ok(body))=tokio::time::timeout(Duration::from_secs(chorus_core::replies::REPLY_WAIT_SECS),receiver).await
                                        && body!=json!({}) { let _=socket.send(&json!({"id":id,"result":body})); }
                                });
                            }
                        },
                        Update::Unsupported => {
                            let _=crate::store::sessions::set_note(&manager.store,session,Some("Codex requested an interaction Friendly does not support. Open Raw to respond."));
                        }
                    }
                }
                drop(r); manager.notify_id(session);
            }
            if let (Some(manager),Some(run))=(manager.upgrade(),run.upgrade())
                && run.exit.lock().expect("exit lock").is_none() {
                run.recorder.lock().expect("recorder lock").replies.clear();
                let reason=bridge.upgrade().and_then(|b|b.failure.lock().expect("connection failure").clone()).unwrap_or_else(||"Codex Friendly control disconnected.".into());
                let _=crate::store::sessions::set_note(&manager.store,session,Some(&format!("{reason} Continue in Raw, or restart to reconnect. No reply was replayed.")));
                manager.notify_id(session);
            }
        });
    }
}
