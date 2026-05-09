use std::sync::Arc;

use serde::Serialize;
use sysinfo::{
    CpuRefreshKind, Disks, MemoryRefreshKind, Networks, ProcessRefreshKind, RefreshKind, System,
    Users,
};
use tokio::sync::RwLock;

#[derive(Clone, Debug, Serialize, Default)]
pub struct MetricsSnapshot {
    pub ts: i64,
    pub cpu_percent: f32,
    pub cpu_per_core: Vec<f32>,
    pub mem_total: u64,
    pub mem_used: u64,
    pub swap_total: u64,
    pub swap_used: u64,
    pub disks: Vec<DiskUsage>,
    pub net_rx_bps: u64,
    pub net_tx_bps: u64,
    pub load_avg: [f64; 3],
    pub uptime: u64,
    pub host: String,
    pub kernel: Option<String>,
    pub os: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DiskUsage {
    pub mount: String,
    pub total: u64,
    pub available: u64,
    pub fs: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProcessRow {
    pub pid: u32,
    pub name: String,
    pub cmd: String,
    pub cpu: f32,
    pub mem: u64,
    pub user: Option<String>,
    pub start_time: u64,
}

pub struct SystemCollector {
    sys: RwLock<System>,
    disks: RwLock<Disks>,
    networks: RwLock<Networks>,
    users: RwLock<Users>,
    snapshot: RwLock<MetricsSnapshot>,
}

impl SystemCollector {
    pub fn new() -> Self {
        let sys = System::new_with_specifics(
            RefreshKind::new()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything())
                .with_processes(ProcessRefreshKind::everything()),
        );
        let disks = Disks::new_with_refreshed_list();
        let networks = Networks::new_with_refreshed_list();
        let users = Users::new_with_refreshed_list();
        Self {
            sys: RwLock::new(sys),
            disks: RwLock::new(disks),
            networks: RwLock::new(networks),
            users: RwLock::new(users),
            snapshot: RwLock::new(MetricsSnapshot::default()),
        }
    }

    pub fn start_refresher(self: Arc<Self>) {
        tokio::spawn(async move {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(1));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tick.tick().await;
                self.refresh().await;
            }
        });
    }

    async fn refresh(&self) {
        let mut sys = self.sys.write().await;
        sys.refresh_cpu_usage();
        sys.refresh_memory();
        sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

        let mut disks = self.disks.write().await;
        disks.refresh();

        let mut nets = self.networks.write().await;
        nets.refresh();

        let cpu_per_core: Vec<f32> = sys.cpus().iter().map(|c| c.cpu_usage()).collect();
        let cpu_percent = if cpu_per_core.is_empty() {
            0.0
        } else {
            cpu_per_core.iter().sum::<f32>() / cpu_per_core.len() as f32
        };

        let mem_total = sys.total_memory();
        let mem_used = sys.used_memory();
        let swap_total = sys.total_swap();
        let swap_used = sys.used_swap();

        let disk_usage: Vec<DiskUsage> = disks
            .list()
            .iter()
            .filter(|d| {
                let mp = d.mount_point().to_string_lossy();
                !mp.starts_with("/snap/") && !mp.starts_with("/run/") && !mp.starts_with("/dev")
            })
            .map(|d| DiskUsage {
                mount: d.mount_point().to_string_lossy().into_owned(),
                total: d.total_space(),
                available: d.available_space(),
                fs: d.file_system().to_string_lossy().into_owned(),
            })
            .collect();

        let (rx, tx): (u64, u64) = nets
            .list()
            .iter()
            .map(|(_, d)| (d.received(), d.transmitted()))
            .fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1));

        let load = System::load_average();
        let uptime = System::uptime();
        let host = System::host_name().unwrap_or_default();
        let kernel = System::kernel_version();
        let os = System::long_os_version();

        let new_snapshot = MetricsSnapshot {
            ts: crate::db::now_unix(),
            cpu_percent,
            cpu_per_core,
            mem_total,
            mem_used,
            swap_total,
            swap_used,
            disks: disk_usage,
            net_rx_bps: rx,
            net_tx_bps: tx,
            load_avg: [load.one, load.five, load.fifteen],
            uptime,
            host,
            kernel,
            os,
        };

        *self.snapshot.write().await = new_snapshot;
    }

    pub async fn snapshot(&self) -> MetricsSnapshot {
        self.snapshot.read().await.clone()
    }

    pub async fn top_processes(&self, sort_by: SortBy, limit: usize) -> Vec<ProcessRow> {
        let sys = self.sys.read().await;
        let users = self.users.read().await;
        let mut rows: Vec<ProcessRow> = sys
            .processes()
            .iter()
            .map(|(pid, p)| ProcessRow {
                pid: pid.as_u32(),
                name: p.name().to_string_lossy().into_owned(),
                cmd: p
                    .cmd()
                    .iter()
                    .map(|s| s.to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join(" "),
                cpu: p.cpu_usage(),
                mem: p.memory(),
                user: p
                    .user_id()
                    .and_then(|uid| users.get_user_by_id(uid))
                    .map(|u| u.name().to_string()),
                start_time: p.start_time(),
            })
            .collect();
        match sort_by {
            SortBy::Cpu => rows.sort_by(|a, b| {
                b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal)
            }),
            SortBy::Mem => rows.sort_by(|a, b| b.mem.cmp(&a.mem)),
        }
        rows.truncate(limit);
        rows
    }
}

#[derive(Clone, Copy, Debug)]
pub enum SortBy {
    Cpu,
    Mem,
}

impl SortBy {
    pub fn parse(s: &str) -> Self {
        match s {
            "mem" | "memory" | "ram" => SortBy::Mem,
            _ => SortBy::Cpu,
        }
    }
}
