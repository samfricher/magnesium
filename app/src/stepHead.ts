// When a step's head is set into the lesson. The window hears which step a turn began only after its reply has
// ended, while the learner may be reading that reply: a head set in above it then would push it down under their
// eyes, the kind of motion the steady line (#121) took out. So a head never moves anything in view.

/** Where the page stands when the window hears that the turn just ended began a step. */
export type HeadPlace = {
  /** The page is the task alone: the lesson, and the head with it, is hidden. */
  folded: boolean;
  /** The session is the tab in front, and drawn. */
  shown: boolean;
  /** The top of that turn's reply on screen, or null when it has none drawn. */
  replyTop: number | null;
  /** The top of the lesson's view on screen. */
  viewTop: number;
};

/**
 * Whether the head can be drawn now. It can when nothing of it would be seen: the lesson is hidden, or the head
 * would sit wholly above the view (the reply's top is at or above the view's top), where the page keeps the reply
 * still by scrolling on by the head's height. Otherwise it is held.
 */
export function drawHeadNow(place: HeadPlace): boolean {
  if (place.folded || !place.shown) return true;
  return place.replyTop !== null && place.replyTop <= place.viewTop;
}

/**
 * Whether a head held for the turn begun at `held` is still held. It is drawn at the learner's next action that
 * already changes the page: the page folding to the task as they start writing, or their next message (the turn
 * on screen is then another).
 */
export function stillHeld(held: number | null, liveSince: number | null, folded: boolean): boolean {
  return held !== null && held === liveSince && !folded;
}
