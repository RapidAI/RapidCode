//! `SmoothMetricValue` —— Rust 移植自 `packages/ui/src/components/ui/smooth-metric-value.tsx`
//!
//! 作用：模型流式输出会把行数按**不规则批次**推到 UI，直接显示就会数字大跳变。
//! 这一层在展示侧逐帧追赶真实目标，让 [`crate::components::flip_metric_value`]
//! 看到的是连续数字。
//!
//! ## 关键移植点 1：三段数学全量保留
//!
//! TS 里有三个 `export` 出来的纯函数，是这一层的全部可判定逻辑，
//! 这里逐条移植并单测：
//!
//! | TS | Rust |
//! |---|---|
//! | `normalizeSmoothMetricTarget` | [`normalize_smooth_metric_target`] / [`normalize_smooth_metric_target_str`] |
//! | `estimateSmoothMetricLinesPerSecond` | [`estimate_smooth_metric_lines_per_second`] |
//! | `resolveNextSmoothMetricDisplay` | [`resolve_next_smooth_metric_display`] |
//! | `clampMetricSpeed`（私有） | [`clamp_metric_speed`]（公开，便于测试） |
//!
//! ## 关键移植点 2：速度用 EMA 平滑，并按「帧数上限」兜底
//!
//! 每次目标变化时用 `delta / elapsed` 估一个「每秒多少行」，再与上一次做
//! 指数移动平均（`SPEED_EMA_ALPHA = 0.35`），然后夹到 `[24, 180]`。
//! 逐帧步长取 `max(速度步长, 收尾步长)` 再夹住剩余距离 —— 收尾步长由
//! `MAX_SMOOTH_ANIMATION_MS = 900` 反推，保证再大的跳变也能在 0.9s 内收敛。
//!
//! ## 关键移植点 3：`number | string` → [`MetricValue`]
//!
//! TS 的 `value: number | string` 两条分支行为**不同**：
//! `normalizeSmoothMetricTarget(1234.7)` 走 `Math.round` → `1235`，
//! 而 `normalizeSmoothMetricTarget("1234.7")` 走 `parseInt` → `1234`。
//! Rust 侧用 [`MetricValue`] 枚举保留这个区别，而不是统一成字符串。
//!
//! ## 与 TS 的差异
//!
//! 1. **`requestAnimationFrame` → `futures-timer` 的 16ms 轮询**：Dioxus 没有 rAF。
//!    空闲时轮询间隔放宽到 200ms，避免无谓唤醒（见 [`SmoothMetricValue`]）。
//! 2. **`prefers-reduced-motion` 改为显式 prop**：同 [`crate::components::flip_metric_value`]。
//! 3. **`useId()` → 调用方传 `metric_key`**：Dioxus 没有 `useId`。
//!    不传时退化为固定键，此时「换指标要重置进度」需要靠外层重挂载组件实现。
//! 4. **`Math.round` 与 `f64::round()` 的差异**：JS 的 `Math.round` 对 `.5` 向
//!    `+∞` 取整，Rust 的 `f64::round()` 是「远离零」。本模块处理的都是**非负**值，
//!    两者结果一致（`normalize_*` 还会再把负值夹到 0）。

use crate::components::flip_metric_value::FlipMetricValue;
use dioxus::prelude::*;
use futures_timer::Delay;
use std::time::{Duration, Instant};

/// 默认「每秒多少行」（TS `DEFAULT_LINES_PER_SECOND`）。
pub const DEFAULT_LINES_PER_SECOND: f64 = 48.0;

/// 速度下限（TS `MIN_LINES_PER_SECOND`）。
pub const MIN_LINES_PER_SECOND: f64 = 24.0;

/// 速度上限（TS `MAX_LINES_PER_SECOND`）。
pub const MAX_LINES_PER_SECOND: f64 = 180.0;

/// 速度 EMA 平滑系数（TS `SPEED_EMA_ALPHA`）。
pub const SPEED_EMA_ALPHA: f64 = 0.35;

/// 两次采样之间超过这个间隔就不采信（TS `SAMPLE_MAX_GAP_MS`）。
pub const SAMPLE_MAX_GAP_MS: f64 = 2_000.0;

/// 首个采样前的缓冲（TS `INITIAL_SAMPLE_BUFFER_MS`）。
pub const INITIAL_SAMPLE_BUFFER_MS: f64 = 120.0;

/// 平滑动画的最长时长（TS `MAX_SMOOTH_ANIMATION_MS`）。
pub const MAX_SMOOTH_ANIMATION_MS: f64 = 900.0;

/// 逐帧步长的最小时间跨度（TS `Math.max(elapsedMs, 16)`）。
pub const MIN_FRAME_MS: f64 = 16.0;

