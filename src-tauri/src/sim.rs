use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ly_plc::{now_ms, ProtocolKind};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::time::{sleep, Instant};

use crate::cycle::CycleHost;
use crate::inspection::{read_tag_u32, tag, tag_is_on, write_tag};
use crate::plc::PlcHost;
use crate::recipe::{Recipe, TriggerMode};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Scenario {
    Normal,
    Excursion,
    Gap,
    LostFrame,
    LocateFail,
    CountMismatch,
    Random,
}

impl Scenario {
    pub fn lost_frame(self, n: usize) -> Option<usize> {
        (self == Scenario::LostFrame).then(|| 3.min(n - 1))
    }

    pub fn locate_fail_frame(self, n: usize) -> Option<usize> {
        (self == Scenario::LocateFail).then(|| 4.min(n - 1))
    }

    /// 在 k1 / k2 归属分界处两侧各放一个缺胶点：单帧看都不超限，按弧长合并后超限。
    pub fn gap_points(self, recipe: &Recipe) -> Vec<usize> {
        if self != Scenario::Gap {
            return Vec::new();
        }
        let k = &recipe.points.k;
        (0..k.len() - 1).find(|&j| k[j] == 1 && k[j + 1] == 2).map(|j| vec![j, j + 1]).unwrap_or_default()
    }

