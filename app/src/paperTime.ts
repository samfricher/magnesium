export const DISC_R = 21;

export function shareLeft(minutesLeft: number, minutes: number): number {
  if (!(minutes > 0)) return 0;
  return Math.max(0, Math.min(1, minutesLeft / minutes));
}

export function discFill(share: number): "whole" | "none" | { path: string } {
  if (share >= 1) return "whole";
  if (share <= 0) return "none";
  const turned = (1 - share) * 2 * Math.PI;
  const x = round(24 + DISC_R * Math.sin(turned));
  const y = round(24 - DISC_R * Math.cos(turned));
  return { path: `M24 24 L${x} ${y} A${DISC_R} ${DISC_R} 0 ${share > 0.5 ? 1 : 0} 1 24 ${24 - DISC_R} Z` };
}

const round = (n: number) => Math.round(n * 1000) / 1000;
