//! `FloatingParticles` —— Rust 移植自 `packages/ui/src/components/ui/floating-particles.tsx`
//!
//! TS 是一张 Canvas 上飘 96 个粒子的背景装饰。移植的关键判断是**切在哪里**：
//!
//! | 部分 | 可移植性 | 本模块的处理 |
//! |---|---|---|
//! | 粒子表生成（`seededUnit` + 8 个通道） | 纯确定性数学 | ✅ 全量移植，[`particles`] |
//! | 每帧的进度 / 插值 / 透明度 / 缩放 | 纯数学 | ✅ 全量移植，[`particle_frame`] |
//! | 离屏 sprite 生成（`createParticleSprite`） | 需要 `canvas.getContext("2d")` | ❌ 由渲染器负责 |
//! | 逐帧 `drawImage` | 需要 2D context | ❌ 由渲染器负责 |
//! | 尺寸测量（`ResizeObserver` + `getBoundingClientRect`） | 需要 DOM | ❌ 改为显式 `size` prop |
//!
//! 也就是说：本模块产出**每帧要画什么**（[`ParticleDraw`]），
//! 「怎么画」留给渲染器。这样粒子运动逻辑可以被完整单测，
//! 而 Canvas 绑定只在真正上屏时才需要。
//!
//! ## 关键移植点 1：粒子表是确定性的
//!
//! `seededUnit(index, salt)` 用 `sin` 哈希生成伪随机数，**没有任何真随机**，
//! 因此 96 个粒子的初始位置/速度在任何环境都一致。移植时必须逐字保留公式，
//! 否则粒子的分布看起来会不一样。
//!
//! ## 关键移植点 2：两处「踩过坑」的注释
//!
//! 1. **统一到一张 Canvas**：旧实现同时挂 96 个 `motion.span` 与独立动画控制器，
//!    恰好和扩散 Canvas 收尾竞争渲染主线程；现在只剩父层 opacity 合成。
//!    → Rust 侧同样只输出**一个** `canvas` 元素。
//! 2. **辉光离屏生成一次**：Canvas 的 `shadowBlur` 在部分 Electron/macOS 环境会
//!    逐帧触发昂贵的软件栅格化。辉光只在尺寸变化时离屏画一次，动画帧复用位图。
//!    → Rust 侧把它写进 [`ParticleDraw::highlight`]，让渲染器知道哪些粒子需要
//!    用「带辉光的 sprite」。
//!
//! ## 关键移植点 3：`settled` 跳过入场段
//!
//! `settled = true` 时把起点往前推 [`PARTICLE_SETTLED_ELAPSED_MS`]（14s，
//! 大于最大相位偏移 2.6s + 最长周期 11.2s），粒子直接处于循环稳态，
//! 不再从空场逐个冒出。
//!
//! ## 与 TS 的差异
//!
//! 1. **没有 `ResizeObserver` / `getComputedStyle`**：尺寸与颜色由调用方提供
//!    （`size` prop + 渲染器自己读 CSS 变量）。
//! 2. **`prefers-reduced-motion` 改为显式 prop**。
//! 3. **`memo` 无对应物**：Dioxus 组件默认按 props 变化重渲染，不需要手写浅比较。
//! 4. 帧节流（`PARTICLE_FRAME_INTERVAL_MS`）由调用方的 tick 驱动负责，
//!    本模块只提供 [`should_draw_frame`] 这个纯判定。

use crate::cn::cn;
use dioxus::prelude::*;
use futures_timer::Delay;
use std::sync::OnceLock;
use std::time::Duration;

/// 粒子数量（TS `PARTICLE_COUNT`）。
pub const PARTICLE_COUNT: usize = 96;

/// 帧间隔（ms）—— 30fps（TS `PARTICLE_FRAME_INTERVAL_MS = 1_000 / 30`）。
pub const PARTICLE_FRAME_INTERVAL_MS: f64 = 1_000.0 / 30.0;

/// 离屏 sprite 的边长（px）。
pub const PARTICLE_SPRITE_SIZE: f64 = 24.0;

/// 普通粒子半径（px）。
pub const PARTICLE_RADIUS: f64 = 1.0;

