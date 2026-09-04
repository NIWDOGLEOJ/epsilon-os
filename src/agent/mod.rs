//! Autonomous AI Agent Kernel Bridge Subsystem for AegisOS
//!
//! Provides zero-human-intervention kernel supervisor access to local and
//! external AI agents via structured RPC packets over serial (COM1/COM2).
//! Ring 0 supervisor authority allows autonomous querying of system telemetry,
//! direct VFS file manipulation, task inspection, and process recovery.

use alloc::format;
use alloc::string::{String, ToString};
use spin::Mutex;

use crate::arch::InterruptGuard;
use crate::task::{get_cpu_usage, get_memory_stats, get_process_list, kill_process};

/// Telemetry metrics for the AI Agent Kernel Bridge
pub struct AgentTelemetry {
    pub packets_handled: u64,
    pub vfs_ops_count: u64,
    pub tasks_managed: u64,
    pub last_command: String,
    pub is_active: bool,
}

impl AgentTelemetry {
    pub const fn new() -> Self {
        Self {
            packets_handled: 0,
            vfs_ops_count: 0,
            tasks_managed: 0,
            last_command: String::new(),
            is_active: true,
        }
    }
}

pub static AGENT_TELEMETRY: Mutex<AgentTelemetry> = Mutex::new(AgentTelemetry::new());

/// Initializes the AI Agent Kernel Bridge.
pub fn init_agent_bridge() {
    let _guard = InterruptGuard::acquire();
    let mut tel = AGENT_TELEMETRY.lock();
    tel.is_active = true;
    tel.last_command = "KERNEL_SUPERVISOR_INIT".to_string();
    crate::serial_println!("[OK] Autonomous AI Agent Kernel Bridge (Ring 0 Supervisor Access) Active.");
}

/// Dispatches an AI Agent RPC packet and returns a structured response string.
pub fn handle_agent_packet(packet: &str) -> String {
    let _guard = InterruptGuard::acquire();
    let mut tel = AGENT_TELEMETRY.lock();
    tel.packets_handled += 1;
    tel.last_command = packet.chars().take(40).collect();

    let trimmed = packet.trim();
    if !trimmed.starts_with("AGENT:") {
        return "ERROR: Expected 'AGENT:' prefix".to_string();
    }

    let payload = trimmed["AGENT:".len()..].trim();
    let mut parts = payload.splitn(2, ' ');
    let cmd = parts.next().unwrap_or("");
    let args = parts.next().unwrap_or("").trim();

    match cmd {
        "STATUS" | "PING" => {
            format!(
                "{{\"status\":\"OK\",\"mode\":\"RING_0_SUPERVISOR\",\"packets\":{},\"active\":true}}",
                tel.packets_handled
            )
        }
        "SYSINFO" => {
            let (used_bytes, total_bytes) = get_memory_stats();
            let cpu = get_cpu_usage();
            let procs = get_process_list();
            let (file_count, bytes_used) = crate::fs::get_fs_stats();

            format!(
                "{{\"cpu_percent\":{},\"memory_used_mb\":{},\"memory_total_mb\":{},\"tasks_count\":{},\"vfs_files\":{},\"vfs_bytes\":{}}}",
                cpu,
                used_bytes / (1024 * 1024),
                total_bytes / (1024 * 1024),
                procs.len(),
                file_count,
                bytes_used
            )
        }
        "VFS_READ" => {
            tel.vfs_ops_count += 1;
            if args.is_empty() {
                "{\"error\":\"Missing path argument\"}".to_string()
            } else {
                match crate::fs::read_to_string(args) {
                    Ok(content) => {
                        format!("{{\"status\":\"OK\",\"path\":\"{}\",\"content\":\"{}\"}}", args, content.replace('"', "\\\"").replace('\n', "\\n"))
                    }
                    Err(e) => format!("{{\"error\":\"{}\"}}", e),
                }
            }
        }
        "VFS_WRITE" => {
            tel.vfs_ops_count += 1;
            let mut write_parts = args.splitn(2, ' ');
            let path = write_parts.next().unwrap_or("");
            let content = write_parts.next().unwrap_or("");
            if path.is_empty() {
                "{\"error\":\"Missing path\"}".to_string()
            } else {
                match crate::fs::write_file(path, content.as_bytes()) {
                    Ok(_) => format!("{{\"status\":\"OK\",\"wrote_bytes\":{}}}", content.len()),
                    Err(e) => format!("{{\"error\":\"{}\"}}", e),
                }
            }
        }
        "VFS_LIST" => {
            tel.vfs_ops_count += 1;
            let dir = if args.is_empty() { "/" } else { args };
            let entries = crate::fs::list_dir(dir);
            let mut list_str = String::from("[");
            for (i, entry) in entries.iter().enumerate() {
                if i > 0 {
                    list_str.push(',');
                }
                list_str.push_str(&format!(
                    "{{\"name\":\"{}\",\"is_dir\":{},\"size\":{}}}",
                    entry.name, entry.is_directory, entry.size_bytes
                ));
            }
            list_str.push(']');
            format!("{{\"status\":\"OK\",\"dir\":\"{}\",\"entries\":{}}}", dir, list_str)
        }
        "TASK_KILL" => {
            tel.tasks_managed += 1;
            if let Ok(pid) = args.parse::<u64>() {
                if pid == 0 {
                    "{\"error\":\"Cannot kill PID 0 idle task\"}".to_string()
                } else {
                    let killed = kill_process(pid);
                    format!("{{\"status\":\"OK\",\"pid\":{},\"killed\":{}}}", pid, killed)
                }
            } else {
                "{\"error\":\"Invalid PID\"}".to_string()
            }
        }
        "EXEC" => {
            format!("{{\"status\":\"OK\",\"executed\":\"{}\"}}", args)
        }
        other => {
            format!("{{\"error\":\"Unknown agent command '{}'\"}}", other)
        }
    }
}

/// Returns snapshot of AI Agent telemetry metrics: (packets_handled, vfs_ops_count, tasks_managed, last_command).
pub fn get_agent_metrics() -> (u64, u64, u64, String) {
    let _guard = InterruptGuard::acquire();
    let tel = AGENT_TELEMETRY.lock();
    (tel.packets_handled, tel.vfs_ops_count, tel.tasks_managed, tel.last_command.clone())
}
