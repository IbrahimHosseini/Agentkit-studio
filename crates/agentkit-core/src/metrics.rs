use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashSet;
use sysinfo::{Pid, System};

fn tree(system: &System, root: Pid) -> HashSet<Pid> {
    let mut included = HashSet::new();
    if system.process(root).is_none() {
        return included;
    }
    included.insert(root);
    loop {
        let mut changed = false;
        for (pid, process) in system.processes() {
            if process
                .parent()
                .is_some_and(|parent| included.contains(&parent))
                && included.insert(*pid)
            {
                changed = true;
            }
        }
        if !changed {
            break;
        }
    }
    included
}
fn summary(system: &System, pids: &HashSet<Pid>, pid: u32) -> Value {
    let mut processes = Vec::new();
    let mut memory = 0u64;
    let mut cpu = 0f64;
    for id in pids {
        if let Some(process) = system.process(*id) {
            memory = memory.saturating_add(process.memory());
            cpu += f64::from(process.cpu_usage());
            processes.push(json!({"pid":id.as_u32(),"parent_pid":process.parent().map(|p|p.as_u32()),"name":process.name().to_string_lossy(),"rss_bytes":process.memory(),"cpu_percent":process.cpu_usage()}));
        }
    }
    processes.sort_by_key(|p| p["pid"].as_u64().unwrap_or(0));
    json!({"pid":pid,"present":!pids.is_empty(),"rss_bytes":memory,"cpu_percent":cpu,"process_count":processes.len(),"processes":processes})
}
pub async fn sample(gui_pid: Option<u32>) -> Result<Value> {
    Ok(tokio::task::spawn_blocking(move|| {
        let mut system=System::new_all();
        let wait=sysinfo::MINIMUM_CPU_UPDATE_INTERVAL.max(std::time::Duration::from_millis(250));
        std::thread::sleep(wait);
        system.refresh_all();
        let engine_pid=std::process::id();let engine=tree(&system,Pid::from_u32(engine_pid));
        let gui=gui_pid.map(|pid|tree(&system,Pid::from_u32(pid)));
        let mut total=engine.clone();if let Some(gui)=&gui {total.extend(gui);}
        json!({"timestamp":crate::store::now(),"platform":std::env::consts::OS,"cpu_sample_interval_ms":wait.as_millis(),"engine":summary(&system,&engine,engine_pid),"gui":gui_pid.zip(gui.as_ref()).map(|(pid,pids)|summary(&system,pids,pid)),"total":summary(&system,&total,engine_pid),"notes":["RSS and CPU cover the engine and supplied GUI PID plus live descendants; shared WebView helpers outside those process trees are not included.","RSS sums can count shared pages more than once; values are not proportional or unique physical memory.","CPU is sampled over the stated warmup interval; totals can exceed 100 percent on multiple cores.","The sample includes the engine's own collection overhead."]})
    }).await?)
}