/// 高亮粒子半径（px）。
pub const HIGHLIGHT_PARTICLE_RADIUS: f64 = 1.5;

/// 高亮粒子的辉光半径（px）。
pub const HIGHLIGHT_PARTICLE_BLUR: f64 = 7.0;

/// 高亮粒子的下标。
///
/// TS 注释：按 seeded `startLeft` 选取，横向覆盖约 8%–92%，
/// 避免亮点集中在同一区域。
pub const HIGHLIGHT_PARTICLE_INDICES: [usize; 7] = [1, 22, 42, 56, 57, 74, 90];

/// `settled` 时把起点往前推的时长（ms）。
///
/// TS 注释：超过最大相位偏移 2.6s + 最长周期 11.2s，粒子直接处于循环稳态，
/// 不再从空场逐个冒出。
pub const PARTICLE_SETTLED_ELAPSED_MS: f64 = 14_000.0;

/// 减动效时的固定进度（TS `progress = 0.48`）。
pub const REDUCED_MOTION_PROGRESS: f64 = 0.48;

/// 减动效时的透明度系数（TS `particle.opacity * 0.72`）。
pub const REDUCED_MOTION_OPACITY_SCALE: f64 = 0.72;

/// 插值的时间锚点（TS `interpolateParticleChannel` 里的 `times`）。
pub const PARTICLE_CHANNEL_TIMES: [f64; 5] = [0.0, 0.12, 0.38, 0.7, 1.0];

/// 外层容器类名（TS 原文，逐字保留）。
pub const FLOATING_PARTICLES_BASE: &str =
    "pointer-events-none absolute inset-0 overflow-hidden";

/// 画布类名（TS 原文，逐字保留）。
pub const FLOATING_PARTICLES_CANVAS: &str = "highspeed-particle-canvas absolute inset-0 size-full";

/// 一个粒子的静态参数（对应 TS 的 `FloatingParticle`）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct FloatingParticle {
    /// 一个完整上升周期的时长（秒）。
    pub cycle_duration: f64,
    /// 是否用「带辉光」的 sprite。
    pub highlight: bool,
    /// 横向漂移总距离（px）。
    pub horizontal_drift: f64,
    /// 峰值透明度。
    pub opacity: f64,
    /// 相位偏移（秒）—— 让粒子错开出发时间。
    pub phase_offset: f64,
    /// 上升总距离（px，向上为负）。
    pub rise_distance: f64,
    /// 起始横向位置（占容器宽度的百分比）。
    pub start_left: f64,
    /// 起始纵向位置（占容器高度的百分比）。
    pub start_top: f64,
    /// 左右摇摆幅度（px）。
    pub sway_distance: f64,
}

/// 确定性伪随机：`sin` 哈希，逐字移植 TS `seededUnit`。
///
/// 返回 `[0, 1)`。**不要改公式** —— 改了粒子的分布就与 TS 不一致。
pub fn seeded_unit(index: usize, salt: f64) -> f64 {
    let value = ((index as f64 + 1.0) * 12.9898 + salt * 78.233).sin() * 43758.5453;
    value - value.floor()
}

/// 生成全部粒子（确定性，只算一次）。
pub fn particles() -> &'static [FloatingParticle] {
    static PARTICLES: OnceLock<Vec<FloatingParticle>> = OnceLock::new();
    PARTICLES.get_or_init(|| {
        (0..PARTICLE_COUNT)
            .map(|index| {
                let highlight = HIGHLIGHT_PARTICLE_INDICES.contains(&index);
                FloatingParticle {
                    // 94 + [0,6)：从底部稍下方起飘
                    start_top: 94.0 + seeded_unit(index, 1.0) * 6.0,
                    start_left: seeded_unit(index, 2.0) * 100.0,
                    rise_distance: 72.0 + seeded_unit(index, 3.0) * 64.0,
                    // [-18, 18)
                    horizontal_drift: seeded_unit(index, 4.0) * 36.0 - 18.0,
                    // [-8, 8)
                    sway_distance: seeded_unit(index, 5.0) * 16.0 - 8.0,
                    opacity: if highlight {
                        0.82 + seeded_unit(index, 6.0) * 0.16
                    } else {
                        0.4 + seeded_unit(index, 6.0) * 0.32
                    },
                    cycle_duration: 6.4 + seeded_unit(index, 7.0) * 4.8,
                    phase_offset: seeded_unit(index, 8.0) * 2.6,
                    highlight,
                }
            })
            .collect()
    })
}

