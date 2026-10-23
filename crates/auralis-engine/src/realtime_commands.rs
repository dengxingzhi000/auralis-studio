//! 实时采集相关的 Tauri 命令。
//!
//! 职责划分（`docs/ADR.md` ADR-016 / ADR-018）：
//! - 音频回调（`auralis-audio`）只做拷贝与计数
//! - 工作线程（`auralis-audio::realtime`）做全部 DSP
//! - 本层只负责编排与把快照送到前端
//!
//! 前端**不参与音频计算**，只是每 20–30 Hz 拉一次最新快照。

use std::sync::Arc;

use auralis_audio::{
    CaptureConfig, CaptureSession, DeviceError, RealtimeConfig, RealtimeWorker, SampleRing,
    list_input_devices, start_capture,
};
use serde::Serialize;

/// 设备信息 DTO。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfoDto {
    /// 稳定标识，传给 `start_capture`。
    pub id: String,
    /// 显示名。
    pub name: String,
    /// 设备声明支持的输入采样率；为空表示未申报。
    pub supported_sample_rates: Vec<u32>,
    /// 输入声道数。
    pub input_channels: u16,
}

/// 采集状态 DTO。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureStatusDto {
    /// 是否正在采集。
    pub running: bool,
    /// 采样率。
    pub sample_rate: u32,
    /// 声道数。
    pub channels: u16,
    /// 环形缓冲丢弃的帧数。
    pub dropped_frames: u64,
    /// 快照被覆盖次数（UI 跟不上）。
    pub snapshot_overwrites: u64,
    /// 工作线程已处理的帧数。
    pub processed_frames: u64,
}

/// 实时快照 DTO。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RealtimeSnapshotDto {
    /// 频谱 dB，**小端 f32 字节流**。
    ///
    /// ADR-018 规则 2：二进制优先。1024 个点走 JSON 数组是每秒几十 KB 的
    /// 无谓开销，而且 JSON 数字还要在前端再解析一遍。
    #[serde(serialize_with = "crate::b64::encode")]
    pub spectrum_db: Vec<u8>,
    /// 频率间隔 Hz。
    pub bin_hz: f64,
    /// 采样率 Hz。前端画频率轴要用它算奈奎斯特，不能沿用一个文件的采样率。
    pub sample_rate: u32,
    /// 峰值 dBFS；`null` = 静音。
    pub peak_dbfs: Option<f32>,
    /// RMS dBFS；`null` = 静音。
    pub rms_dbfs: Option<f32>,
    /// 已处理帧数。
    pub processed_frames: u64,
    /// 环形缓冲丢帧。
    pub dropped_frames: u64,
    /// 快照覆盖次数。
    pub snapshot_overwrites: u64,
}

/// 列出输入设备。
///
/// # Errors
/// 音频后端不可用。
pub fn input_devices() -> Result<Vec<DeviceInfoDto>, String> {
    let devices = list_input_devices().map_err(|e| e.to_string())?;
    Ok(devices
        .into_iter()
        .map(|d| DeviceInfoDto {
            id: d.id,
            name: d.name,
            supported_sample_rates: d.supported_sample_rates,
            input_channels: d.input_channels,
        })
        .collect())
}

/// 实时会话的内部状态。
struct RealtimeInner {
    session: Option<CaptureSession>,
    worker: Option<RealtimeWorker>,
    config: CaptureConfig,
}

/// 实时会话的持有者。
///
/// 锁只保护**会话的启停**，不保护音频数据流：
/// 音频回调与工作线程都只碰 `SampleRing`（无锁 SPSC），
/// 不碰这把锁。因此 UI 读快照时不会阻塞采集。
pub struct RealtimeSession {
    inner: std::sync::Mutex<RealtimeInner>,
}

impl RealtimeSession {
    /// 新建（未启动）。
    #[must_use]
    pub fn new() -> Self {
        Self {
            inner: std::sync::Mutex::new(RealtimeInner {
                session: None,
                worker: None,
                config: CaptureConfig::default(),
            }),
        }
    }
}

impl Default for RealtimeSession {
    fn default() -> Self {
        Self::new()
    }
}