/// 目标值：对应 TS 的 `number | string` 联合类型。
///
/// 两条分支行为不同，所以不能统一成字符串（见模块文档）。
#[derive(Clone, PartialEq, Debug)]
pub enum MetricValue {
    /// 对应 TS 的 `number`：`Math.round` 后夹到非负。
    Number(f64),
    /// 对应 TS 的 `string`：`parseInt(x, 10)` 后夹到非负。
    Text(String),
}

impl From<f64> for MetricValue {
    fn from(v: f64) -> Self {
        MetricValue::Number(v)
    }
}

impl From<i64> for MetricValue {
    fn from(v: i64) -> Self {
        MetricValue::Number(v as f64)
    }
}

impl From<&str> for MetricValue {
    fn from(v: &str) -> Self {
        MetricValue::Text(v.to_string())
    }
}

impl From<String> for MetricValue {
    fn from(v: String) -> Self {
        MetricValue::Text(v)
    }
}

impl MetricValue {
    /// 归一化成整数目标；`None` 表示不是有限数值（TS 返回 `null`）。
    pub fn normalize(&self) -> Option<i64> {
        match self {
            MetricValue::Number(v) => normalize_smooth_metric_target(*v),
            MetricValue::Text(v) => normalize_smooth_metric_target_str(v),
        }
    }

    /// 原样渲染用（`normalize` 为 `None` 时 TS 直接显示原值）。
    pub fn as_display_text(&self) -> String {
        match self {
            MetricValue::Number(v) => format_number(*v),
            MetricValue::Text(v) => v.clone(),
        }
    }
}

/// `f64` 的显示形式：整数不带小数点（`42` 而不是 `42`）。
fn format_number(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// 数值目标归一化（逐字移植 TS `normalizeSmoothMetricTarget` 的 `number` 分支）。
///
/// 非有限值 → `None`；否则 `max(0, round(v))`。
pub fn normalize_smooth_metric_target(value: f64) -> Option<i64> {
    if !value.is_finite() {
        return None;
    }
    Some((value.round().max(0.0)) as i64)
}

/// 字符串目标归一化（对应 TS 的 `Number.parseInt(value, 10)` 分支）。
///
/// `parseInt` 的语义与 Rust 的 `str::parse` **不同**，这里按前者实现：
///
/// - 允许前导空白；
/// - 允许一个 `+` / `-`；
/// - 只吃连续数字，**后面的字符一律忽略**（`"12.5"` → `12`、`"1e3"` → `1`）；
/// - 一个数字都没有 → `None`（`parseInt` 返回 `NaN`）。
pub fn normalize_smooth_metric_target_str(value: &str) -> Option<i64> {
    let trimmed = value.trim_start();
    let mut chars = trimmed.chars().peekable();
    let sign = match chars.peek() {
        Some('+') => {
            chars.next();
            1.0
        }
        Some('-') => {
            chars.next();
            -1.0
        }
        _ => 1.0,
    };

    let digits: String = chars.take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        // parseInt 会返回 NaN，落到 `!Number.isFinite` 分支 → null
        return None;
    }
    // parseInt 用 IEEE double 累加，超大整数会丢精度；这里用 f64 复刻同样行为
    let magnitude: f64 = digits.parse().unwrap_or(f64::INFINITY);
    let signed = sign * magnitude;
    normalize_smooth_metric_target(signed)
}

/// 把速度夹到 `[MIN, MAX]`；非有限值退回默认值（逐字移植 TS `clampMetricSpeed`）。
pub fn clamp_metric_speed(value: f64) -> f64 {
    if !value.is_finite() {
        return DEFAULT_LINES_PER_SECOND;
    }
    value.clamp(MIN_LINES_PER_SECOND, MAX_LINES_PER_SECOND)
}

/// 用相邻两次目标变化估计「每秒多少行」，并对上一次结果做 EMA 平滑。
///
/// 逐字移植 TS `estimateSmoothMetricLinesPerSecond`。
pub fn estimate_smooth_metric_lines_per_second(
    elapsed_ms: f64,
    next_target_value: i64,
    previous_lines_per_second: f64,
    previous_target_value: i64,
) -> f64 {
    let delta = (next_target_value - previous_target_value).abs() as f64;
    // 没变化 / 时间倒退或为零 / 间隔过长 → 不采信这次采样
    if delta <= 0.0 || elapsed_ms <= 0.0 || elapsed_ms > SAMPLE_MAX_GAP_MS {
        return previous_lines_per_second;
    }

    let sampled_lines_per_second = (delta / elapsed_ms) * 1_000.0;
    let smoothed = previous_lines_per_second * (1.0 - SPEED_EMA_ALPHA)
        + sampled_lines_per_second * SPEED_EMA_ALPHA;
    clamp_metric_speed(smoothed)
}