/// 线性插值（TS `lerp`）。
pub fn lerp(from: f64, to: f64, progress: f64) -> f64 {
    from + (to - from) * progress
}

/// 在 5 个锚点上做分段线性插值，逐字移植 TS `interpolateParticleChannel`。
///
/// `values` 必须正好 5 个（对应 [`PARTICLE_CHANNEL_TIMES`]）。
pub fn interpolate_particle_channel(progress: f64, values: &[f64; 5]) -> f64 {
    // `times.findIndex((time) => time > progress)`：找不到（progress >= 1）时是 -1
    let next_index = PARTICLE_CHANNEL_TIMES
        .iter()
        .position(|time| *time > progress)
        .map(|i| i as isize)
        .unwrap_or(-1);
    // `Math.max(0, Math.min(times.length - 2, nextIndex - 1))`
    let segment = 0isize.max((PARTICLE_CHANNEL_TIMES.len() as isize - 2).min(next_index - 1)) as usize;

    let start = PARTICLE_CHANNEL_TIMES[segment];
    let end = PARTICLE_CHANNEL_TIMES[segment + 1];
    lerp(
        values[segment],
        values[segment + 1],
        (progress - start) / (end - start).max(f64::EPSILON),
    )
}

/// 某一帧里单个粒子的绘制状态。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ParticleFrame {
    /// 相对起点的横向偏移（px）。
    pub offset_x: f64,
    /// 相对起点的纵向偏移（px，向上为负）。
    pub offset_y: f64,
    /// 全局透明度。
    pub opacity: f64,
    /// sprite 缩放倍数。
    pub scale: f64,
}

/// 算出该粒子在 `progress` 处的绘制状态。
///
/// 逐字移植 TS `draw` 里那 4 次 `interpolateParticleChannel`。
pub fn particle_frame(
    progress: f64,
    particle: &FloatingParticle,
    reduced_motion: bool,
) -> ParticleFrame {
    let sway = particle.sway_distance;
    let drift = particle.horizontal_drift;
    let rise = particle.rise_distance;

    let offset_x = interpolate_particle_channel(
        progress,
        &[0.0, sway * 0.35, -sway * 0.25, drift * 0.65 + sway * 0.2, drift],
    );
    let offset_y = interpolate_particle_channel(
        progress,
        &[0.0, -rise * 0.38, -rise * 0.64, -rise * 0.82, -rise],
    );
    let opacity = if reduced_motion {
        particle.opacity * REDUCED_MOTION_OPACITY_SCALE
    } else {
        interpolate_particle_channel(
            progress,
            &[
                0.0,
                particle.opacity,
                particle.opacity * 0.92,
                particle.opacity * 0.7,
                0.0,
            ],
        )
    };
    let scale = interpolate_particle_channel(progress, &[0.55, 1.1, 1.0, 0.8, 0.4]);

    ParticleFrame {
        offset_x,
        offset_y,
        opacity,
        scale,
    }
}

/// 该粒子在 `elapsed_ms` 时的循环进度。
///
/// - 还没到出发时间（`elapsed < phaseOffset`）且**没有**减动效 → `None`（这一帧不画）。
/// - 减动效 → 恒定 [`REDUCED_MOTION_PROGRESS`]（静态定格）。
/// - 否则 → `(active % cycle) / cycle`，落在 `[0, 1)`。
pub fn particle_progress(
    elapsed_ms: f64,
    particle: &FloatingParticle,
    reduced_motion: bool,
) -> Option<f64> {
    let elapsed_seconds = elapsed_ms.max(0.0) / 1_000.0;
    let active_elapsed = elapsed_seconds - particle.phase_offset;
    if active_elapsed < 0.0 && !reduced_motion {
        return None;
    }
    if reduced_motion {
        return Some(REDUCED_MOTION_PROGRESS);
    }
    Some((active_elapsed % particle.cycle_duration) / particle.cycle_duration)
}

