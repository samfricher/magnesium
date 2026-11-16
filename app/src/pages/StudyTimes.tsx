// The plans' planned sessions on a workspace's page: the times the learner
// chose to practise, each in its own words, and the one action they offer,
// adding them to the learner's calendar. The runtime reads them from the plans
// (`magnesium calendar`), as it writes them into the calendar file, so the
// page shows what the file holds. A plan with no session lines shows nothing.

import { Line } from "../surface/Rich";
import type { Planned, PlannedSession } from "../types";

const DAY_ORDER = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
const SHORT: Record<string, string> = { MO: "Mon", TU: "Tue", WE: "Wed", TH: "Thu", FR: "Fri", SA: "Sat", SU: "Sun" };
const LONG: Record<string, string> = { MO: "Mondays", TU: "Tuesdays", WE: "Wednesdays", TH: "Thursdays", FR: "Fridays", SA: "Saturdays", SU: "Sundays" };

/** A session's days as a person says them: "Weekdays", "Every day", "Saturdays", "Tue and Thu", "Mon, Wed and Fri". */
export function daysWords(days: string[]): string {
  const set = DAY_ORDER.filter((day) => days.includes(day));
  const key = set.join(",");
  if (set.length === 7) return "Every day";
  if (key === "MO,TU,WE,TH,FR") return "Weekdays";
  if (key === "SA,SU") return "Weekends";
  if (set.length === 1) return LONG[set[0]];
  const names = set.map((day) => SHORT[day]);
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}

/** "Weekdays, 08:00" or "Saturdays, 10:00–11:30". */
export function whenWords(session: PlannedSession): string {
  return `${daysWords(session.days)}, ${session.end ? `${session.start}–${session.end}` : session.start}`;
}

type Props = {
  planned: Planned;
  busy: boolean;
  onAdd: () => void;
};

export function StudyTimes({ planned, busy, onAdd }: Props) {
  const { sessions, unreadable } = planned;
  const unreadablePlans = planned.unreadable_plans ?? 0;
  if (sessions.length === 0 && unreadable === 0 && unreadablePlans === 0) return null;
  // With two plans, each row says which plan its time is for.
  const plans = new Set(sessions.map((session) => session.plan));
  return (
    <section aria-label="When you study" className="study-times">
      <p className="list-head">When you study</p>
      {sessions.length > 0 && (
        <div className="list" role="list">
          {sessions.map((session, at) => (
            <div key={at} role="listitem" className="home-row static study-time">
              <span className="row-name study-time-what">
                <Line text={plans.size > 1 ? `${session.title ?? session.plan}: ${session.action}` : session.action} />
              </span>
              <span className="row-when">{whenWords(session)}</span>
            </div>
          ))}
        </div>
      )}
      {unreadable > 0 && (
        <p className="page-note">
          {unreadable === 1
            ? "One session in your plan could not be read and is left out. Ask the tutor to fix it."
            : `${unreadable} sessions in your plan could not be read and are left out. Ask the tutor to fix them.`}
        </p>
      )}
      {unreadablePlans > 0 && (
        <p className="page-note">
          {unreadablePlans === 1
            ? "One of your plans could not be read, so its sessions are left out. Ask the tutor to fix it."
            : `${unreadablePlans} of your plans could not be read, so their sessions are left out. Ask the tutor to fix them.`}
        </p>
      )}
      {sessions.length > 0 && (
        <p className="page-acts">
          <button className="quiet-act" data-drive="add-to-calendar" disabled={busy} onClick={onAdd}>
            Add to Calendar…
          </button>
        </p>
      )}
    </section>
  );
}