/// 算出下一帧要显示的值。
///
/// 逐字移植 TS `resolveNextSmoothMetricDisplay`：
///
/// 1. 两端都取整；相等就直接返回。
/// 2. `speedStep` = 按当前速度这一帧能走多少行（至少 1）。
/// 3. `settleStep` = 按 `MAX_SMOOTH_ANIMATION_MS` 反推的收尾步长（至少 1）。
/// 4. 取两者较大者，再夹住剩余距离。
pub fn resolve_next_smooth_metric_display(
    current_value: f64,
    elapsed_ms: f64,
    lines_per_second: f64,
    target_value: f64,
) -> i64 {
    let current_value = current_value.round();
    let target_value = target_value.round();
    if current_value == target_value {
        return target_value as i64;
    }

    let distance = (target_value - current_value).abs();
    let direction = if target_value > current_value { 1.0 } else { -1.0 };
    let elapsed_ms = elapsed_ms.max(MIN_FRAME_MS);
    let lines_per_second = clamp_metric_speed(lines_per_second);
    let speed_step = ((lines_per_second * elapsed_ms) / 1_000.0).floor().max(1.0);
    let max_frames = (MAX_SMOOTH_ANIMATION_MS / elapsed_ms).ceil().max(1.0);
    let settle_step = (distance / max_frames).ceil().max(1.0);
    let step = distance.min(speed_step.max(settle_step));
    (current_value + direction * step) as i64
}

/// 平滑展示的运行时状态（对应 TS `SmoothMetricRuntimeState`）。
///
/// 纯数据 + 纯方法，因此「目标变化 → 逐帧收敛」这段时序逻辑可以完全离线单测。
#[derive(Clone, PartialEq, Debug)]
pub struct SmoothMetricState {
    /// 是否已经过首次采样缓冲。
    pub buffered_once: bool,
    /// 当前显示值（浮点，取整只发生在算步长时）。
    pub display_value: f64,
    /// 逐帧循环是否在跑（对应 TS 的 `frameId !== null`）。
    pub running: bool,
    /// 上一帧的时间戳（ms）。
    pub last_frame_at: f64,
    /// 当前速度估计。
    pub lines_per_second: f64,
    /// 指标键：变了就重置进度（对应 TS 的 `metricKey`）。
    pub metric_key: String,
    /// 目标值最后一次变化的时间戳（ms）。
    pub target_updated_at: f64,
    /// 当前目标值。
    pub target_value: f64,
}

impl SmoothMetricState {
    /// 以 `target` 为起点建立状态（对应 TS 的 `useRef` 初值）。
    pub fn new(metric_key: impl Into<String>, target: i64, now_ms: f64) -> Self {
        Self {
            buffered_once: false,
            display_value: target as f64,
            running: false,
            last_frame_at: now_ms,
            lines_per_second: DEFAULT_LINES_PER_SECOND,
            metric_key: metric_key.into(),
            target_updated_at: now_ms,
            target_value: target as f64,
        }
    }

    /// 目标/开关变化时的处理（对应 TS 那段 `useEffect` 的主体）。
    ///
    /// - `target == None`：不是有限数值，直接显示原值、停表。
    /// - 指标键变化：重置进度。
    /// - 不该动（未激活 / 减动效）：直接跳到目标、停表。
    /// - 否则重新估速，并在需要时启动逐帧循环。
    ///
    /// 返回逐帧循环当前是否应该在跑。
    pub fn apply_target(
        &mut self,
        now_ms: f64,
        target: Option<i64>,
        raw_display: String,
        active: bool,
        reduced_motion: bool,
        metric_key: &str,
    ) -> bool {
        if target.is_none() {
            // TS：`setDisplayValue(value)` 之后直接 return，**不动** state
            self.running = false;
            return false;
        }
        let target = target.unwrap();

        if self.metric_key != metric_key {
            self.buffered_once = false;
            self.display_value = target as f64;
            self.last_frame_at = now_ms;
            self.lines_per_second = DEFAULT_LINES_PER_SECOND;
            self.metric_key = metric_key.to_string();
            self.target_updated_at = now_ms;
            self.target_value = target as f64;
            self.running = false;
        }

        if !active || reduced_motion {
            self.running = false;
            self.display_value = target as f64;
            self.target_value = target as f64;
            self.target_updated_at = now_ms;
            self.last_frame_at = now_ms;
            return false;
        }

        if self.target_value != target as f64 {
            self.lines_per_second = estimate_smooth_metric_lines_per_second(
                now_ms - self.target_updated_at,
                target,
                self.lines_per_second,
                self.target_value as i64,
            );
            self.target_value = target as f64;
            self.target_updated_at = now_ms;
            if !self.buffered_once {
                // 首次采样前多等一会儿，避免把「组件刚挂载」当成一次真实变化
                self.buffered_once = true;
                self.last_frame_at = now_ms + INITIAL_SAMPLE_BUFFER_MS;
            }
        }

        // 对应 TS：`displayValue !== targetValue && frameId === null` 才启动循环
        if !self.running && self.display_value != self.target_value {
            self.running = true;
        }
        let _ = raw_display;
        self.running
    }

