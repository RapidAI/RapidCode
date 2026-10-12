#!/usr/bin/env bash
# 从 packages/ui/src/styles.css 抽取设计令牌 + 映射 Rust 源码里的类名，
# 生成 crates/zcode-app/assets/app.css。
#
# 为什么不是直接编译 packages/ui/src/styles.css：
#   1. 它 @import 了 shadcn/tailwind.css、@xterm/xterm、katex、@tailwindcss/typography
#      等本仓库未安装的包；
#   2. 它 @source 扫描的是 TSX，对 Rust 源码无效。
# 所以这里只抽取「自包含」的部分（@theme 令牌 + .dark 覆盖 + 基础层），
# 自己补上 shadcn/tailwind.css 里的 @custom-variant 映射，再把 @source 指向 rust/crates。
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
STYLES="$REPO/packages/ui/src/styles.css"
OUT_ENTRY="$HERE/entry.css"
OUT_CSS="$REPO/rust/crates/zcode-app/assets/app.css"

test -f "$STYLES" || { echo "找不到 $STYLES"; exit 1; }

{
  cat <<'CSS'
/* =====================================================================
 * 本文件由 rust/.tailwind-build/build.sh 自动生成，请勿手工编辑。
 *
 * 生成方式：以 packages/ui/src/styles.css 的 @theme / .dark 令牌为准，
 * 扫描 rust/crates 下的 .rs 源码收集类名，用 Tailwind v4 CLI 编译。
 * 重新生成：bash rust/.tailwind-build/build.sh
 * ===================================================================== */

@import "tailwindcss" source(none);
@import "tw-animate-css";

/* 只扫描 Rust 源码（类名字面量都在 rsx! 里） */
@source "../crates/**/*.rs";

/* 运行时才拼出来的类名，用 inline 白名单兜底 */
@source inline("sr-only");

/* ---------------------------------------------------------------------
 * 翻转数字的进入动画
 *
 * TS 侧用 `motion/react` 的 `AnimatePresence` 做 rotateX + y 的进出场。
 * Rust 侧没有动画库，改成「按字符 key 重建元素 + CSS 关键帧」：
 * 字符变化时旧元素卸载、新元素挂载并播放一次进入动画。
 *
 * 关键帧参数逐字对应 flip-metric-value.tsx 里的
 *   initial={{ rotateX: -90, y: "-0.45em", opacity: 0 }}
 *   animate={{ rotateX: 0,   y: 0,        opacity: 1 }}
 *   transition={{ duration: 0.16, ease: [0.4, 0, 0.2, 1] }}
 * --------------------------------------------------------------------- */
@keyframes zcode-flip-metric-in {
  from {
    transform: rotateX(-90deg) translateY(-0.45em);
    opacity: 0;
  }
  to {
    transform: rotateX(0deg) translateY(0);
    opacity: 1;
  }
}

@utility zcode-flip-metric-in {
  animation: zcode-flip-metric-in 160ms cubic-bezier(0.4, 0, 0.2, 1) both;
  transform-origin: 50% 50%;
}

/* ---------------------------------------------------------------------
 * `scrollbar-hide` 的等价物
 *
 * 上游 styles.css 第 3 行 `@import "tailwind-scrollbar-hide/v4"` 提供这个
 * 工具类，而那个 npm 包不在本仓库里。缺了它，`command-list` 这类
 * 「滚动但不显示滚动条」的容器就会露出原生滚动条。
 *
 * 这里按该插件的实现原样补一条 @utility。
 * --------------------------------------------------------------------- */
@utility scrollbar-hide {
  -ms-overflow-style: none;
  scrollbar-width: none;
  &::-webkit-scrollbar {
    display: none;
  }
}

/* ---------------------------------------------------------------------
 * shadcn/tailwind.css 的等价物
 *
 * 上游 Radix 写的是 data-state="open" / data-orientation="vertical" 这类属性，
 * 而 shadcn 组件源码里用的是 data-open: / data-horizontal: 这类简写。
 * 两者之间的桥就是这几个 @custom-variant —— 缺了它们就会出现
 * 「类名在、效果不在」。
 *
 * 本仓库的 Rust 移植同样只写 data-state / data-orientation / data-highlighted /
 * data-inset / data-disabled，所以这里必须原样补上。
 * --------------------------------------------------------------------- */
@custom-variant dark (&:where(.dark, .dark *));

@custom-variant data-open    (&[data-state="open"]);
@custom-variant data-closed  (&[data-state="closed"]);
@custom-variant data-active  (&[data-state="active"]);
@custom-variant data-inactive (&[data-state="inactive"]);
@custom-variant data-checked (&[data-state="checked"]);
@custom-variant data-unchecked (&[data-state="unchecked"]);

@custom-variant data-horizontal (&[data-orientation="horizontal"]);
@custom-variant data-vertical   (&[data-orientation="vertical"]);

/* 注意：这三组在 TS 侧是「属性存在即生效」，与 Rust 侧写出的空属性值一致 */
@custom-variant data-disabled (&[data-disabled]);
@custom-variant data-highlighted (&[data-highlighted]);
@custom-variant data-inset (&[data-inset]);

/* ---------------------------------------------------------------------
 * 基础层（摘自 packages/ui/src/styles.css 第 68-163 行，去掉了与本应用无关的
 * 容器查询辅助类）
 * --------------------------------------------------------------------- */
html,
body,
#root {
  width: 100%;
  height: 100%;
  min-width: 0;
  min-height: 0;
}

:root {
  --ui-font-size: 14px;
}

body {
  margin: 0;
  overflow: hidden;
  font-family: var(--font-sans);
  background: var(--color-background);
  color: var(--color-foreground);
  -webkit-font-smoothing: antialiased;
}

/* 同上游：清掉系统级 focus 高亮，只保留组件自己的 ring */
*:focus,
*:focus-visible {
  outline: none !important;
  box-shadow: none !important;
  -webkit-box-shadow: none !important;
}

/* ---------------------------------------------------------------------
 * 主题变量：直接抽取 packages/ui/src/styles.css 的 @theme 块
 * --------------------------------------------------------------------- */
CSS

  # @theme 块（含起止大括号）
  sed -n '/^@theme {/,/^}$/p' "$STYLES"

  echo
  echo "/* ---------------------------------------------------------------------"
  echo " * 深色主题覆盖：直接抽取 packages/ui/src/styles.css 的 .dark 块"
  echo " * --------------------------------------------------------------------- */"
  sed -n '/^\.dark {/,/^}$/p' "$STYLES"
} > "$OUT_ENTRY"

mkdir -p "$(dirname "$OUT_CSS")"
"$HERE/node_modules/.bin/tailwindcss" -i "$OUT_ENTRY" -o "$OUT_CSS" --minify

echo "已生成 $OUT_CSS ($(wc -c < "$OUT_CSS") 字节)"