    fn resolve(self, seed: u32) -> Scenario {
        if self != Scenario::Random {
            return self;
        }
        match seed % 25 {
            0 => Scenario::Gap,
            1 => Scenario::LostFrame,
            2 => Scenario::LocateFail,
            3 | 4 => Scenario::Excursion,
            _ => Scenario::Normal,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimStatus {
    pub running: bool,
    pub continuous: bool,
    pub parts: u32,
    pub message: String,
}

/// 模拟 PLC 与机器人：写入工件信息、等待布防、发触发、给 partEnd、确认结果。仅在 PLC 协议为模拟器时可用。
#[derive(Default)]
pub struct SimCtl {
    running: AtomicBool,
    stop: AtomicBool,
    continuous: AtomicBool,
    parts: AtomicU32,
    part_scenario: Mutex<Option<Scenario>>,
    message: Mutex<String>,
}

impl SimCtl {
    pub fn part_scenario(&self) -> Scenario {
        self.part_scenario.lock().unwrap().unwrap_or(Scenario::Normal)
    }

    pub fn status(&self) -> SimStatus {
        SimStatus {
            running: self.running.load(Ordering::SeqCst),
            continuous: self.continuous.load(Ordering::SeqCst),
            parts: self.parts.load(Ordering::SeqCst),
            message: self.message.lock().unwrap().clone(),
        }
    }

    fn set_message(&self, app: &AppHandle, message: impl Into<String>) {
        *self.message.lock().unwrap() = message.into();
        let _ = app.emit("sim://status", self.status());
    }
}

async fn wait_for(app: &AppHandle, timeout: Duration, cond: impl Fn(&AppHandle) -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if cond(app) {
            return true;
        }
        sleep(Duration::from_millis(10)).await;
    }
    false
}

fn on(app: &AppHandle, t: &str) -> bool {
    tag_is_on(app.state::<PlcHost>().engine(), t)
}

async fn put(app: &AppHandle, t: &str, v: serde_json::Value) -> Result<(), String> {
    write_tag(app.state::<PlcHost>().engine(), t, v).await
}

async fn run_part(app: &AppHandle, recipe: &Recipe, scenario: Scenario) -> Result<String, String> {
    let cycle = app.state::<CycleHost>();
    let n = recipe.shot_count();
    let sn = (now_ms() / 1000 % 1_000_000_000) as u32;
    *cycle.sim.part_scenario.lock().unwrap() = Some(scenario);

    let shot_count = if scenario == Scenario::CountMismatch { n - 1 } else { n };
    put(app, tag::PART_SN, json!(sn)).await?;
    put(app, tag::PRODUCT_CODE, json!(recipe.product_code)).await?;
    put(app, tag::SHOT_COUNT, json!(shot_count)).await?;
    put(app, tag::PART_START, json!(true)).await?;

    if !wait_for(app, Duration::from_secs(3), |a| on(a, tag::ARMED) || on(a, tag::DONE)).await {
        return Err("3 s 内未收到 armed 或 done".into());
    }
    if on(app, tag::ARMED) {
        let interval = match recipe.trigger_mode {
            TriggerMode::Fly => 450,
            TriggerMode::Stop => 1100,
        };
        let lost = scenario.lost_frame(n);
        for k in 0..n {
            sleep(Duration::from_millis(interval)).await;
            let _ = cycle.camera.trigger(lost == Some(k));
        }
        sleep(Duration::from_millis(300)).await;
        put(app, tag::PART_END, json!(true)).await?;
    }

    if !wait_for(app, Duration::from_secs(10), |a| on(a, tag::DONE)).await {
        return Err("10 s 内未收到 done".into());
    }
    let code = read_tag_u32(app.state::<PlcHost>().engine(), tag::RESULT_CODE).unwrap_or(0);
    sleep(Duration::from_millis(150)).await;
    put(app, tag::RESULT_ACK, json!(true)).await?;
    wait_for(app, Duration::from_secs(3), |a| !on(a, tag::DONE)).await;
    for t in [tag::PART_START, tag::PART_END, tag::RESULT_ACK] {
        put(app, t, json!(false)).await?;
    }
    Ok(format!("SN {sn} 完成，resultCode = {code}"))
}

pub async fn run(app: AppHandle, recipe: Arc<Recipe>, scenario: Scenario, continuous: bool) {
    let sim = &app.state::<CycleHost>().sim;
    sim.continuous.store(continuous, Ordering::SeqCst);
    let mut seed = (now_ms() % 997) as u32;
    loop {
        seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
        let s = scenario.resolve(seed >> 8);
        sim.set_message(&app, format!("运行中：{}", recipe.id));
        let result = run_part(&app, &recipe, s).await;
        *sim.part_scenario.lock().unwrap() = None;
        sim.parts.fetch_add(1, Ordering::SeqCst);
        match result {
            Ok(msg) => sim.set_message(&app, msg),
            Err(e) => {
                for t in [tag::PART_START, tag::PART_END, tag::RESULT_ACK] {
                    let _ = put(&app, t, json!(false)).await;
                }
                sim.set_message(&app, format!("已中止：{e}"));
                break;
            }
        }
        if !continuous || sim.stop.load(Ordering::SeqCst) {
            break;
        }
        sleep(Duration::from_millis(800)).await;
    }
    sim.running.store(false, Ordering::SeqCst);
    let _ = app.emit("sim://status", sim.status());
}

#[tauri::command]
pub fn sim_status(cycle: State<'_, CycleHost>) -> SimStatus {
    cycle.sim.status()
}

#[tauri::command]
pub fn sim_start(
    app: AppHandle,
    plc: State<'_, PlcHost>,
    cycle: State<'_, CycleHost>,
    recipe_id: String,
    scenario: Scenario,
    continuous: bool,
) -> Result<(), String> {
    if plc.engine().config().connection.protocol != ProtocolKind::Simulator {
        return Err("模拟节拍只能在 PLC 协议为“模拟器”时使用".into());
    }
    if plc.engine().status().state != ly_plc::LinkState::Connected {
        return Err("PLC 模拟器未连接".into());
    }
    let cam = cycle.camera.config();
    if cam.source == crate::camera::CameraSource::Mvs && cam.trigger_source != "Software" {
        return Err("相机触发源为 Line0，模拟节拍发不出硬触发；改为 Software 或切换到模拟相机".into());
    }
    let recipe = cycle.recipe(&recipe_id).ok_or("配方不存在")?;
    if cycle.sim.running.swap(true, Ordering::SeqCst) {
        return Err("模拟节拍已在运行".into());
    }
    cycle.sim.stop.store(false, Ordering::SeqCst);
    tauri::async_runtime::spawn(run(app, recipe, scenario, continuous));
    Ok(())
}

#[tauri::command]
pub fn sim_stop(cycle: State<'_, CycleHost>) {
    cycle.sim.stop.store(true, Ordering::SeqCst);
    cycle.sim.continuous.store(false, Ordering::SeqCst);
}