/// 启动实时采集。
///
/// # Errors
/// 设备不存在、格式不支持、流无法建立、工作线程无法启动。
pub fn start_realtime(
    realtime: &RealtimeSession,
    device_id: String,
    sample_rate: Option<u32>,
    channels: Option<u16>,
) -> Result<CaptureStatusDto, String> {
    // 先把旧会话摘出来，**不要**持锁做停止与设备打开：
    // 停止会 join 分析线程，打开设备要走 cpal 后端（几十到几百毫秒）。
    // 持锁期间，快通道的快照轮询（20–30 Hz）会被整段卡住，
    // 界面上的实时频谱会僵住不动。
    let previous = {
        let mut rt = realtime.inner.lock().map_err(|e| e.to_string())?;
        (rt.worker.take(), rt.session.take())
    };
    stop_previous(previous.0, previous.1);

    let config = CaptureConfig {
        sample_rate: sample_rate.unwrap_or(48_000),
        channels: channels.unwrap_or(2),
        buffer_frames: 256,
    };

    let session = start_capture(&device_id, config, 4096).map_err(|e| match e {
        DeviceError::BackendUnavailable(m) => {
            format!("本机没有可用的音频输入后端：{m}")
        }
        DeviceError::NotFound(id) => format!("找不到输入设备：{id}"),
        DeviceError::UnsupportedSampleRate {
            requested,
            available,
        } => format!(
            "设备不支持 {requested} Hz（可用：{}）。请在检查器里改采样率。",
            available
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => other.to_string(),
    })?;

    // 关键：用**设备实际开的格式**，不是用户请求的格式。
    // 设备不支持请求值时 start_capture 会回退到默认值，若这里仍用
    // 请求值去喂分析线程，频率轴会整体偏移而不报任何错。
    let actual = session.format();
    let ring: Arc<SampleRing<_>> = session.ring().clone();
    let recycler = session.recycler();

    let worker = RealtimeWorker::spawn(
        ring,
        recycler,
        actual.sample_rate,
        RealtimeConfig {
            channels: actual.channels,
            ..RealtimeConfig::default()
        },
    )
    .map_err(|e| format!("无法启动实时分析线程：{e}"))?;

    let mut rt = realtime.inner.lock().map_err(|e| e.to_string())?;
    rt.config = CaptureConfig {
        sample_rate: actual.sample_rate,
        channels: actual.channels,
        buffer_frames: config.buffer_frames,
    };
    rt.session = Some(session);
    rt.worker = Some(worker);

    tracing::info!(
        device_id = %device_id,
        sample_rate = actual.sample_rate,
        channels = actual.channels,
        "realtime capture started"
    );
    Ok(capture_status(&rt))
}

/// 停止实时采集。
///
/// # Errors
/// 内部锁中毒。
pub fn stop_realtime(realtime: &RealtimeSession) -> Result<CaptureStatusDto, String> {
    // 与 `start_realtime` 同一个原则：停止要 join 分析线程、放掉 cpal 流，
    // 这些都**不**在锁内做。锁只用来把会话从状态里摘出去。
    let previous = {
        let mut rt = realtime.inner.lock().map_err(|e| e.to_string())?;
        (rt.worker.take(), rt.session.take())
    };
    stop_previous(previous.0, previous.1);

    tracing::info!("realtime capture stopped");
    let rt = realtime.inner.lock().map_err(|e| e.to_string())?;
    Ok(capture_status(&rt))
}

/// 收尾一个旧会话：先停分析线程，再释放采集会话。
///
/// 顺序有讲究：分析线程还在消费环形缓冲时丢掉会话会关闭缓冲；
/// 而丢掉分析线程（它的 `Drop` 会 join 工作线程）会先把消费端停掉。
fn stop_previous(mut worker: Option<RealtimeWorker>, session: Option<CaptureSession>) {
    if let Some(mut w) = worker.take() {
        w.stop();
    }
    // `CaptureSession` 的 `Drop` 会关闭环形缓冲并释放 cpal 流。
    drop(session);
}

/// 采集状态（丢帧等）。
///
/// # Errors
/// 内部锁中毒。
pub fn capture_status_cmd(realtime: &RealtimeSession) -> Result<CaptureStatusDto, String> {
    let rt = realtime.inner.lock().map_err(|e| e.to_string())?;
    Ok(capture_status(&rt))
}

/// 取最新实时快照。
///
/// # Errors
/// 内部锁中毒。
pub fn realtime_snapshot(realtime: &RealtimeSession) -> Result<RealtimeSnapshotDto, String> {
    let rt = realtime.inner.lock().map_err(|e| e.to_string())?;
    let Some(worker) = rt.worker.as_ref() else {
        return Ok(RealtimeSnapshotDto {
            spectrum_db: Vec::new(),
            bin_hz: 1.0,
            sample_rate: rt.config.sample_rate,
            peak_dbfs: None,
            rms_dbfs: None,
            processed_frames: 0,
            dropped_frames: 0,
            snapshot_overwrites: 0,
        });
    };
    let s = worker.latest();
    Ok(RealtimeSnapshotDto {
        spectrum_db: f32s_to_le_bytes(&s.spectrum_db),
        bin_hz: s.bin_hz,
        sample_rate: s.sample_rate,
        peak_dbfs: s.peak_dbfs,
        rms_dbfs: s.rms_dbfs,
        processed_frames: s.processed_frames,
        dropped_frames: s.dropped_frames,
        snapshot_overwrites: s.snapshot_overwrites,
    })
}

/// `&[f32]` -> 小端字节流。前端用 `new Float32Array(buf)` 还原。
fn f32s_to_le_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

fn capture_status(rt: &RealtimeInner) -> CaptureStatusDto {
    let (dropped, overwrites, processed) = match (rt.session.as_ref(), rt.worker.as_ref()) {
        (Some(s), Some(w)) => {
            let st = s.stats();
            let snap = w.latest();
            (
                st.dropped_frames,
                snap.snapshot_overwrites,
                snap.processed_frames,
            )
        }
        (None, None) => (0, 0, 0),
        (Some(s), None) => (s.stats().dropped_frames, 0, 0),
        (None, Some(w)) => {
            let snap = w.latest();
            (0, snap.snapshot_overwrites, snap.processed_frames)
        }
    };
    CaptureStatusDto {
        running: rt.session.is_some(),
        sample_rate: rt.config.sample_rate,
        channels: rt.config.channels,
        dropped_frames: dropped,
        snapshot_overwrites: overwrites,
        processed_frames: processed,
    }
}
