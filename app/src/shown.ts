export function whenShown(element: HTMLElement, read: () => void): () => void {
  if (element.checkVisibility?.() !== false) {
    read();
    return () => {};
  }
  let frame = 0;
  const watch = new ResizeObserver(() => {
    if (element.checkVisibility?.() === false) return;
    watch.disconnect();
    frame = requestAnimationFrame(read);
  });
  watch.observe(element);
  return () => {
    watch.disconnect();
    cancelAnimationFrame(frame);
  };
}
