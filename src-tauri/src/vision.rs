//! lyFlow 视觉引擎：加载 core DLL，逐帧注入图像跑飞拍检测图（定位 + 逐点卡尺），读回 glue.Pose2D 与 glue.StationMeasure。
//! 图只量不判，判定在 judge 模块（设计稿 §9）。

use std::collections::HashMap;
use std::ffi::{c_char, c_void};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use lyflow_client::{Core, RunHandle, RunSpec};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::camera::CameraSource;
use crate::cycle::CycleHost;
use crate::recipe::Recipe;
use crate::simimage;

/// 与 LyFlow packs/glue/graphs/flyshot.lyflow.json 同一份；本程序固定用它。
pub const FLYSHOT_GRAPH: &str = include_str!("../resources/flyshot.lyflow.json");

/// 一帧 8 位灰度图，行主序、无行填充。
pub struct FrameImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl std::fmt::Debug for FrameImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FrameImage({}×{})", self.width, self.height)
    }
}

/// lyFlow main 的 C ABI 只能注入点云；glue 分支的 RunImageInput 合入后删掉这个开关，恢复逐帧注图。
const IMAGE_INPUT_UNSUPPORTED: bool = true;

unsafe extern "C" fn ignore_event(_: *const c_char, _: *mut c_void) {}

pub struct Engine {
    core: Arc<Core>,
    pub path: PathBuf,
    pub version: String,
}

impl Engine {
    pub fn load(path: &Path) -> Result<Self, String> {
        let core = Core::load_from(path).map_err(|e| e.to_string())?;
        core.self_check()?;
        let version = core.version();
        Ok(Self { core: Arc::new(core), path: path.to_path_buf(), version })
    }

    /// 跑一次图，返回 run summary 与图级命名输出（都已解析成 JSON）。
    pub fn run(&self, graph: &str, run_id: &str, base_dir: &str, _image: &FrameImage, params: &Value) -> Result<RunResult, String> {
        if IMAGE_INPUT_UNSUPPORTED {
            return Err("当前 lyFlow（main）不支持注入图像，无法运行飞拍检测图".into());
        }
        let params_json = params.to_string();
        let spec = RunSpec::new(graph, run_id, base_dir, &[]).with_params_json(&params_json);
        let handle = unsafe { RunHandle::start(self.core.clone(), spec, ignore_event, Box::new(())) }.map_err(|e| e.to_string())?;
        handle.join();
        let summary: Value = self
            .core
            .run_summary(run_id)
            .map_err(|e| e.to_string())?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let outputs: Value = serde_json::from_str(&self.core.run_outputs(run_id).map_err(|e| e.to_string())?).unwrap_or(Value::Null);
        drop(handle);
        Ok(RunResult { summary, outputs })
    }
}

pub struct RunResult {
    pub summary: Value,
    pub outputs: Value,
}

impl RunResult {
    pub fn status(&self) -> &str {
        self.summary.get("status").and_then(|s| s.as_str()).unwrap_or("failed")
    }

    /// 命名输出里 Record 的 data。
    pub fn record(&self, name: &str) -> Option<&Value> {
        let v = self.outputs.get(name)?.get("value")?;
        v.get("data").or(Some(v))
    }

