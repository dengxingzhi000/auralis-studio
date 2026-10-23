/**
 * RealtimePanel — 实时采集控制与状态。
 *
 * ## 为什么 UI 只做「拉快照」
 *
 * 依据 docs/BENCHMARKS.md：48 kHz 回调预算约 20.8 µs，而 FFT 4096 需 271 µs。
 * 所以链路是：
 *
 * ```text
 * 音频回调 -> 环形缓冲 -> 工作线程(FFT) -> 最新快照槽 -> UI 每 ~33 ms 拉一次
 * ```
 *
 * UI **不做任何音频计算**，也不参与时基；它只是周期性地取一个已经算好的
 * 快照。丢帧/覆盖计数由后端给出，前端只负责**显示出来**。
 *
 * ## 丢帧必须可见
 *
 * `droppedFrames`（分析跟不上）和 `snapshotOverwrites`（UI 跟不上）
 * 都直接显示。用户必须能判断当前读数是否完整，而不是看到一条平滑的假曲线。
 */

import { useCallback, useEffect, useRef, useState } from "react";

import {
  captureStatus,
  listInputDevices,
  realtimeSnapshot,
  startRealtime,
  stopRealtime,
} from "@/lib/ipc";
import type { CaptureStatus, DeviceInfo, RealtimeSnapshot } from "@/lib/contracts";

export interface RealtimePanelProps {
  /**
   * 实时快照回调；`null` 表示采集已停止。
   *
   * 频谱是**已降采样**的小端 f32 数据，前端只画不解析语义。
   * 传整个对象而不是 5 个位置参数：少一次改签名就多一处漏改。
   */
  onSnapshot?: (snap: RealtimeSnapshot | null) => void;
  /**
   * 采集状态变化回调；`true` = 采集中。
   *
   * 导航项上的「采集中」标记靠它。**不能**用「有没有快照」来代替：
   * 采集中但输入静音时快照是空的，那样标记会一闪一闪地消失。
   */
  onStatusChange?: (running: boolean) => void;
}

/** 轮询周期。30 Hz 与显示器刷新和高频通道的占位目标一致。 */
const POLL_MS = 33;

