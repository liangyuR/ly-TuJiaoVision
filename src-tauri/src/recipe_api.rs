//! 配方页用的命令：列表、编辑、预览、保存（内容变了版本号 +1）、删除、导入胶路点。

use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::cycle::{CycleHost, Input, Phase, RecipeSummary};
use crate::recipe::{self, default_follow_spec, InspectMode, Recipe, RecipeDoc};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecipeListing {
    pub recipes: Vec<RecipeSummary>,
    /// 读不了的配方文件
    pub errors: Vec<String>,
}

#[tauri::command]
pub fn recipe_list(cycle: State<'_, CycleHost>) -> RecipeListing {
    RecipeListing { recipes: cycle.recipes.list().iter().map(|r| RecipeSummary::from(&**r)).collect(), errors: cycle.recipes.errors() }
}

#[tauri::command]
pub fn recipe_doc(cycle: State<'_, CycleHost>, id: String) -> Result<RecipeDoc, String> {
    cycle.recipes.doc(&id).ok_or_else(|| format!("配方不存在：{id}"))
}

/// 按编辑中的内容生成配方但不保存，配方页画预览用。
#[tauri::command]
pub fn recipe_preview(doc: RecipeDoc) -> Result<Recipe, String> {
    doc.build()
}

/// 新建配方的起点：复制现有配方，或按工况给一份默认值。
#[tauri::command]
pub fn recipe_template(cycle: State<'_, CycleHost>, mode: InspectMode) -> RecipeDoc {
    let samples = recipe::samples();
    let mut doc = samples.into_iter().find(|d| d.mode == mode).expect("样例覆盖两种工况");
    let used: Vec<u16> = cycle.recipes.list().iter().map(|r| r.product_code).collect();
    doc.product_code = (1..u16::MAX).find(|c| !used.contains(c)).unwrap_or(0);
    doc.id = format!("NEW-{}", doc.product_code);
    doc.name = "新配方".into();
    doc.version = 1;
    if mode == InspectMode::Follow {
        doc.follow = Some(default_follow_spec((0..cycle.camera.len().min(3) as u8).collect(), 80.0));
    }
    doc
}

#[tauri::command]
pub fn recipe_save(cycle: State<'_, CycleHost>, doc: RecipeDoc, original_id: Option<String>) -> Result<RecipeSummary, String> {
    let saved: Arc<Recipe> = cycle.recipes.save(doc, original_id.as_deref())?;
    // 改了编号的配方正被人工选中时，跟着改过去
    if let Some(old) = original_id.filter(|o| *o != saved.id) {
        let mut settings = cycle.settings();
        if settings.manual_recipe_id.as_deref() == Some(old.as_str()) {
            settings.manual_recipe_id = Some(saved.id.clone());
            cycle.save_settings(settings)?;
        }
    }
    let _ = cycle.tx.send(Input::Refresh);
    Ok(RecipeSummary::from(&*saved))
}

#[tauri::command]
pub fn recipe_delete(cycle: State<'_, CycleHost>, id: String) -> Result<(), String> {
    let settings = cycle.settings();
    if settings.manual_recipe_id.as_deref() == Some(id.as_str()) && !matches!(cycle.phase(), Phase::Idle | Phase::Fault) {
        return Err("该配方正在检测中，工件结束后再删".into());
    }
    cycle.recipes.delete(&id)?;
    let _ = cycle.tx.send(Input::Refresh);
    Ok(())
}

/// 解析胶路点文件的内容（前端读文件后把文本传过来）。
#[tauri::command]
pub fn recipe_parse_points(text: String, file_name: String) -> Result<Vec<[f32; 2]>, String> {
    let ext = std::path::Path::new(&file_name).extension().and_then(|e| e.to_str()).unwrap_or("csv");
    recipe::parse_points(&text, ext)
}
