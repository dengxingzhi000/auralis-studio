/**
 * 把某张卡片滚进视野。
 *
 * 中央画布是可纵向滚动的（窗口变矮时图表不会被压成条带），侧栏的
 * 导航项需要能把对应的卡片带进来。
 *
 * 用**瞬时**滚动而不是平滑滚动：平滑滚动是一次动画，页面不可见时
 * （窗口最小化、被完全遮挡）没有帧来推进它，滚动就停在原地 ——
 * 导航项会变成「点了没反应」。瞬时滚动没有这个依赖。
 *
 * 立刻滚一次，下一帧再滚一次：刚打开文件时卡片可能还没布局完，
 * 只滚一次会滚不到位。
 */
export function scrollToPane(id: string): void {
  const scroll = () => {
    document
      .getElementById(id)
      ?.scrollIntoView({ behavior: "instant", block: "nearest" });
  };
  scroll();
  window.requestAnimationFrame(scroll);
}