export function RealtimePanel({ onSnapshot, onStatusChange }: RealtimePanelProps) {
  const [devices, setDevices] = useState<DeviceInfo[]>([]);
  const [deviceError, setDeviceError] = useState<string | null>(null);
  const [selected, setSelected] = useState("");
  const [status, setStatus] = useState<CaptureStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [devicesLoaded, setDevicesLoaded] = useState(false);

  // 回调放进 ref：否则父组件每次渲染都传入新的箭头函数，
  // effect 依赖就会变化 → 每帧清掉重建定时器，轮询永远不稳定。
  const snapshotRef = useRef(onSnapshot);
  useEffect(() => {
    snapshotRef.current = onSnapshot;
  }, [onSnapshot]);

  const statusRef = useRef(onStatusChange);
  useEffect(() => {
    statusRef.current = onStatusChange;
  }, [onStatusChange]);

  // 采集状态对外播报。只在「运行中与否」变化时触发，避免每 33 ms
  // 的轮询都把父组件叫醒一次。
  const running = status?.running === true;
  useEffect(() => {
    statusRef.current?.(running);
  }, [running]);

  // 挂载：枚举设备，并接手可能已经在跑的会话（侧栏隐藏再显示不该中断采集）。
  useEffect(() => {
    let alive = true;
    listInputDevices()
      .then((d) => {
        if (!alive) return;
        setDevices(d);
        setSelected((s) => s || d[0]?.id || "");
      })
      .catch((e) => {
        if (alive) setDeviceError(String(e));
      })
      .finally(() => {
        if (alive) setDevicesLoaded(true);
      });
    captureStatus()
      .then((s) => {
        if (alive && s.running) setStatus(s);
      })
      .catch(() => undefined);
    return () => {
      alive = false;
    };
  }, []);

  // 只在采集中轮询。依赖里只有 `running`，定时器不会因为重渲染而重建。
  useEffect(() => {
    if (!running) return;
    let alive = true;
    let ticks = 0;

    const id = window.setInterval(() => {
      realtimeSnapshot()
        .then((s) => {
          if (!alive) return;
          snapshotRef.current?.(
            s.spectrumDb.length > 0 ? s : null,
          );
          // 计数直接来自快照，省掉每帧第二次 IPC。
          setStatus((prev) =>
            prev === null
              ? prev
              : {
                  ...prev,
                  processedFrames: s.processedFrames,
                  droppedFrames: s.droppedFrames,
                  snapshotOverwrites: s.snapshotOverwrites,
                },
          );
          // 每 ~10 帧核对一次会话状态，用于发现被外部停掉的情况。
          ticks += 1;
          if (ticks % 10 === 0) {
            captureStatus()
              .then((st) => {
                if (alive) setStatus(st);
              })
              .catch(() => undefined);
          }
        })
        .catch(() => undefined);
    }, POLL_MS);

    return () => {
      alive = false;
      window.clearInterval(id);
    };
  }, [running]);

  const start = useCallback(async () => {
    setBusy(true);
    setError(null);
    try {
      setStatus(await startRealtime(selected));
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, [selected]);

  const stop = useCallback(async () => {
    setBusy(true);
    try {
      setStatus(await stopRealtime());
      snapshotRef.current?.(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  }, []);

  return (
    <div className="rt-panel" id="realtime-panel">
      <div className="rt-head">
        <span className="label">实时输入</span>
        {running && <span className="rt-live-dot" title="采集中" />}
      </div>

      {!devicesLoaded ? (
        <div className="muted rt-note">正在枚举输入设备…</div>
      ) : devices.length === 0 ? (
        <div className="muted rt-note">
          {deviceError
            ? `无法枚举输入设备：${deviceError}`
            : "未发现输入设备（无声卡环境属正常）"}
        </div>
      ) : (
        <>
          <select
            className="rt-select"
            value={selected}
            onChange={(e) => setSelected(e.target.value)}
            disabled={running || busy}
            aria-label="输入设备"
          >
            {devices.map((d) => (
              <option key={d.id} value={d.id}>
                {d.name}
                {d.inputChannels > 0 ? ` · ${d.inputChannels}ch` : ""}
              </option>
            ))}
          </select>

          <button
            className={running ? "btn wide" : "btn primary wide"}
            onClick={running ? stop : start}
            disabled={busy || (!running && !selected)}
          >
            {busy ? "处理中…" : running ? "停止采集" : "开始采集"}
          </button>
        </>
      )}

      {error && <div className="rt-error">{error}</div>}

      {running && status && (
        <div className="rt-stats">
          <div className="kv">
            <span className="muted">采样率</span>
            <span className="num">{status.sampleRate.toLocaleString("en-US")} Hz</span>
          </div>
          <div className="kv">
            <span className="muted">声道</span>
            <span className="num">{status.channels}</span>
          </div>
          <div className="kv">
            <span className="muted">已处理</span>
            <span className="num">
              {status.processedFrames.toLocaleString("en-US")}
            </span>
          </div>
          {/* 丢帧必须可见：分析跟不上 */}
          <div className="kv">
            <span className="muted">分析丢帧</span>
            <span className={status.droppedFrames > 0 ? "num bad" : "num"}>
              {status.droppedFrames.toLocaleString("en-US")}
            </span>
          </div>
          {/* UI 跟不上：快照被覆盖 */}
          <div className="kv">
            <span className="muted">快照覆盖</span>
            <span className={status.snapshotOverwrites > 0 ? "num warn" : "num"}>
              {status.snapshotOverwrites.toLocaleString("en-US")}
            </span>
          </div>
        </div>
      )}

      {running && status && status.droppedFrames > 0 && (
        <div className="rt-note warn-note">
          分析线程跟不上采集速率，已丢弃 {status.droppedFrames} 帧样本；
          当前频谱不连续。
        </div>
      )}
    </div>
  );
}
