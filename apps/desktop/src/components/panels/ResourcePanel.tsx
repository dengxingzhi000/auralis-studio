/**
 * ResourcePanel —— 左侧资源栏。
 *
 * 纯展示 + 回调：文件列表、分析视图导航、实时输入、后台任务队列。
 * 所有状态与 IPC 都在 `App` 里，这里不读存储、不发命令。
 *
 * `analyzing` 只表示**分析**正在进行：导出是独立操作，不应让
 * 「打开音频文件」显示成「分析中…」，也不应禁用文件列表。
 */

import { LevelMeter } from "@/components/audio-viz/LevelMeter";
import { RealtimePanel } from "@/components/audio-viz/RealtimePanel";
import { TaskQueuePanel } from "@/components/audio-viz/TaskQueuePanel";
import {
  IconDevice,
  IconFilePlus,
  IconGauge,
  IconHelp,
  IconMore,
  IconSearch,
  IconSettings,
  IconSpectrogram,
  IconSpectrum,
  IconTasks,
  IconWaveform,
} from "@/components/ui/icons";
import { IconButton, NavGroup, NavItem } from "@/components/ui/primitives";
import type { FileInfo, RealtimeSnapshot, TaskInfo } from "@/lib/contracts";
import { scrollToPane } from "@/lib/scroll";
import type { HistoryEntry } from "@/lib/task-history";

/** 侧栏电平表使用的固定量程。 */
const MIN_DB = -140;
const MAX_DB = 0;

export interface ResourcePanelProps {
  files: FileInfo[];
  selected: FileInfo | null;
  onPick: () => void;
  /** 点击历史文件重新分析，不必再过一遍文件对话框 */
  onSelect: (path: string) => void;
  /** 分析（文件或合成信号）进行中；导出不置位 */
  analyzing: boolean;
  version: string;
  /** 实时 Peak dBFS；null = 未采集或静音 */
  livePeak: number | null;
  /** 实时 RMS dBFS；null = 未采集或静音 */
  liveRms: number | null;
  /** 实时快照回调（频谱 / 电平） */
  onLiveSnapshot: (snap: RealtimeSnapshot | null) => void;
  /** 运行中的后台任务 */
  tasks: TaskInfo[];
  /** 最近完成的文件分析 */
  history: HistoryEntry[];
  /** 取消一个运行中的任务 */
  onCancelTask: (taskId: number) => void;
  /** 实时采集是否在运行 */
  liveRunning: boolean;
  /** 实时采集状态变化回调 */
  onLiveStatusChange: (running: boolean) => void;
  /** 打开全局搜索 */
  onSearch: () => void;
  /** 打开偏好设置 */
  onOpenPrefs: () => void;
  /** 打开帮助 */
  onOpenHelp: () => void;
  /** 打开关于 */
  onOpenAbout: () => void;
}

