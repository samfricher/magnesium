export type HeadPlace = {
  folded: boolean;
  shown: boolean;
  replyTop: number | null;
  viewTop: number;
};

export function drawHeadNow(place: HeadPlace): boolean {
  if (place.folded || !place.shown) return true;
  return place.replyTop !== null && place.replyTop <= place.viewTop;
}

export function stillHeld(held: number | null, liveSince: number | null, folded: boolean): boolean {
  return held !== null && held === liveSince && !folded;
}