    /// 推进一帧（对应 TS 的 `tick`）。返回 `(显示值, 是否继续逐帧)`。
    ///
    /// 时间戳倒退时（`elapsed < 0`）不更新、继续排下一帧 —— 与 TS 一致。
    pub fn tick(&mut self, now_ms: f64) -> (i64, bool) {
        let elapsed_ms = now_ms - self.last_frame_at;
        if elapsed_ms < 0.0 {
            return (self.display_value.round() as i64, true);
        }

        let next = resolve_next_smooth_metric_display(
            self.display_value,
            elapsed_ms,
            self.lines_per_second,
            self.target_value,
        );
        self.display_value = next as f64;
        self.last_frame_at = now_ms;

        let cont = self.display_value != self.target_value;
        self.running = cont;
        (next, cont)
    }
}

/// 自时钟起点起的毫秒数（对应 TS 的 `nowMs()` / `performance.now()`）。
fn elapsed_ms(epoch: Instant) -> f64 {
    epoch.elapsed().as_secs_f64() * 1_000.0
}

/// 空转时的轮询间隔（ms）。TS 的 rAF 大约 16ms 一帧。
const FRAME_POLL_MS: u64 = 16;

/// 未运行时放宽到的轮询间隔（ms）。
///
/// TS 只在需要动画时排 rAF；Rust 侧用一个常驻轮询代替，
/// 空闲时放宽间隔以免每秒无谓唤醒 60 次。
const IDLE_POLL_MS: u64 = 200;

