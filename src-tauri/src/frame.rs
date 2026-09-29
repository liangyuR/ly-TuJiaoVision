//! 相机帧与整帧像素缓冲池。三路相机连续采集时每帧都新分配 1–5 MB 会让内存抖动，
//! 缓冲用完（最后一个 Arc 释放）就回到池里给下一帧用。

use std::sync::{Arc, Mutex, Weak};

use serde::Serialize;

/// 一帧 8 位灰度图，行主序、无行填充。
pub struct FrameImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pool: Option<Weak<PoolInner>>,
}

impl FrameImage {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Self {
        Self { width, height, pixels, pool: None }
    }

    /// 双线性取值；超出图像返回 None。
    pub fn sample(&self, x: f32, y: f32) -> Option<f32> {
        let (w, h) = (self.width as usize, self.height as usize);
        if !(x >= 0.0 && y >= 0.0 && x <= (w - 1) as f32 && y <= (h - 1) as f32) {
            return None;
        }
        let (x0, y0) = (x.floor() as usize, y.floor() as usize);
        let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
        let (fx, fy) = (x - x0 as f32, y - y0 as f32);
        let p = |x: usize, y: usize| self.pixels[y * w + x] as f32;
        let top = p(x0, y0) * (1.0 - fx) + p(x1, y0) * fx;
        let bottom = p(x0, y1) * (1.0 - fx) + p(x1, y1) * fx;
        Some(top * (1.0 - fy) + bottom * fy)
    }
}

impl std::fmt::Debug for FrameImage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FrameImage({}×{})", self.width, self.height)
    }
}

impl Drop for FrameImage {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.take().and_then(|p| p.upgrade()) {
            pool.put(std::mem::take(&mut self.pixels));
        }
    }
}

struct PoolInner {
    free: Mutex<Vec<Vec<u8>>>,
    keep: usize,
}

impl PoolInner {
    fn put(&self, buf: Vec<u8>) {
        let mut free = self.free.lock().unwrap();
        if free.len() < self.keep {
            free.push(buf);
        }
    }
}

#[derive(Clone)]
pub struct FramePool(Arc<PoolInner>);

impl FramePool {
    /// `keep`：池里最多留几块空闲缓冲。
    pub fn new(keep: usize) -> Self {
        Self(Arc::new(PoolInner { free: Mutex::new(Vec::new()), keep }))
    }

    /// 拷一份像素到池里的缓冲。
    pub fn copy(&self, width: u32, height: u32, src: &[u8]) -> FrameImage {
        let mut buf = self.0.free.lock().unwrap().pop().unwrap_or_default();
        buf.clear();
        buf.extend_from_slice(src);
        FrameImage { width, height, pixels: buf, pool: Some(Arc::downgrade(&self.0)) }
    }

    /// 把现成的图像（模拟合成、回放解码出来的）挂到池上，用完后缓冲回收。
    pub fn adopt(&self, mut img: FrameImage) -> FrameImage {
        img.pool = Some(Arc::downgrade(&self.0));
        img
    }
}

/// 一帧图像的元数据。计数器取自相机 Chunk（帧计数、Line0 触发计数），未开启 Chunk 时退化为 SDK 帧号。
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Frame {
    /// 相机组里的序号（从 0 开始）
    pub cam: u8,
    pub frame_counter: u64,
    pub trigger_counter: u64,
    pub lost_packets: u32,
    pub ts: i64,
    /// 整帧 Mono8 像素。需要图像测量或帧录制时才带上。
    #[serde(skip)]
    pub image: Option<Arc<FrameImage>>,
}