    /// 运行失败时给人看的原因：失败输出的 root 节点与错误。
    pub fn failure(&self) -> String {
        let mut parts = Vec::new();
        if let Some(outs) = self.summary.get("outputs").and_then(|o| o.as_object()) {
            for (name, o) in outs {
                if o.get("state").and_then(|s| s.as_str()) == Some("failed") {
                    let root = o.get("root").and_then(|r| r.as_str()).unwrap_or("?");
                    let code = o.get("rootCode").and_then(|r| r.as_str()).unwrap_or("?");
                    parts.push(format!("{name}：{root}（{code}）"));
                }
            }
        }
        if parts.is_empty() {
            format!("lyFlow 运行状态 {}", self.status())
        } else {
            parts.join("；")
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
pub struct Pose {
    pub ok: bool,
    #[serde(default)]
    pub score: f64,
}

/// glue.StationMeasure 里本程序用到的字段。
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StationMeasure {
    pub unit: String,
    pub ids: Vec<Value>,
    pub status: Vec<String>,
    pub inner_center: Vec<Option<f64>>,
}

/// 一个拍照点的视觉资料（示教产物）：模板、模板锚点、测量点文件。
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShotAssets {
    pub template: PathBuf,
    pub anchor: [f64; 2],
    pub stations: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisionAssets {
    pub recipe_id: String,
    pub recipe_hash: String,
    /// 模拟相机的像素当量；真实相机为 None（用工位标定）
    pub sim_mm_per_px: Option<f64>,
    pub calib: PathBuf,
    pub shots: Vec<ShotAssets>,
}

impl VisionAssets {
    pub fn params(&self, k: usize) -> Option<Value> {
        let s = self.shots.get(k)?;
        let p = |p: &Path| p.to_string_lossy().replace('\\', "/");
        Some(json!({
            "template": p(&s.template),
            "anchor": s.anchor,
            "stations": p(&s.stations),
            "calib": p(&self.calib),
        }))
    }

    pub fn load(path: &Path) -> Option<Self> {
        std::fs::read_to_string(path).ok().and_then(|s| serde_json::from_str(&s).ok())
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        std::fs::write(path, serde_json::to_string_pretty(self).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    }
}

#[derive(Clone, Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatus {
    pub loaded: bool,
    pub path: Option<String>,
    pub version: Option<String>,
    pub message: String,
}

/// 引擎与各配方视觉资料的缓存。引擎按设置里的 DLL 路径懒加载，路径变了重载。
#[derive(Default)]
pub struct VisionHost {
    engine: Mutex<Option<Arc<Engine>>>,
    error: Mutex<Option<String>>,
    assets: Mutex<HashMap<String, Arc<VisionAssets>>>,
    generating: Mutex<()>,
}

impl VisionHost {
    pub fn engine(&self, path: Option<&str>) -> Option<Arc<Engine>> {
        let path = path.map(str::trim).filter(|p| !p.is_empty())?;
        // LOAD_WITH_ALTERED_SEARCH_PATH 遇到正斜杠行为未定义（依赖 DLL 会找不到）
        let path = PathBuf::from(if cfg!(windows) { path.replace('/', "\\") } else { path.to_string() });
        let mut guard = self.engine.lock().unwrap();
        if let Some(e) = guard.as_ref().filter(|e| e.path == path) {
            return Some(e.clone());
        }
        match Engine::load(&path) {
            Ok(e) => {
                let e = Arc::new(e);
                *guard = Some(e.clone());
                *self.error.lock().unwrap() = None;
                Some(e)
            }
            Err(msg) => {
                *guard = None;
                *self.error.lock().unwrap() = Some(msg);
                None
            }
        }
    }

    pub fn status(&self, path: Option<&str>) -> EngineStatus {
        let engine = self.engine(path);
        EngineStatus {
            loaded: engine.is_some(),
            path: path.map(String::from),
            version: engine.as_ref().map(|e| e.version.clone()),
            message: match (&engine, path.filter(|p| !p.trim().is_empty())) {
                (Some(_), _) => "已加载".into(),
                (None, None) => "未配置 lyFlow 核心库路径，使用模拟测量".into(),
                (None, Some(_)) => self.error.lock().unwrap().clone().unwrap_or_else(|| "加载失败".into()),
            },
        }
    }

    pub fn assets(&self, key: &str) -> Option<Arc<VisionAssets>> {
        self.assets.lock().unwrap().get(key).cloned()
    }

    pub fn set_assets(&self, key: String, assets: Arc<VisionAssets>) {
        self.assets.lock().unwrap().insert(key, assets);
    }
}

/// 工位标定文件（标定属于相机工位，换型不重标）。
pub fn station_calib_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_config_dir().map_err(|e| e.to_string())?.join("calib").join("plane_calib.json"))
}

/// 配方在当前相机下的视觉资料。模拟相机按名义几何自动生成（按配方哈希缓存）；
/// 真实相机用示教向导的产物，标定取工位标定文件。
pub fn assets_for(app: &AppHandle, recipe: &Recipe) -> Result<Arc<VisionAssets>, String> {
    let source = app.state::<CycleHost>().camera.config().source;
    let host = app.state::<VisionHost>();
    let key = format!("{source:?}:{}:{}", recipe.id, recipe.hash);
    if let Some(a) = host.assets(&key) {
        return Ok(a);
    }
    let _generating = host.generating.lock().unwrap();
    if let Some(a) = host.assets(&key) {
        return Ok(a);
    }
    let root = app.path().app_data_dir().map_err(|e| e.to_string())?.join("vision");
    let name = format!("{}-{}", recipe.id, &recipe.hash[..8.min(recipe.hash.len())]);
    let assets = match source {
        CameraSource::Sim => {
            let dir = root.join("sim").join(name);
            let file = dir.join("vision.json");
            match VisionAssets::load(&file).filter(|a| a.recipe_hash == recipe.hash && a.shots.iter().all(|s| s.template.exists())) {
                Some(a) => a,
                None => {
                    let a = simimage::teach(recipe, &dir)?;
                    a.save(&file)?;
                    a
                }
            }
        }
        CameraSource::Mvs => {
            let dir = root.join("taught").join(name);
            let mut a = VisionAssets::load(&dir.join("vision.json")).ok_or_else(|| format!("配方 {} 尚未示教", recipe.id))?;
            a.calib = station_calib_path(app)?;
            a
        }
    };
    let assets = Arc::new(assets);
    host.set_assets(key, assets.clone());
    Ok(assets)
}

/// 启用 lyFlow 测量且核心库加载成功时，取图回调才拷贝整帧。设置变了之后调一次。
pub fn apply_settings(app: &AppHandle) -> bool {
    let settings = app.state::<CycleHost>().settings();
    let on = settings.vision && app.state::<VisionHost>().engine(settings.lyflow_core.as_deref()).is_some();
    app.state::<CycleHost>().camera.set_capture_full(on);
    on
}

/// 当前是否用 lyFlow 测量。
pub fn enabled(app: &AppHandle) -> bool {
    let settings = app.state::<CycleHost>().settings();
    settings.vision && app.state::<VisionHost>().engine(settings.lyflow_core.as_deref()).is_some()
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalibInfo {
    pub path: String,
    pub rms: Option<f64>,
    pub mm_per_px: Option<f64>,
    pub max_error: Option<f64>,
    pub pattern: Option<Vec<f64>>,
    pub square: Option<f64>,
    pub ts: Option<i64>,
}

fn calib_info(path: &Path) -> Option<CalibInfo> {
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let d = doc.get("data").unwrap_or(&doc);
    let f = |k: &str| d.get(k).and_then(|v| v.as_f64());
    Some(CalibInfo {
        path: path.display().to_string(),
        rms: f("rms"),
        mm_per_px: f("mmPerPx"),
        max_error: f("maxError"),
        pattern: d.get("pattern").and_then(|v| serde_json::from_value(v.clone()).ok()),
        square: f("square"),
        ts: doc.get("ts").and_then(|v| v.as_i64()),
    })
}

#[tauri::command]
pub fn vision_calib_info(app: AppHandle) -> Result<Option<CalibInfo>, String> {
    Ok(calib_info(&station_calib_path(&app)?))
}

/// 工位标定：用相机最近一帧整图跑 image.board_calib，结果存成工位标定文件。
#[tauri::command]
pub async fn vision_calibrate(app: AppHandle, pattern: [f64; 2], square: f64) -> Result<CalibInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let settings = app.state::<CycleHost>().settings();
        let engine = app.state::<VisionHost>().engine(settings.lyflow_core.as_deref()).ok_or("lyFlow 核心库未加载")?;
        let image = app
            .state::<CycleHost>()
            .camera
            .last_full()
            .ok_or("还没有整帧图像：启用 lyFlow 测量后软触发一帧（标定板放在内边所在高度）")?;
        let graph = json!({
            "schemaVersion": 1,
            "id": "01TUJIAOBOARDCALIB00000000",
            "nodes": [
                {"id": "n_load", "op": "io.load_image", "params": {"source": "inputs"}},
                {"id": "n_calib", "op": "image.board_calib", "params": {"pattern": pattern, "square": square}}
            ],
            "edges": [{"id": "e0", "from": {"node": "n_load", "port": "image"}, "to": {"node": "n_calib", "port": "image"}}],
            "outputs": {"calib": {"node": "n_calib", "port": "calib"}}
        });
        let run_id = format!("calib-{}", ly_plc::now_ms());
        let r = engine.run(&graph.to_string(), &run_id, "", &image, &json!({}))?;
        if r.status() == "failed" {
            let why = r.failure();
            return Err(if why.contains("no_board") {
                format!("没找到 {}×{} 个内角点的棋盘格：确认 pattern 是内角点数、整块板都在图里", pattern[0], pattern[1])
            } else {
                why
            });
        }
        let data = r.record("calib").cloned().ok_or("标定没有输出")?;
        let path = station_calib_path(&app)?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let doc = json!({"kind": "Record", "type": "image.PlaneCalib", "data": data, "ts": ly_plc::now_ms()});
        std::fs::write(&path, serde_json::to_string_pretty(&doc).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        calib_info(&path).ok_or_else(|| "标定文件写入后读不回来".into())
    })
    .await
    .map_err(|e| e.to_string())?
}