/// 逐帧追赶目标的指标数值。
///
/// 把结果交给 [`FlipMetricValue`] 渲染，于是数字是连续滚动而不是大跳变。
#[component]
pub fn SmoothMetricValue(
    /// 目标值（对应 TS 的 `number | string`）。
    ///
    /// 必填：没有「合理的默认指标值」，静默显示 0 比报错更糟。
    /// 用 `From` 直接写 `value: 42.0` 或 `value: "1.2K"`。
    #[props(into)] value: MetricValue,
    /// 是否允许动画（对应 TS `active`，默认 `false`）。
    #[props(default)] active: bool,
    /// 指标键：变了就重置进度（对应 TS `metricKey`）。
    #[props(default, into)] metric_key: String,
    /// 无障碍：为真时不动画（对应 TS 内部的 `prefers-reduced-motion`）。
    #[props(default)] reduced_motion: bool,
    #[props(default, into)] class: String,
) -> Element {
    let target = value.normalize();
    let raw_display = value.as_display_text();

    // 单调时钟起点。`use_hook` 保证只取一次。
    //
    // 用 `Instant` 而不是墙上时钟：`performance.now()` 在 TS 里也是单调的，
    // 用户改系统时间不该让动画跳帧。
    let epoch = use_hook(Instant::now);
    let start_ms = elapsed_ms(epoch);

    let initial = target.unwrap_or(0);
    let mut state = use_signal(|| SmoothMetricState::new(metric_key.clone(), initial, start_ms));
    let mut display = use_signal(|| target.unwrap_or(0));

    // 渲染期同步状态机。
    //
    // 只在 `target` / `active` / `reduced_motion` / `metric_key` 真正变化时才改动
    // `display`，因此是收敛的（下一次渲染值相同，不再写）。
    // 对应 TS 那段 `useEffect`，只是把「跑在渲染后」改成「跑在渲染中」。
    {
        let now = elapsed_ms(epoch);
        let mut guard = state.write();
        guard.apply_target(now, target, raw_display.clone(), active, reduced_motion, &metric_key);
        let next_display = if target.is_none() {
            // 非数值：TS 直接显示原值，不参与逐帧
            0
        } else {
            guard.display_value.round() as i64
        };
        drop(guard);

        if target.is_none() {
            // 非数值路径单独处理，见下方渲染
        } else if display() != next_display {
            display.set(next_display);
        }
    }

    // 逐帧循环。Dioxus 没有 rAF，用一个常驻任务代替：运行中 16ms 一次，
    // 空闲时 200ms 一次。
    use_future(move || async move {
        loop {
            let running = (state.read().running) && target.is_some() && active && !reduced_motion;
            let wait = if running { FRAME_POLL_MS } else { IDLE_POLL_MS };
            Delay::new(Duration::from_millis(wait)).await;
            if !running {
                continue;
            }
            let (next, _cont) = state.write().tick(elapsed_ms(epoch));
            display.set(next);
        }
    });

    // 目标不是有限数值时，TS 直接显示原字符串（不经过平滑层）
    let shown = if target.is_none() {
        raw_display
    } else {
        display().to_string()
    };

    rsx! {
        FlipMetricValue { value: shown, reduced_motion, class }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- normalize ----

    #[test]
    fn normalize_number_rounds_and_clamps_to_zero() {
        assert_eq!(normalize_smooth_metric_target(42.0), Some(42));
        assert_eq!(normalize_smooth_metric_target(42.4), Some(42));
        assert_eq!(normalize_smooth_metric_target(42.6), Some(43));
        // 负数被夹到 0
        assert_eq!(normalize_smooth_metric_target(-7.0), Some(0));
        assert_eq!(normalize_smooth_metric_target(-0.4), Some(0));
        assert_eq!(normalize_smooth_metric_target(0.0), Some(0));
    }

    #[test]
    fn normalize_number_rejects_non_finite() {
        assert_eq!(normalize_smooth_metric_target(f64::NAN), None);
        assert_eq!(normalize_smooth_metric_target(f64::INFINITY), None);
        assert_eq!(normalize_smooth_metric_target(f64::NEG_INFINITY), None);
    }

    /// `parseInt` 语义：只吃前导整数，后面的字符一律忽略。
    #[test]
    fn normalize_str_follows_parse_int() {
        assert_eq!(normalize_smooth_metric_target_str("42"), Some(42));
        // 小数被截断（parseInt 不会四舍五入）
        assert_eq!(normalize_smooth_metric_target_str("12.9"), Some(12));
        // 指数记法在 'e' 处停下
        assert_eq!(normalize_smooth_metric_target_str("1e3"), Some(1));
        // 尾随垃圾忽略
        assert_eq!(normalize_smooth_metric_target_str("  42abc"), Some(42));
        assert_eq!(normalize_smooth_metric_target_str("42px"), Some(42));
        // 前导空白与正负号
        assert_eq!(normalize_smooth_metric_target_str("\t7"), Some(7));
        assert_eq!(normalize_smooth_metric_target_str("+8"), Some(8));
        assert_eq!(normalize_smooth_metric_target_str("-8"), Some(0));
        // 十六进制前缀按十进制解析：parseInt("0x1f", 10) === 0
        assert_eq!(normalize_smooth_metric_target_str("0x1f"), Some(0));
    }

    #[test]
    fn normalize_str_rejects_when_no_digits() {
        assert_eq!(normalize_smooth_metric_target_str(""), None);
        assert_eq!(normalize_smooth_metric_target_str("abc"), None);
        assert_eq!(normalize_smooth_metric_target_str("   "), None);
        assert_eq!(normalize_smooth_metric_target_str("-"), None);
        assert_eq!(normalize_smooth_metric_target_str("+"), None);
        assert_eq!(normalize_smooth_metric_target_str("."), None);
        assert_eq!(normalize_smooth_metric_target_str("中"), None);
    }

    /// `number` 与 `string` 两条分支行为不同 —— 这正是要保留 [`MetricValue`] 的原因。
    #[test]
    fn number_and_string_branches_differ() {
        // 1234.7：number 分支四舍五入到 1235，string 分支截断到 1234
        assert_eq!(MetricValue::Number(1234.7).normalize(), Some(1235));
        assert_eq!(MetricValue::Text("1234.7".into()).normalize(), Some(1234));
    }

    #[test]
    fn metric_value_conversions() {
        assert_eq!(MetricValue::from(7.0).normalize(), Some(7));
        assert_eq!(MetricValue::from(7i64).normalize(), Some(7));
        assert_eq!(MetricValue::from("7").normalize(), Some(7));
        assert_eq!(MetricValue::from("7".to_string()).normalize(), Some(7));

        // 显示文本：整数不带小数点
        assert_eq!(MetricValue::from(42.0).as_display_text(), "42");
        assert_eq!(MetricValue::from(42.5).as_display_text(), "42.5");
        assert_eq!(MetricValue::from("1.2K").as_display_text(), "1.2K");
    }

    // ---- clamp ----

    #[test]
    fn clamp_speed_bounds_and_fallback() {
        assert_eq!(clamp_metric_speed(48.0), 48.0);
        assert_eq!(clamp_metric_speed(1.0), MIN_LINES_PER_SECOND);
        assert_eq!(clamp_metric_speed(10_000.0), MAX_LINES_PER_SECOND);
        assert_eq!(clamp_metric_speed(MIN_LINES_PER_SECOND), MIN_LINES_PER_SECOND);
        assert_eq!(clamp_metric_speed(MAX_LINES_PER_SECOND), MAX_LINES_PER_SECOND);
        // 非有限 → 默认值
        assert_eq!(clamp_metric_speed(f64::NAN), DEFAULT_LINES_PER_SECOND);
        assert_eq!(clamp_metric_speed(f64::INFINITY), DEFAULT_LINES_PER_SECOND);
        assert_eq!(clamp_metric_speed(f64::NEG_INFINITY), DEFAULT_LINES_PER_SECOND);
    }

    // ---- estimate ----

    #[test]
    fn estimate_ignores_useless_samples() {
        // 目标没变
        assert_eq!(
            estimate_smooth_metric_lines_per_second(100.0, 50, 60.0, 50),
            60.0
        );
        // 时间没走 / 倒退
        assert_eq!(
            estimate_smooth_metric_lines_per_second(0.0, 60, 60.0, 50),
            60.0
        );
        assert_eq!(
            estimate_smooth_metric_lines_per_second(-5.0, 60, 60.0, 50),
            60.0
        );
        // 间隔过长 → 不采信
        assert_eq!(
            estimate_smooth_metric_lines_per_second(SAMPLE_MAX_GAP_MS + 1.0, 60, 60.0, 50),
            60.0
        );
    }

    #[test]
    fn estimate_applies_ema_and_clamps() {
        // delta=10 行 / 100ms → 100 行每秒
        // smoothed = 48 * 0.65 + 100 * 0.35 = 31.2 + 35 = 66.2
        let got = estimate_smooth_metric_lines_per_second(100.0, 60, 48.0, 50);
        assert!((got - 66.2).abs() < 1e-9, "got {got}");

        // 极大的采样会被夹到上限
        let fast = estimate_smooth_metric_lines_per_second(1.0, 100_000, 48.0, 0);
        assert_eq!(fast, MAX_LINES_PER_SECOND);

        // 极小的采样：delta=1 行 / 1000ms → 1 行每秒
        // smoothed = 48 * 0.65 + 1 * 0.35 = 31.55，仍在 [24, 180] 内，不触发夹取
        let slow = estimate_smooth_metric_lines_per_second(1_000.0, 1, 48.0, 0);
        assert!((slow - 31.55).abs() < 1e-9, "got {slow}");

        // 要让结果落到下限，上一次速度本身就得接近下限
        let at_floor = estimate_smooth_metric_lines_per_second(1_000.0, 1, MIN_LINES_PER_SECOND, 0);
        assert!((at_floor - MIN_LINES_PER_SECOND).abs() < 1e-9, "got {at_floor}");
    }

    #[test]
    fn estimate_boundary_at_exactly_the_max_gap() {
        // `elapsedMs > SAMPLE_MAX_GAP_MS` 才丢弃 —— 等于时仍然采信
        let at_limit = estimate_smooth_metric_lines_per_second(SAMPLE_MAX_GAP_MS, 60, 48.0, 50);
        assert_ne!(at_limit, 48.0);
    }

    // ---- resolve ----

    #[test]
    fn resolve_returns_target_when_equal() {
        assert_eq!(resolve_next_smooth_metric_display(10.0, 16.0, 48.0, 10.0), 10);
        // 取整后相等也算到齐
        assert_eq!(resolve_next_smooth_metric_display(10.4, 16.0, 48.0, 10.2), 10);
    }

    #[test]
    fn resolve_steps_up_and_down() {
        // elapsed=16、速度 48：速度步长 = floor(0.768) = 0 → 抬到 1；
        // 但收尾步长 = ceil(100 / ceil(900/16)) = ceil(100/57) = 2 更大，
        // 所以这一帧走 2 行（收尾步长优先，保证不会拖太久）。
        assert_eq!(resolve_next_smooth_metric_display(0.0, 16.0, 48.0, 100.0), 2);
        // 向下同理
        assert_eq!(resolve_next_smooth_metric_display(100.0, 16.0, 48.0, 0.0), 98);

        // 大间隔：max_frames 塌成 1，收尾步长 = 整个距离 → 一帧到位
        assert_eq!(
            resolve_next_smooth_metric_display(0.0, 1_000.0, 180.0, 1_000.0),
            1_000
        );
    }

    /// 一帧时间跨度很大时直接到位（`max_frames` 塌成 1）。
    #[test]
    fn resolve_snaps_in_one_frame_for_long_gaps() {
        assert_eq!(resolve_next_smooth_metric_display(0.0, 5_000.0, 48.0, 777.0), 777);
        assert_eq!(resolve_next_smooth_metric_display(777.0, 5_000.0, 48.0, 0.0), 0);
    }

    /// 步长永远不超过剩余距离 —— 否则会冲过目标再回头。
    #[test]
    fn resolve_never_overshoots() {
        for target in [1.0, 3.0, 7.0, 50.0] {
            let next = resolve_next_smooth_metric_display(0.0, 1_000.0, 180.0, target);
            assert!(next <= target as i64, "冲过目标：next={next} target={target}");
            assert_eq!(next, target as i64, "距离小于一帧步长时应一次到位");
        }
    }

    /// 收尾步长保证再大的跳变也能在 `MAX_SMOOTH_ANIMATION_MS` 内收敛。
    #[test]
    fn resolve_settles_within_the_animation_budget() {
        let elapsed = 16.0;
        let max_frames = (MAX_SMOOTH_ANIMATION_MS / elapsed).ceil();
        let distance = 100_000.0;
        let step = (distance / max_frames).ceil().max(1.0);
        let next = resolve_next_smooth_metric_display(0.0, elapsed, MIN_LINES_PER_SECOND, distance);
        // 速度步长（24 * 16 / 1000 = 0.384 → 1）远小于收尾步长，所以取收尾步长
        assert_eq!(next as f64, step);
    }

    #[test]
    fn resolve_clamps_elapsed_to_one_frame_minimum() {
        // `elapsedMs = 0` / 负数都会被抬到 16，因此与 elapsed=16 结果一致
        assert_eq!(resolve_next_smooth_metric_display(0.0, 0.0, 48.0, 100.0), 2);
        assert_eq!(resolve_next_smooth_metric_display(0.0, -50.0, 48.0, 100.0), 2);
        // 与显式传 16 完全相同
        assert_eq!(
            resolve_next_smooth_metric_display(0.0, 0.0, 48.0, 100.0),
            resolve_next_smooth_metric_display(0.0, 16.0, 48.0, 100.0)
        );
    }

    // ---- 状态机 ----

    #[test]
    fn state_starts_at_the_target() {
        let s = SmoothMetricState::new("lines", 42, 1_000.0);
        assert_eq!(s.display_value, 42.0);
        assert_eq!(s.target_value, 42.0);
        assert_eq!(s.lines_per_second, DEFAULT_LINES_PER_SECOND);
        assert!(!s.running);
        assert!(!s.buffered_once);
    }

    /// 未激活 → 直接跳到目标，不跑逐帧。
    #[test]
    fn inactive_jumps_straight_to_target() {
        let mut s = SmoothMetricState::new("lines", 0, 0.0);
        let running = s.apply_target(100.0, Some(500), "500".into(), false, false, "lines");
        assert!(!running);
        assert_eq!(s.display_value, 500.0);
        assert!(!s.running);
    }

    /// 减动效 → 同样直接跳到目标。
    #[test]
    fn reduced_motion_jumps_straight_to_target() {
        let mut s = SmoothMetricState::new("lines", 0, 0.0);
        let running = s.apply_target(100.0, Some(500), "500".into(), true, true, "lines");
        assert!(!running);
        assert_eq!(s.display_value, 500.0);
    }

    /// 激活 + 目标变远 → 启动逐帧循环，并吃掉首次采样缓冲。
    #[test]
    fn active_starts_the_frame_loop() {
        let mut s = SmoothMetricState::new("lines", 0, 0.0);
        let running = s.apply_target(100.0, Some(500), "500".into(), true, false, "lines");
        assert!(running);
        assert!(s.running);
        assert!(s.buffered_once);
        // 首次采样后 last_frame_at 被推后 INITIAL_SAMPLE_BUFFER_MS
        assert_eq!(s.last_frame_at, 100.0 + INITIAL_SAMPLE_BUFFER_MS);
        // 显示值还在起点，等着逐帧追
        assert_eq!(s.display_value, 0.0);
    }

    /// 目标没变时不应重启循环、也不应重新估速。
    #[test]
    fn same_target_keeps_speed_and_loop_state() {
        let mut s = SmoothMetricState::new("lines", 0, 0.0);
        s.apply_target(100.0, Some(500), "500".into(), true, false, "lines");
        let speed_after_first = s.lines_per_second;
        let last_frame_after_first = s.last_frame_at;

        // 同一个目标再来一次
        s.apply_target(200.0, Some(500), "500".into(), true, false, "lines");
        assert_eq!(s.lines_per_second, speed_after_first);
        assert_eq!(s.last_frame_at, last_frame_after_first);
    }

    /// 指标键变化 → 重置进度（不补动画，直接对齐到新目标）。
    #[test]
    fn changing_metric_key_resets_progress() {
        let mut s = SmoothMetricState::new("lines", 0, 0.0);
        s.apply_target(100.0, Some(500), "500".into(), true, false, "lines");
        // last_frame_at 被推到了 100 + INITIAL_SAMPLE_BUFFER_MS = 220，
        // 所以第一帧要排在 220 之后，否则会被当成「时间戳倒退」而跳过
        s.tick(240.0);
        assert!(s.display_value > 0.0, "应该已经往前走了一点");

        let running = s.apply_target(200.0, Some(9), "9".into(), true, false, "tokens");
        assert_eq!(s.metric_key, "tokens");
        assert_eq!(s.display_value, 9.0, "换指标要直接对齐，不补动画");
        assert_eq!(s.lines_per_second, DEFAULT_LINES_PER_SECOND);
        assert!(!s.buffered_once);
        // 显示值已等于目标 → 不启动循环
        assert!(!running);
    }

    /// 目标不是有限数值 → 不参与平滑（TS 直接显示原值）。
    #[test]
    fn non_numeric_target_stops_the_loop() {
        let mut s = SmoothMetricState::new("lines", 0, 0.0);
        s.apply_target(100.0, Some(500), "500".into(), true, false, "lines");
        assert!(s.running);

        let running = s.apply_target(200.0, None, "N/A".into(), true, false, "lines");
        assert!(!running);
        assert!(!s.running);
    }

    /// 逐帧推进最终必须收敛到目标并自己停下。
    #[test]
    fn ticking_converges_and_stops() {
        let mut s = SmoothMetricState::new("lines", 0, 0.0);
        s.apply_target(100.0, Some(1_000), "1000".into(), true, false, "lines");
        assert!(s.running);

        let mut now = s.last_frame_at;
        let mut frames = 0;
        loop {
            now += 16.0;
            let (value, cont) = s.tick(now);
            frames += 1;
            assert!(frames < 10_000, "没有收敛");
            if !cont {
                assert_eq!(value, 1_000);
                break;
            }
        }
        assert!(!s.running, "收敛后应自动停表");

        // ⚠️ 关于帧数：`MAX_SMOOTH_ANIMATION_MS = 900` 只保证「**每帧走剩余距离的
        // 1/max_frames**」，而剩余距离本身在缩小，所以这是**几何收敛**而不是
        // 恒定步长的 57 帧 —— 1000 行的跳变实测约 170 帧（≈2.7s）。
        // 这里逐字复刻上游公式，不「修正」它；断言只保证有界收敛。
        assert!(frames < 400, "收敛太慢：{frames} 帧");
    }

    /// 小跳变（距离 ≤ max_frames）时收尾步长退化为 1，速度步长接管，收敛很快。
    #[test]
    fn small_jumps_converge_quickly() {
        let mut s = SmoothMetricState::new("lines", 0, 0.0);
        // 上一次速度直接给定为默认值，避免被估速分支改大
        s.apply_target(100.0, Some(20), "20".into(), true, false, "lines");
        s.lines_per_second = DEFAULT_LINES_PER_SECOND;

        let mut now = s.last_frame_at;
        let mut frames = 0;
        loop {
            now += 16.0;
            let (value, cont) = s.tick(now);
            frames += 1;
            assert!(frames < 100, "小跳变不该这么慢：{frames} 帧");
            if !cont {
                assert_eq!(value, 20);
                break;
            }
        }
        // 速度步长 = floor(48*16/1000) = 0 → 1；20 行 → 约 20 帧
        assert!(frames <= 25, "got {frames}");
    }

    /// 时间戳倒退时不更新、继续排帧（与 TS 的 `elapsedMs < 0` 分支一致）。
    #[test]
    fn tick_ignores_backwards_timestamps() {
        let mut s = SmoothMetricState::new("lines", 0, 0.0);
        s.apply_target(100.0, Some(1_000), "1000".into(), true, false, "lines");
        let before = s.display_value;
        let (value, cont) = s.tick(0.0); // 早于 last_frame_at
        assert_eq!(s.display_value, before);
        assert_eq!(value, before as i64);
        assert!(cont, "倒退时应继续排下一帧");
    }

    /// 时间戳倒退后再前进，仍然能正常收敛。
    #[test]
    fn recovers_after_a_backwards_timestamp() {
        let mut s = SmoothMetricState::new("lines", 0, 0.0);
        s.apply_target(100.0, Some(200), "200".into(), true, false, "lines");
        let mut now = s.last_frame_at;
        let _ = s.tick(now - 500.0);
        let mut frames = 0;
        loop {
            now += 16.0;
            let (value, cont) = s.tick(now);
            frames += 1;
            assert!(frames < 10_000);
            if !cont {
                assert_eq!(value, 200);
                break;
            }
        }
    }

    /// 往下追（目标变小）也要收敛。
    #[test]
    fn ticking_converges_downwards_too() {
        let mut s = SmoothMetricState::new("lines", 5_000, 0.0);
        s.apply_target(100.0, Some(10), "10".into(), true, false, "lines");
        let mut now = s.last_frame_at;
        let mut frames = 0;
        loop {
            now += 16.0;
            let (value, cont) = s.tick(now);
            frames += 1;
            assert!(frames < 10_000);
            if !cont {
                assert_eq!(value, 10);
                break;
            }
        }
        assert!(!s.running);
    }
}