export function ResourcePanel({
  files,
  selected,
  onPick,
  onSelect,
  analyzing,
  version,
  livePeak,
  liveRms,
  onLiveSnapshot,
  tasks,
  history,
  onCancelTask: onCancelQueueTask,
  liveRunning,
  onLiveStatusChange,
  onSearch,
  onOpenPrefs,
  onOpenHelp,
  onOpenAbout,
}: ResourcePanelProps) {
  return (
    <>
      <div className="panel-body">
        <button className="nav-cta" onClick={onPick} disabled={analyzing}>
          <IconFilePlus size={15} />
          {analyzing ? "分析中…" : "打开音频文件…"}
        </button>

        <NavGroup label="文件">
          {files.length === 0 ? (
            <li>
              <div className="empty" style={{ padding: "10px 4px" }}>
                <span className="empty-hint">
                  还没有文件。可打开 WAV / FLAC / MP3 / AIFF / OGG；
                  WAV / FLAC 已用真实样本验证，其余容器会在名字旁标注。
                </span>
              </div>
            </li>
          ) : (
            files.map((f) => (
              <li key={f.path}>
                <button
                  type="button"
                  className={
                    "file-item" + (selected?.path === f.path ? " on" : "")
                  }
                  title={`${f.path}\n点击重新分析`}
                  disabled={analyzing}
                  onClick={() => onSelect(f.path)}
                >
                  <span className="file-name">{f.fileName}</span>
                  <span className="file-meta num">
                    <span className="meta-text">
                      {f.containerName} ·{" "}
                      {f.sampleRate.toLocaleString("en-US")} Hz ·{" "}
                      {f.channels === 1 ? "单声道" : `${f.channels} ch`}
                    </span>
                    {!f.verified && (
                      <span className="file-warn" title={f.verificationNote}>
                        未验证
                      </span>
                    )}
                  </span>
                </button>
              </li>
            ))
          )}
        </NavGroup>

        <NavGroup label="分析视图">
          <NavItem
            icon={<IconWaveform />}
            active
            onClick={() => scrollToPane("pane-waveform")}
          >
            波形
          </NavItem>
          <NavItem
            icon={<IconSpectrum />}
            active
            onClick={() => scrollToPane("pane-spectrum")}
          >
            频谱
          </NavItem>
          <NavItem
            icon={<IconSpectrogram />}
            onClick={() => scrollToPane("pane-spectrogram")}
            title="声谱图：Shift + 滚轮缩放频率，Shift + 拖拽框选频段"
          >
            声谱图缩放
          </NavItem>
          <NavItem
            icon={<IconGauge />}
            onClick={() => scrollToPane("pane-metrics")}
            title="电平、信号指标与数据质量"
          >
            电平表 / 指标
          </NavItem>
          <NavItem
            icon={<IconTasks />}
            onClick={() => scrollToPane("task-queue-panel")}
            title="运行中的任务与最近完成的分析"
            tag={tasks.length > 0 ? String(tasks.length) : undefined}
          >
            后台任务队列
          </NavItem>
          <NavItem
            icon={<IconDevice />}
            onClick={() => scrollToPane("realtime-panel")}
            title="枚举输入设备并启动实时采集"
            tag={liveRunning ? "采集中" : undefined}
          >
            实时输入
          </NavItem>
        </NavGroup>

        {/* M4：实时采集。放在导航之外，因为它是一个真正的控制面板，
            不是切换视图的入口。 */}
        <div className="nav-divider" />
        <RealtimePanel
          onSnapshot={onLiveSnapshot}
          onStatusChange={onLiveStatusChange}
        />
        {livePeak !== null && (
          <LevelMeter
            peakDbfs={livePeak}
            rmsDbfs={liveRms}
            peakPerChannel={[]}
            rmsPerChannel={[]}
            minDb={MIN_DB}
            maxDb={MAX_DB}
            overFullScale={0}
            clippingRegions={0}
          />
        )}

        {/* M3：后台任务队列。与实时面板一样是控制面板，不是视图入口。 */}
        <div className="nav-divider" />
        <div id="task-queue-panel">
          <TaskQueuePanel
            running={tasks}
            history={history}
            onCancel={onCancelQueueTask}
          />
        </div>

        <NavGroup label="工程">
          <NavItem
            icon={<IconSettings />}
            onClick={onOpenPrefs}
            title="显示、布局与分析参数（Ctrl+,）"
          >
            偏好设置
          </NavItem>
        </NavGroup>
      </div>

      <div className="sidebar-foot">
        <ul className="nav">
          <NavItem
            icon={<IconSearch />}
            onClick={onSearch}
            title="搜索文件、峰值或操作（Ctrl+K）"
          >
            搜索
          </NavItem>
          <NavItem
            icon={<IconHelp />}
            onClick={onOpenHelp}
            title="快捷键、界面导览与能力边界（F1）"
          >
            帮助
          </NavItem>
        </ul>
        <div className="sidebar-user">
          <span className="avatar">AS</span>
          <span className="who">
            <b>Auralis Studio</b>
            <span className="num">{version} · 本地分析</span>
          </span>
          <span className="spacer" />
          <IconButton label="关于与版本信息" onClick={onOpenAbout}>
            <IconMore size={15} />
          </IconButton>
        </div>
      </div>
    </>
  );
}