/// 一帧里要画的一条指令（渲染器据此 `drawImage`）。
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct ParticleDraw {
    /// 目标中心 x（px）。
    pub x: f64,
    /// 目标中心 y（px）。
    pub y: f64,
    /// 目标边长（px）。
    pub size: f64,
    /// 透明度。
    pub opacity: f64,
    /// 用带辉光的 sprite（对应 TS 的 `highlightParticleSprite`）。
    pub highlight: bool,
}

/// 算出这一帧所有要画的粒子。
///
/// `width` / `height` 是画布的逻辑尺寸（px）。
/// 逐字移植 TS `draw` 里的坐标公式：
/// `(startLeft / 100) * width + x`、`(startTop / 100) * height + y`，
/// sprite 以中心对齐。
pub fn particle_frame_draws(
    elapsed_ms: f64,
    width: f64,
    height: f64,
    reduced_motion: bool,
) -> Vec<ParticleDraw> {
    particles()
        .iter()
        .filter_map(|particle| {
            let progress = particle_progress(elapsed_ms, particle, reduced_motion)?;
            let frame = particle_frame(progress, particle, reduced_motion);
            let sprite_size = PARTICLE_SPRITE_SIZE * frame.scale;
            Some(ParticleDraw {
                x: (particle.start_left / 100.0) * width + frame.offset_x,
                y: (particle.start_top / 100.0) * height + frame.offset_y,
                size: sprite_size,
                opacity: frame.opacity,
                highlight: particle.highlight,
            })
        })
        .collect()
}

/// 帧节流判定（对应 TS `draw` 开头的 `if (now - lastFrameAt < FRAME_INTERVAL_MS) return`）。
pub fn should_draw_frame(now_ms: f64, last_frame_at_ms: f64) -> bool {
    now_ms - last_frame_at_ms >= PARTICLE_FRAME_INTERVAL_MS
}

/// 粒子背景的绘制回调载荷。
#[derive(Clone, PartialEq, Debug)]
pub struct ParticleBatch {
    /// 距动画起点的毫秒数（已计入 `settled` 的偏移）。
    pub elapsed_ms: f64,
    /// 本帧要画的粒子。
    pub draws: Vec<ParticleDraw>,
}

