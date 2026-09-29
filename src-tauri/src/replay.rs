//! 回放相机：从目录读图当作相机帧交付。认两种命名：
//! 本程序的帧录制 `cam{通道}_{序号}.pgm`，以及海康演示图 `Frame{序号}_{通道}.jpg`；其余图片按自然顺序、不分通道。

use std::path::{Path, PathBuf};

use crate::vision::FrameImage;

const EXTS: [&str; 6] = ["pgm", "jpg", "jpeg", "png", "bmp", "tif"];

struct Entry {
    channel: Option<u32>,
    seq: u64,
    name: String,
    path: PathBuf,
}

fn digits(s: &str) -> Option<u64> {
    (!s.is_empty() && s.bytes().all(|b| b.is_ascii_digit())).then(|| s.parse().ok()).flatten()
}

/// 文件名（不含扩展名）→ (通道, 序号)。通道从 1 开始。
fn parse(stem: &str) -> Option<(u32, u64)> {
    let s = stem.to_ascii_lowercase();
    if let Some(rest) = s.strip_prefix("cam") {
        let (c, n) = rest.split_once('_')?;
        return Some((digits(c)? as u32, digits(n)?));
    }
    if let Some(rest) = s.strip_prefix("frame") {
        let (n, c) = rest.split_once('_')?;
        return Some((digits(c)? as u32, digits(n)?));
    }
    None
}

/// 列出目录里属于某通道（从 1 开始；0 表示不分通道）的帧，按序号排好。
pub fn scan(dir: &Path, channel: u32) -> Result<Vec<PathBuf>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("读不了回放目录 {}：{e}", dir.display()))?;
    let mut entries = Vec::new();
    for e in rd.flatten() {
        let path = e.path();
        let Some(ext) = path.extension().and_then(|x| x.to_str()).map(str::to_ascii_lowercase) else { continue };
        if !EXTS.contains(&ext.as_str()) {
            continue;
        }
        let stem = path.file_stem().and_then(|x| x.to_str()).unwrap_or_default().to_string();
        let (channel, seq) = match parse(&stem) {
            Some((c, n)) => (Some(c), n),
            None => (None, stem.bytes().filter(u8::is_ascii_digit).fold(0u64, |a, b| a.saturating_mul(10).saturating_add((b - b'0') as u64))),
        };
        entries.push(Entry { channel, seq, name: stem, path });
    }
    let named = entries.iter().any(|e| e.channel.is_some());
    if named && channel > 0 {
        entries.retain(|e| e.channel == Some(channel));
    } else if named {
        let first = entries.iter().filter_map(|e| e.channel).min();
        entries.retain(|e| e.channel == first);
    }
    entries.sort_by(|a, b| a.seq.cmp(&b.seq).then_with(|| a.name.cmp(&b.name)));
    if entries.is_empty() {
        return Err(if channel > 0 { format!("回放目录里没有通道 {channel} 的图片") } else { "回放目录里没有图片".into() });
    }
    Ok(entries.into_iter().map(|e| e.path).collect())
}

/// 帧录制目录（有 part.json）里这些帧相对工件开始的时刻（ms），与 files 一一对应；缺了任何一帧就不按时刻回放。
pub fn timeline(dir: &Path, files: &[PathBuf]) -> Option<Vec<i64>> {
    let meta: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(dir.join("part.json")).ok()?).ok()?;
    let started = meta.get("startedTs")?.as_i64()?;
    let frames = meta.get("frames")?.as_array()?;
    let ts_of = |name: &str| frames.iter().find(|f| f.get("file").and_then(|v| v.as_str()) == Some(name)).and_then(|f| f.get("ts")?.as_i64());
    files.iter().map(|p| ts_of(p.file_name()?.to_str()?).map(|ts| ts - started)).collect()
}

/// 读成 8 位灰度。彩色 / 16 位图按亮度换算。
pub fn load(path: &Path) -> Result<FrameImage, String> {
    let img = image::open(path).map_err(|e| format!("解码 {} 失败：{e}", path.display()))?.into_luma8();
    let (width, height) = img.dimensions();
    Ok(FrameImage::new(width, height, img.into_raw()))
}

/// 存成 PGM（P5）。帧录制用，不压缩、写得快。
pub fn save_pgm(path: &Path, img: &FrameImage) -> Result<(), String> {
    use std::io::Write;
    let mut f = std::io::BufWriter::new(std::fs::File::create(path).map_err(|e| e.to_string())?);
    write!(f, "P5\n{} {}\n255\n", img.width, img.height).map_err(|e| e.to_string())?;
    f.write_all(&img.pixels).map_err(|e| e.to_string())?;
    f.flush().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn names() {
        assert_eq!(parse("Frame12_3"), Some((3, 12)));
        assert_eq!(parse("cam2_000041"), Some((2, 41)));
        assert_eq!(parse("IMG_0001"), None);
    }
}
