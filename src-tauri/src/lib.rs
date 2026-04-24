use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    thread,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, RunEvent, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
enum AppError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Sql(#[from] rusqlite::Error),
    #[error(transparent)]
    Pty(#[from] anyhow::Error),
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

type AppResult<T> = Result<T, AppError>;

#[derive(Default)]
struct ProcessState {
    processes: Mutex<HashMap<String, ManagedProcess>>,
    completed_logs: Mutex<HashMap<String, String>>,
    ptys: Mutex<HashMap<String, PtySession>>,
}

struct ManagedProcess {
    child: Child,
    log: Arc<Mutex<String>>,
}

struct PtySession {
    child: Mutex<Box<dyn portable_pty::Child + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    output: Arc<Mutex<String>>,
}

#[derive(Debug, Clone, Serialize)]
struct SshHost {
    alias: String,
    hostname: Option<String>,
    user: Option<String>,
    port: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
struct RunningProcess {
    id: String,
    running: bool,
}

#[derive(Debug, Clone, Serialize)]
struct PtySnapshot {
    id: String,
    output: String,
    running: bool,
}

#[derive(Debug, Clone, Serialize)]
struct ActionLog {
    id: String,
    action: String,
    entity_id: Option<String>,
    summary: String,
    created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Task {
    id: String,
    name: String,
    command: String,
    cwd: Option<String>,
    target: String,
    ssh_host: Option<String>,
    env_json: String,
    auto_restart: bool,
    system_autostart: bool,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug, Clone, Deserialize)]
struct TaskInput {
    name: String,
    command: String,
    cwd: Option<String>,
    target: String,
    ssh_host: Option<String>,
    env_json: Option<String>,
    auto_restart: bool,
    system_autostart: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct StartProcessInput {
    id: String,
    command: String,
    cwd: Option<String>,
    target: String,
    ssh_host: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct PtyStartInput {
    shell: Option<String>,
    ssh_host: Option<String>,
    cwd: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
struct ActionLogInput {
    action: String,
    entity_id: Option<String>,
    summary: String,
}

#[tauri::command]
fn list_ssh_hosts() -> AppResult<Vec<SshHost>> {
    parse_ssh_config(&ssh_config_path())
}

#[tauri::command]
fn list_tasks(app: AppHandle) -> AppResult<Vec<Task>> {
    let conn = open_db(&app)?;
    let mut stmt = conn.prepare(
        "select id, name, command, cwd, target, ssh_host, env_json, auto_restart, system_autostart, created_at, updated_at
         from tasks order by updated_at desc",
    )?;
    let tasks = stmt
        .query_map([], |row| {
            Ok(Task {
                id: row.get(0)?,
                name: row.get(1)?,
                command: row.get(2)?,
                cwd: row.get(3)?,
                target: row.get(4)?,
                ssh_host: row.get(5)?,
                env_json: row.get(6)?,
                auto_restart: row.get::<_, i64>(7)? == 1,
                system_autostart: row.get::<_, i64>(8)? == 1,
                created_at: row.get(9)?,
                updated_at: row.get(10)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tasks)
}

#[tauri::command]
fn save_task(app: AppHandle, input: TaskInput) -> AppResult<Task> {
    validate_task(&input)?;
    let now = now_ms();
    let task = Task {
        id: Uuid::new_v4().to_string(),
        name: input.name.trim().to_string(),
        command: input.command.trim().to_string(),
        cwd: clean_optional(input.cwd),
        target: input.target,
        ssh_host: clean_optional(input.ssh_host),
        env_json: input.env_json.unwrap_or_else(|| "{}".to_string()),
        auto_restart: input.auto_restart,
        system_autostart: input.system_autostart,
        created_at: now,
        updated_at: now,
    };
    let conn = open_db(&app)?;
    conn.execute(
        "insert into tasks (id, name, command, cwd, target, ssh_host, env_json, auto_restart, system_autostart, created_at, updated_at)
         values (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        params![
            task.id,
            task.name,
            task.command,
            task.cwd,
            task.target,
            task.ssh_host,
            task.env_json,
            bool_to_i64(task.auto_restart),
            bool_to_i64(task.system_autostart),
            task.created_at,
            task.updated_at
        ],
    )?;
    if task.system_autostart && task.target == "local" {
        install_local_autostart(&task)?;
    }
    Ok(task)
}

#[tauri::command]
fn delete_task(app: AppHandle, id: String) -> AppResult<()> {
    let conn = open_db(&app)?;
    conn.execute("delete from tasks where id = ?1", params![id])?;
    Ok(())
}

#[tauri::command]
fn record_user_action(app: AppHandle, input: ActionLogInput) -> AppResult<ActionLog> {
    let log = ActionLog {
        id: Uuid::new_v4().to_string(),
        action: input.action.trim().to_string(),
        entity_id: clean_optional(input.entity_id),
        summary: input.summary.trim().to_string(),
        created_at: now_ms(),
    };
    let conn = open_db(&app)?;
    conn.execute(
        "insert into action_logs (id, action, entity_id, summary, created_at)
         values (?1, ?2, ?3, ?4, ?5)",
        params![
            log.id,
            log.action,
            log.entity_id,
            log.summary,
            log.created_at
        ],
    )?;
    Ok(log)
}

#[tauri::command]
fn list_user_action_logs(app: AppHandle, limit: Option<i64>) -> AppResult<Vec<ActionLog>> {
    let conn = open_db(&app)?;
    let limit = limit.unwrap_or(200).clamp(1, 1000);
    let mut stmt = conn.prepare(
        "select id, action, entity_id, summary, created_at
         from action_logs order by created_at desc limit ?1",
    )?;
    let logs = stmt
        .query_map(params![limit], |row| {
            Ok(ActionLog {
                id: row.get(0)?,
                action: row.get(1)?,
                entity_id: row.get(2)?,
                summary: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(logs)
}

#[tauri::command]
fn start_process(
    state: tauri::State<ProcessState>,
    input: StartProcessInput,
) -> AppResult<RunningProcess> {
    let mut processes = state
        .processes
        .lock()
        .map_err(|_| AppError::Message("process registry is unavailable".to_string()))?;
    if let Some(process) = processes.get_mut(&input.id) {
        if process.child.try_wait()?.is_none() {
            return Ok(RunningProcess {
                id: input.id,
                running: true,
            });
        }
        archive_completed_log(&state, &input.id, Arc::clone(&process.log))?;
        processes.remove(&input.id);
    }
    state
        .completed_logs
        .lock()
        .map_err(|_| AppError::Message("completed log registry is unavailable".to_string()))?
        .remove(&input.id);

    let mut command = process_command(&input)?;
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn()?;
    let log = Arc::new(Mutex::new(String::new()));
    if let Some(stdout) = child.stdout.take() {
        pipe_log(stdout, Arc::clone(&log));
    }
    if let Some(stderr) = child.stderr.take() {
        pipe_log(stderr, Arc::clone(&log));
    }

    processes.insert(input.id.clone(), ManagedProcess { child, log });
    Ok(RunningProcess {
        id: input.id,
        running: true,
    })
}

#[tauri::command]
fn stop_process(state: tauri::State<ProcessState>, id: String) -> AppResult<RunningProcess> {
    let mut processes = state
        .processes
        .lock()
        .map_err(|_| AppError::Message("process registry is unavailable".to_string()))?;
    if let Some(mut process) = processes.remove(&id) {
        let _ = process.child.kill();
        let _ = process.child.wait();
        archive_completed_log(&state, &id, Arc::clone(&process.log))?;
    }
    Ok(RunningProcess { id, running: false })
}

#[tauri::command]
fn process_status(state: tauri::State<ProcessState>, id: String) -> AppResult<RunningProcess> {
    let mut processes = state
        .processes
        .lock()
        .map_err(|_| AppError::Message("process registry is unavailable".to_string()))?;
    let running = if let Some(process) = processes.get_mut(&id) {
        process.child.try_wait()?.is_none()
    } else {
        false
    };
    if !running {
        if let Some(process) = processes.remove(&id) {
            archive_completed_log(&state, &id, Arc::clone(&process.log))?;
        }
    }
    Ok(RunningProcess { id, running })
}

#[tauri::command]
fn process_log(state: tauri::State<ProcessState>, id: String) -> AppResult<String> {
    let processes = state
        .processes
        .lock()
        .map_err(|_| AppError::Message("process registry is unavailable".to_string()))?;
    if let Some(process) = processes.get(&id) {
        let log = process
            .log
            .lock()
            .map_err(|_| AppError::Message("process log is unavailable".to_string()))?;
        return Ok(log.clone());
    }
    drop(processes);
    let completed_logs = state
        .completed_logs
        .lock()
        .map_err(|_| AppError::Message("completed log registry is unavailable".to_string()))?;
    Ok(completed_logs.get(&id).cloned().unwrap_or_default())
}

#[tauri::command]
fn verify_system_auth() -> AppResult<()> {
    #[cfg(target_os = "macos")]
    {
        let status = Command::new("osascript")
            .args([
                "-e",
                "do shell script \"true\" with administrator privileges with prompt \"TeRmalM needs system verification to reveal the full command.\"",
            ])
            .status()?;
        if status.success() {
            return Ok(());
        }
        return Err(AppError::Message(
            "system verification was cancelled".to_string(),
        ));
    }

    #[cfg(target_os = "windows")]
    {
        let status = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                "Start-Process cmd -ArgumentList '/c exit 0' -Verb RunAs -Wait",
            ])
            .status()?;
        if status.success() {
            return Ok(());
        }
        return Err(AppError::Message(
            "system verification was cancelled".to_string(),
        ));
    }

    #[cfg(target_os = "linux")]
    {
        let status = Command::new("pkexec").arg("true").status()?;
        if status.success() {
            return Ok(());
        }
        return Err(AppError::Message(
            "system verification was cancelled".to_string(),
        ));
    }
}

#[tauri::command]
fn pty_start(state: tauri::State<ProcessState>, input: PtyStartInput) -> AppResult<PtySnapshot> {
    let pty_system = native_pty_system();
    let pair = pty_system.openpty(PtySize {
        rows: input.rows.unwrap_or(28),
        cols: input.cols.unwrap_or(100),
        pixel_width: 0,
        pixel_height: 0,
    })?;

    let mut cmd = if let Some(host) = input
        .ssh_host
        .clone()
        .filter(|value| !value.trim().is_empty())
    {
        let mut builder = CommandBuilder::new("ssh");
        builder.arg(host);
        builder
    } else {
        let shell = input.shell.unwrap_or_else(default_shell);
        CommandBuilder::new(shell)
    };

    if let Some(cwd) = input.cwd.filter(|value| !value.trim().is_empty()) {
        cmd.cwd(cwd);
    }

    let child = pair.slave.spawn_command(cmd)?;
    let mut reader = pair.master.try_clone_reader()?;
    let writer = pair.master.take_writer()?;
    let output = Arc::new(Mutex::new(String::new()));
    let output_reader = Arc::clone(&output);
    thread::spawn(move || {
        let mut buffer = [0_u8; 8192];
        loop {
            match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => append_limited(&output_reader, &String::from_utf8_lossy(&buffer[..n])),
                Err(_) => break,
            }
        }
    });

    let id = Uuid::new_v4().to_string();
    state
        .ptys
        .lock()
        .map_err(|_| AppError::Message("pty registry is unavailable".to_string()))?
        .insert(
            id.clone(),
            PtySession {
                child: Mutex::new(child),
                writer: Mutex::new(writer),
                output: Arc::clone(&output),
            },
        );
    Ok(PtySnapshot {
        id,
        output: String::new(),
        running: true,
    })
}

#[tauri::command]
fn pty_write(state: tauri::State<ProcessState>, id: String, data: String) -> AppResult<()> {
    let ptys = state
        .ptys
        .lock()
        .map_err(|_| AppError::Message("pty registry is unavailable".to_string()))?;
    let Some(session) = ptys.get(&id) else {
        return Err(AppError::Message(
            "terminal session does not exist".to_string(),
        ));
    };
    let mut writer = session
        .writer
        .lock()
        .map_err(|_| AppError::Message("terminal writer is unavailable".to_string()))?;
    writer.write_all(data.as_bytes())?;
    writer.flush()?;
    Ok(())
}

#[tauri::command]
fn pty_read(state: tauri::State<ProcessState>, id: String) -> AppResult<PtySnapshot> {
    let mut ptys = state
        .ptys
        .lock()
        .map_err(|_| AppError::Message("pty registry is unavailable".to_string()))?;
    let (running, chunk) = {
        let Some(session) = ptys.get_mut(&id) else {
            return Ok(PtySnapshot {
                id,
                output: String::new(),
                running: false,
            });
        };
        let running = {
            let mut child = session
                .child
                .lock()
                .map_err(|_| AppError::Message("terminal child is unavailable".to_string()))?;
            child.try_wait()?.is_none()
        };
        let mut output = session
            .output
            .lock()
            .map_err(|_| AppError::Message("terminal output is unavailable".to_string()))?;
        let chunk = output.clone();
        output.clear();
        (running, chunk)
    };
    if !running {
        ptys.remove(&id);
    }
    Ok(PtySnapshot {
        id,
        output: chunk,
        running,
    })
}

#[tauri::command]
fn pty_stop(state: tauri::State<ProcessState>, id: String) -> AppResult<()> {
    let mut ptys = state
        .ptys
        .lock()
        .map_err(|_| AppError::Message("pty registry is unavailable".to_string()))?;
    if let Some(session) = ptys.remove(&id) {
        if let Ok(mut child) = session.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    Ok(())
}

fn validate_task(input: &TaskInput) -> AppResult<()> {
    if input.name.trim().is_empty() {
        return Err(AppError::Message("task name is required".to_string()));
    }
    if input.command.trim().is_empty() {
        return Err(AppError::Message("command is required".to_string()));
    }
    if input.target != "local" && input.target != "ssh" {
        return Err(AppError::Message("target must be local or ssh".to_string()));
    }
    if input.target == "ssh" && input.ssh_host.as_deref().unwrap_or("").trim().is_empty() {
        return Err(AppError::Message(
            "ssh host is required for remote tasks".to_string(),
        ));
    }
    Ok(())
}

fn process_command(input: &StartProcessInput) -> AppResult<Command> {
    if input.target == "ssh" {
        let host = input
            .ssh_host
            .clone()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| AppError::Message("ssh host is required".to_string()))?;
        let mut command = Command::new("ssh");
        command.arg(host).arg(input.command.clone());
        Ok(command)
    } else {
        let mut command = shell_command(&input.command);
        if let Some(cwd) = input.cwd.clone().filter(|value| !value.trim().is_empty()) {
            command.current_dir(cwd);
        }
        Ok(command)
    }
}

fn shell_command(command_text: &str) -> Command {
    #[cfg(target_os = "windows")]
    {
        let mut command = Command::new("cmd");
        command.args(["/C", command_text]);
        command
    }
    #[cfg(not(target_os = "windows"))]
    {
        let mut command = Command::new("sh");
        command.args(["-lc", command_text]);
        command
    }
}

fn pipe_log<R>(reader: R, log: Arc<Mutex<String>>)
where
    R: Read + Send + 'static,
{
    thread::spawn(move || {
        let reader = BufReader::new(reader);
        for line in reader.lines().map_while(Result::ok) {
            append_limited(&log, &(line + "\n"));
        }
    });
}

fn append_limited(output: &Arc<Mutex<String>>, chunk: &str) {
    if let Ok(mut value) = output.lock() {
        value.push_str(chunk);
        if value.len() > 200_000 {
            let keep_from = value.len().saturating_sub(160_000);
            value.replace_range(..keep_from, "");
        }
    }
}

fn archive_completed_log(
    state: &tauri::State<ProcessState>,
    id: &str,
    log: Arc<Mutex<String>>,
) -> AppResult<()> {
    thread::sleep(std::time::Duration::from_millis(80));
    let snapshot = log
        .lock()
        .map_err(|_| AppError::Message("process log is unavailable".to_string()))?
        .clone();
    state
        .completed_logs
        .lock()
        .map_err(|_| AppError::Message("completed log registry is unavailable".to_string()))?
        .insert(id.to_string(), snapshot);
    Ok(())
}

fn parse_ssh_config(path: &Path) -> AppResult<Vec<SshHost>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(path)?;
    let mut hosts = Vec::new();
    let mut current: Vec<SshHost> = Vec::new();

    for raw_line in text.lines() {
        let line = raw_line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let Some(key) = parts.next() else {
            continue;
        };
        let values: Vec<String> = parts.map(ToString::to_string).collect();
        match key.to_ascii_lowercase().as_str() {
            "host" => {
                hosts.append(&mut current);
                current = values
                    .into_iter()
                    .filter(|alias| !alias.contains('*') && !alias.contains('?') && alias != "!")
                    .map(|alias| SshHost {
                        alias,
                        hostname: None,
                        user: None,
                        port: None,
                    })
                    .collect();
            }
            "hostname" => {
                set_current_field(&mut current, |host| host.hostname = values.first().cloned());
            }
            "user" => {
                set_current_field(&mut current, |host| host.user = values.first().cloned());
            }
            "port" => {
                set_current_field(&mut current, |host| host.port = values.first().cloned());
            }
            _ => {}
        }
    }
    hosts.append(&mut current);
    hosts.sort_by(|a, b| a.alias.cmp(&b.alias));
    hosts.dedup_by(|a, b| a.alias == b.alias);
    Ok(hosts)
}

fn set_current_field<F>(hosts: &mut [SshHost], mut setter: F)
where
    F: FnMut(&mut SshHost),
{
    for host in hosts {
        setter(host);
    }
}

fn ssh_config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".ssh")
        .join("config")
}

fn open_db(app: &AppHandle) -> AppResult<Connection> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|err| AppError::Message(err.to_string()))?;
    fs::create_dir_all(&data_dir)?;
    let conn = Connection::open(data_dir.join("termalm.sqlite3"))?;
    init_db(&conn)?;
    Ok(conn)
}

fn init_db(conn: &Connection) -> AppResult<()> {
    conn.execute_batch(
        "create table if not exists tasks (
            id text primary key,
            name text not null,
            command text not null,
            cwd text,
            target text not null check (target in ('local', 'ssh')),
            ssh_host text,
            env_json text not null default '{}',
            auto_restart integer not null default 0,
            system_autostart integer not null default 0,
            created_at integer not null,
            updated_at integer not null
        );
        create table if not exists task_runs (
            id text primary key,
            task_id text not null,
            started_at integer not null,
            ended_at integer,
            exit_code integer,
            log_tail text
        );
        create table if not exists action_logs (
            id text primary key,
            action text not null,
            entity_id text,
            summary text not null,
            created_at integer not null
        );",
    )?;
    Ok(())
}

fn install_local_autostart(task: &Task) -> AppResult<()> {
    #[cfg(target_os = "macos")]
    {
        let launch_agents = dirs::home_dir()
            .ok_or_else(|| AppError::Message("home directory is unavailable".to_string()))?
            .join("Library")
            .join("LaunchAgents");
        fs::create_dir_all(&launch_agents)?;
        let label = format!("com.termalm.task.{}", task.id);
        let plist = launch_agents.join(format!("{label}.plist"));
        let content = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key><string>{label}</string>
  <key>ProgramArguments</key>
  <array><string>/bin/sh</string><string>-lc</string><string>{command}</string></array>
  <key>RunAtLoad</key><true/>
  <key>KeepAlive</key><{keep_alive}/>
</dict>
</plist>
"#,
            label = label,
            command = xml_escape(&task.command),
            keep_alive = if task.auto_restart { "true" } else { "false" }
        );
        fs::write(plist, content)?;
    }

    #[cfg(target_os = "linux")]
    {
        let user_dir = dirs::home_dir()
            .ok_or_else(|| AppError::Message("home directory is unavailable".to_string()))?
            .join(".config")
            .join("systemd")
            .join("user");
        fs::create_dir_all(&user_dir)?;
        let service = user_dir.join(format!("termalm-{}.service", task.id));
        let content = format!(
            "[Unit]\nDescription=TeRmalM task {name}\n\n[Service]\nType=simple\nExecStart=/bin/sh -lc '{command}'\nRestart={restart}\n\n[Install]\nWantedBy=default.target\n",
            name = task.name,
            command = task.command.replace('\'', "'\\''"),
            restart = if task.auto_restart { "always" } else { "no" }
        );
        fs::write(service, content)?;
    }

    #[cfg(target_os = "windows")]
    {
        let task_name = format!("TeRmalM-{}", task.id);
        let status = Command::new("schtasks")
            .args([
                "/Create",
                "/SC",
                "ONLOGON",
                "/TN",
                &task_name,
                "/TR",
                &format!("cmd /C {}", task.command),
                "/F",
            ])
            .status()?;
        if !status.success() {
            return Err(AppError::Message(
                "failed to create Windows scheduled task".to_string(),
            ));
        }
    }

    Ok(())
}

fn default_shell() -> String {
    #[cfg(target_os = "windows")]
    {
        std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
    }
}

fn bool_to_i64(value: bool) -> i64 {
    if value {
        1
    } else {
        0
    }
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value.and_then(|text| {
        let trimmed = text.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as i64)
        .unwrap_or_default()
}

#[cfg(target_os = "macos")]
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

pub fn run() {
    tauri::Builder::default()
        .manage(ProcessState::default())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--background"]),
        ))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_sql::Builder::default().build())
        .setup(|app| {
            let handle = app.handle().clone();
            let conn = open_db(&handle).map_err(Box::<dyn std::error::Error>::from)?;
            drop(conn);
            let show = MenuItem::with_id(app, "show", "Show TeRmalM", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().unwrap_or_else(|| {
                    tauri::include_image!("icons/32x32.png").to_owned()
                }))
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_main_window(tray.app_handle());
                    }
                })
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "show" => show_main_window(app),
                    "quit" => app.exit(0),
                    _ => {}
                })
                .build(app)?;
            if std::env::args().any(|arg| arg == "--background") {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            delete_task,
            list_ssh_hosts,
            list_tasks,
            list_user_action_logs,
            process_log,
            process_status,
            pty_read,
            pty_start,
            pty_stop,
            pty_write,
            record_user_action,
            save_task,
            start_process,
            stop_process,
            verify_system_auth
        ])
        .build(tauri::generate_context!())
        .expect("error while building TeRmalM")
        .run(|app, event| {
            #[cfg(target_os = "macos")]
            if let RunEvent::Reopen {
                has_visible_windows,
                ..
            } = event
            {
                if !has_visible_windows {
                    show_main_window(app);
                }
            }
        });
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