/// 飘浮粒子背景。
///
/// 只渲染「容器 + 一张 canvas」，并把每帧要画什么通过 `frames` 信号交给调用方。
/// 真正的 2D 绘制需要渲染器绑定（见模块文档的差异说明）。
#[component]
pub fn FloatingParticles(
    /// 直接从循环稳态开始绘制（Highspeed 恢复入场）。
    #[props(default)] settled: bool,
    /// 无障碍：为真时粒子定格在固定进度（对应 TS 内部的 `prefers-reduced-motion`）。
    #[props(default)] reduced_motion: bool,
    /// 画布逻辑尺寸（px）。TS 用 `ResizeObserver` 测量，这里显式传入。
    #[props(default = (1.0, 1.0))] size: (f64, f64),
    /// 每帧的绘制载荷。调用方接上 2D context 即可上屏。
    #[props(default)] frames: Option<Signal<ParticleBatch>>,
    #[props(default, into)] class: String,
) -> Element {
    let (width, height) = size;

    // 减动效只需画一帧；否则按 30fps 推进。
    if let Some(mut signal) = frames {
        use_future(move || async move {
            let mut last_frame_at = f64::NEG_INFINITY;
            // `startedAt = now - (settled ? SETTLED_ELAPSED_MS : 0)`
            let started_at = -if settled { PARTICLE_SETTLED_ELAPSED_MS } else { 0.0 };
            let mut elapsed_ms = 0.0;
            loop {
                let now = elapsed_ms - started_at;
                if should_draw_frame(now, last_frame_at) {
                    last_frame_at = now;
                    signal.set(ParticleBatch {
                        elapsed_ms: now,
                        draws: particle_frame_draws(now, width, height, reduced_motion),
                    });
                }
                // 减动效定格：一帧就够，不再推进
                if reduced_motion {
                    return;
                }
                Delay::new(Duration::from_millis(PARTICLE_FRAME_INTERVAL_MS as u64)).await;
                elapsed_ms += PARTICLE_FRAME_INTERVAL_MS;
            }
        });
    }

    rsx! {
        div {
            class: cn(&[FLOATING_PARTICLES_BASE, &class]),
            "data-slot": "floating-particles",
            "data-settled": if settled { "true" } else { "false" },
            "aria-hidden": "true",
            canvas { class: FLOATING_PARTICLES_CANVAS, "data-slot": "floating-particles-canvas" }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_unit_is_deterministic_and_in_range() {
        for index in 0..PARTICLE_COUNT {
            for salt in 1..=8 {
                let a = seeded_unit(index, salt as f64);
                let b = seeded_unit(index, salt as f64);
                assert_eq!(a, b, "同样的输入必须给同样的输出");
                assert!((0.0..1.0).contains(&a), "越界：{a}");
            }
        }
    }

    /// 不同的 salt 必须给出不同的序列 —— 否则 8 个通道会高度相关。
    #[test]
    fn different_salts_give_different_values() {
        let first: Vec<f64> = (1..=8).map(|s| seeded_unit(0, s as f64)).collect();
        let mut sorted = first.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        sorted.dedup();
        assert_eq!(sorted.len(), first.len(), "8 个通道出现了重复：{first:?}");
    }

    #[test]
    fn particles_has_the_expected_count() {
        assert_eq!(particles().len(), PARTICLE_COUNT);
    }

    #[test]
    fn exactly_the_listed_particles_are_highlighted() {
        let ps = particles();
        let highlighted: Vec<usize> = ps
            .iter()
            .enumerate()
            .filter_map(|(i, p)| p.highlight.then_some(i))
            .collect();
        assert_eq!(highlighted, HIGHLIGHT_PARTICLE_INDICES.to_vec());
    }

    /// 每个通道的取值区间必须与 TS 的公式一致。
    #[test]
    fn particle_channels_respect_their_ranges() {
        for p in particles() {
            // startTop = 94 + u * 6
            assert!((94.0..100.0).contains(&p.start_top), "{p:?}");
            // startLeft = u * 100
            assert!((0.0..100.0).contains(&p.start_left), "{p:?}");
            // riseDistance = 72 + u * 64
            assert!((72.0..136.0).contains(&p.rise_distance), "{p:?}");
            // horizontalDrift = u * 36 - 18
            assert!((-18.0..18.0).contains(&p.horizontal_drift), "{p:?}");
            // swayDistance = u * 16 - 8
            assert!((-8.0..8.0).contains(&p.sway_distance), "{p:?}");
            // cycleDuration = 6.4 + u * 4.8
            assert!((6.4..11.2).contains(&p.cycle_duration), "{p:?}");
            // phaseOffset = u * 2.6
            assert!((0.0..2.6).contains(&p.phase_offset), "{p:?}");
        }
    }

    /// 高亮粒子的透明度区间比普通粒子高（0.82–0.98 vs 0.40–0.72）。
    #[test]
    fn highlight_particles_are_brighter() {
        for p in particles() {
            if p.highlight {
                assert!((0.82..0.98).contains(&p.opacity), "{p:?}");
            } else {
                assert!((0.40..0.72).contains(&p.opacity), "{p:?}");
            }
        }
    }

    /// 相位偏移 + 周期不能超过 `settled` 的提前量，否则稳态假设不成立。
    #[test]
    fn settled_offset_covers_every_particle_phase() {
        let settled_seconds = PARTICLE_SETTLED_ELAPSED_MS / 1_000.0;
        for p in particles() {
            assert!(
                p.phase_offset + p.cycle_duration <= settled_seconds,
                "粒子可能还没进入稳态：{p:?}"
            );
        }
    }

    #[test]
    fn lerp_endpoints_and_midpoint() {
        assert_eq!(lerp(0.0, 10.0, 0.0), 0.0);
        assert_eq!(lerp(0.0, 10.0, 1.0), 10.0);
        assert_eq!(lerp(0.0, 10.0, 0.5), 5.0);
        assert_eq!(lerp(10.0, 0.0, 0.25), 7.5);
        // progress 可以超出 [0,1] 做外推（TS 也不夹）
        assert_eq!(lerp(0.0, 10.0, 2.0), 20.0);
    }

    #[test]
    fn channel_interpolation_hits_every_anchor() {
        let values = [0.0, 10.0, 20.0, 30.0, 40.0];
        // 只有 `[0.0, 0.12, 0.38, 0.7]` 是「真锚点」：它们的 `t` 能定位到 `nextIndex`，
        // 于是 `segment = nextIndex - 1` 恰好让 `t` 落在段首，取值等于该锚点本身。
        // 最后一个 `1.0` 不是锚点 —— `findIndex` 找不到更大的 `t` 会返回 -1，见
        // [`channel_interpolation_falls_back_to_segment_zero_past_one`]。
        for (i, t) in PARTICLE_CHANNEL_TIMES[..4].iter().enumerate() {
            let got = interpolate_particle_channel(*t, &values);
            let expected = values[i] as f64;
            assert!(
                (got - expected).abs() < 1e-9,
                "progress={t} 期望 {expected}，实得 {got}"
            );
        }
        // 末端锚点 0.7 属于第 3 段（0.7 → 1.0）的段首，取 `values[3]`
        assert!((interpolate_particle_channel(0.7, &values) - 30.0).abs() < 1e-9);
        // 而 1.0 会退回第 0 段外推：t = (1.0 - 0) / 0.12 = 8.333…
        assert!((interpolate_particle_channel(1.0, &values) - 250.0 / 3.0).abs() < 1e-9);
    }

    #[test]
    fn channel_interpolation_is_piecewise_linear() {
        let values = [0.0, 12.0, 38.0, 70.0, 100.0];
        // 落在第 2 段（0.38 → 0.7）中点
        let mid = (0.38 + 0.7) / 2.0;
        let got = interpolate_particle_channel(mid, &values);
        assert!((got - 54.0).abs() < 1e-9, "got {got}");
        // 落在第 0 段（0 → 0.12）中点
        let got0 = interpolate_particle_channel(0.06, &values);
        assert!((got0 - 6.0).abs() < 1e-9, "got {got0}");
    }

    /// `progress >= 1` 时 `findIndex` 返回 -1，segment 夹到 0 —— 逐字复刻 TS 行为。
    #[test]
    fn channel_interpolation_falls_back_to_segment_zero_past_one() {
        let values = [0.0, 12.0, 38.0, 70.0, 100.0];
        // segment = 0，t = (1.5 - 0) / 0.12 = 12.5 → 0 + 12 * 12.5 = 150
        let got = interpolate_particle_channel(1.5, &values);
        assert!((got - 150.0).abs() < 1e-9, "got {got}");
    }

    #[test]
    fn progress_is_none_before_departure() {
        let p = FloatingParticle {
            cycle_duration: 10.0,
            highlight: false,
            horizontal_drift: 0.0,
            opacity: 0.5,
            phase_offset: 2.0,
            rise_distance: 100.0,
            start_left: 50.0,
            start_top: 95.0,
            sway_distance: 0.0,
        };
        // 还没到 2s
        assert_eq!(particle_progress(1_999.0, &p, false), None);
        // 刚好到
        assert_eq!(particle_progress(2_000.0, &p, false), Some(0.0));
        // 半周期
        assert_eq!(particle_progress(7_000.0, &p, false), Some(0.5));
        // 一个完整周期后回到 0
        assert_eq!(particle_progress(12_000.0, &p, false), Some(0.0));
    }

    /// 减动效时不看出发时间，所有粒子都定格在同一进度。
    #[test]
    fn reduced_motion_pins_every_particle() {
        let p = FloatingParticle {
            cycle_duration: 10.0,
            highlight: true,
            horizontal_drift: 0.0,
            opacity: 0.9,
            phase_offset: 2.5,
            rise_distance: 100.0,
            start_left: 50.0,
            start_top: 95.0,
            sway_distance: 0.0,
        };
        // 即使还没到出发时间也照样定格
        assert_eq!(
            particle_progress(0.0, &p, true),
            Some(REDUCED_MOTION_PROGRESS)
        );
        assert_eq!(
            particle_progress(999_999.0, &p, true),
            Some(REDUCED_MOTION_PROGRESS)
        );
    }

    #[test]
    fn progress_handles_negative_elapsed() {
        let p = &particles()[0];
        // 负数被夹到 0
        assert_eq!(particle_progress(-500.0, p, false), None);
        assert_eq!(
            particle_progress(-500.0, p, true),
            Some(REDUCED_MOTION_PROGRESS)
        );
    }

    /// 粒子从下往上飘：纵向偏移在整个周期里都是负的。
    #[test]
    fn particles_rise_upwards() {
        let p = FloatingParticle {
            cycle_duration: 10.0,
            highlight: false,
            horizontal_drift: 10.0,
            opacity: 0.5,
            phase_offset: 0.0,
            rise_distance: 100.0,
            start_left: 50.0,
            start_top: 95.0,
            sway_distance: 4.0,
        };
        // 真锚点区间是 [0, 0.7]；通道值 [0, -38, -64, -82, -100] 单调下降，
        // 所以这一整段内偏移都非正。
        for step in 0..=70 {
            let progress = step as f64 / 100.0;
            let frame = particle_frame(progress, &p, false);
            assert!(frame.offset_y <= 0.0, "progress={progress} 向上偏移为正了");
        }
        // 0.7 是第 3 段段首 → 恰好是 rise * 0.82
        let at_anchor = particle_frame(0.7, &p, false);
        assert!((at_anchor.offset_y - (-82.0)).abs() < 1e-9, "{at_anchor:?}");
        // 末端锚点 1.0 只在「整段外推」时才被直接调用，此时会退回第 0 段：
        // t = (1.0 - 0) / 0.12 = 8.333…，于是 offset_y = 0 + (-38) * 8.333… = -316.66…
        // 这是逐字复刻 TS 的行为，不是「上升一个 rise_distance」。
        let past_one = particle_frame(1.0, &p, false);
        let expected = -(p.rise_distance * 0.38) / PARTICLE_CHANNEL_TIMES[1];
        assert!(
            (past_one.offset_y - expected).abs() < 1e-9,
            "{past_one:?}，期望 {expected}"
        );
    }

    /// 透明度：起点为 0、中途最高、到 0.7 已经衰减到 70%。
    #[test]
    fn opacity_fades_in_and_out() {
        let p = FloatingParticle {
            cycle_duration: 10.0,
            highlight: false,
            horizontal_drift: 0.0,
            opacity: 0.6,
            phase_offset: 0.0,
            rise_distance: 100.0,
            start_left: 50.0,
            start_top: 95.0,
            sway_distance: 0.0,
        };
        assert_eq!(particle_frame(0.0, &p, false).opacity, 0.0);
        // 锚点 0.12 处正好是峰值
        assert!((particle_frame(0.12, &p, false).opacity - 0.6).abs() < 1e-9);
        // 最后一个真锚点 0.7：opacity * 0.7
        let at_anchor = particle_frame(0.7, &p, false).opacity;
        assert!((at_anchor - 0.42).abs() < 1e-9, "{at_anchor}");
        // progress = 1.0 不是锚点，会退回第 0 段外推（值 > 1，故绘制前需按需夹取）。
        // 这里只钉住移植过来的数学行为，不主张它是「淡出到 0」。
        let past_one = particle_frame(1.0, &p, false).opacity;
        let expected = p.opacity / PARTICLE_CHANNEL_TIMES[1];
        assert!((past_one - expected).abs() < 1e-9, "{past_one}，期望 {expected}");
    }

    #[test]
    fn reduced_motion_dims_the_particle() {
        let p = &particles()[0];
        let frame = particle_frame(REDUCED_MOTION_PROGRESS, p, true);
        assert!((frame.opacity - p.opacity * REDUCED_MOTION_OPACITY_SCALE).abs() < 1e-9);
    }

    /// sprite 缩放区间是 [0.4, 1.1]（在真锚点区间 [0, 0.7] 内）。
    #[test]
    fn sprite_scale_stays_in_range() {
        let p = &particles()[0];
        // 通道值 [0.55, 1.1, 1.0, 0.8, 0.4] 在 0 → 0.7 上先升后降，
        // 峰值 1.1 落在锚点 0.12、谷值 0.8 落在锚点 0.7，都在区间内。
        for step in 0..=70 {
            let progress = step as f64 / 100.0;
            let scale = particle_frame(progress, p, false).scale;
            assert!(
                (0.4..=1.1).contains(&scale),
                "progress={progress} scale={scale}"
            );
        }
        // 正常循环路径下 `particle_progress` 恒 < 1，所以 1.0 只会由直接调用触达；
        // 它退回第 0 段外推后得到 0.55 + 0.55 / 0.12 ≈ 5.13，超出区间 —— 逐字复刻 TS。
        let past_one = particle_frame(1.0, p, false).scale;
        let expected = 0.55 + (1.1 - 0.55) / PARTICLE_CHANNEL_TIMES[1];
        assert!(
            (past_one - expected).abs() < 1e-9,
            "progress=1 scale={past_one}，期望 {expected}"
        );
    }

    #[test]
    fn draws_are_positioned_by_container_size() {
        let draws = particle_frame_draws(60_000.0, 400.0, 300.0, false);
        assert!(!draws.is_empty());
        for d in &draws {
            // x 由 startLeft(0–100%) 决定，允许漂移量（±18）与 sway 外推
            assert!(d.x > -100.0 && d.x < 500.0, "{d:?}");
            // y 由 startTop(94–100%) 决定，减去上升距离（最多 136）
            assert!(d.y > -200.0 && d.y < 320.0, "{d:?}");
            assert!(d.size > 0.0);
            assert!((0.0..=1.1).contains(&d.opacity), "{d:?}");
        }
    }

    /// 大提前量下所有粒子都可见；刚起步时只有相位为 0 的少数粒子可见。
    #[test]
    fn settled_time_shows_every_particle() {
        let settled = particle_frame_draws(PARTICLE_SETTLED_ELAPSED_MS, 400.0, 300.0, false);
        assert_eq!(settled.len(), PARTICLE_COUNT);
        // 起步瞬间只剩相位偏移为 0 的那一个（`phaseOffset` 恒 >= 0）
        let at_zero = particle_frame_draws(0.0, 400.0, 300.0, false);
        assert!(at_zero.len() <= 1, "起步时应几乎为空场：{}", at_zero.len());
    }

    #[test]
    fn frame_throttle_matches_thirty_fps() {
        // 恰好一个帧间隔 → 该画
        assert!(should_draw_frame(PARTICLE_FRAME_INTERVAL_MS, 0.0));
        // 略小于一个帧间隔 → 跳过
        assert!(!should_draw_frame(PARTICLE_FRAME_INTERVAL_MS - 0.001, 0.0));
        // 第一次调用（lastFrameAt = -INTERVAL）→ 该画
        assert!(should_draw_frame(0.0, -PARTICLE_FRAME_INTERVAL_MS));
    }

    #[test]
    fn component_classes() {
        assert!(FLOATING_PARTICLES_BASE.contains("pointer-events-none"));
        assert!(FLOATING_PARTICLES_BASE.contains("absolute"));
        assert!(FLOATING_PARTICLES_BASE.contains("inset-0"));
        assert!(FLOATING_PARTICLES_BASE.contains("overflow-hidden"));

        assert!(FLOATING_PARTICLES_CANVAS.contains("absolute"));
        assert!(FLOATING_PARTICLES_CANVAS.contains("inset-0"));
        assert!(FLOATING_PARTICLES_CANVAS.contains("size-full"));
    }

    #[test]
    fn user_class_is_appended() {
        let c = cn(&[FLOATING_PARTICLES_BASE, "z-10"]);
        assert!(c.contains("z-10"));
        assert!(c.contains("pointer-events-none"));
    }
}
