// The daily notification, as Settings and a workspace's page offer it. The
// core works out what it says from the record and hands it to the Mac; the
// page only turns it on or off, sets its time, and says what is waiting.

import { useCallback, useEffect, useState } from "react";

import type { Core, Reminder } from "../types";
import { shortDay } from "../words";

/** The notification's state, read once, and a way to change it. */
export function useReminder(api: Core | null) {
  const [reminder, setReminder] = useState<Reminder | null>(null);
  const [asking, setAsking] = useState(false);
  useEffect(() => {
    if (!api) return;
    let live = true;
    void api.reminder().then(
      (got) => live && setReminder(got),
      () => {},
    );
    return () => {
      live = false;
    };
  }, [api]);
  const change = useCallback(
    async (on: boolean, time: string | null = null) => {
      if (!api) return;
      setAsking(true);
      try {
        setReminder(await api.setReminder(on, time));
      } finally {
        setAsking(false);
      }
    },
    [api],
  );
  return { reminder, asking, change };
}

const isoDay = (ms: number) => {
  const date = new Date(ms);
  return `${date.getFullYear()}-${String(date.getMonth() + 1).padStart(2, "0")}-${String(date.getDate()).padStart(2, "0")}`;
};

/** "today at 08:00", "tomorrow at 08:00", "Thu 19 Nov at 09:00". */
export function whenWords(ms: number): string {
  const day = isoDay(ms);
  const today = isoDay(Date.now());
  const tomorrow = isoDay(Date.now() + 86_400_000);
  const time = new Date(ms).toTimeString().slice(0, 5);
  return `${day === today ? "today" : day === tomorrow ? "tomorrow" : shortDay(day)} at ${time}`;
}

/** What stands in the way of a notification arriving, and how to clear it. */
export function Permission({ api, reminder }: { api: Core; reminder: Reminder }) {
  const open = () => void api.openNotificationSettings().catch(() => {});
  if (reminder.permission === "unavailable") return <span className="setting-note">Notifications work only in the installed app.</span>;
  if (reminder.permission === "denied")
    return (
      <span className="setting-note">
        Notifications from Magnesium are off in System Settings.{" "}
        <button className="plain" onClick={open}>
          Open Notification Settings…
        </button>
      </span>
    );
  if (reminder.permission === "quiet" && reminder.on)
    return (
      <span className="setting-note">
        They arrive quietly, in Notification Center only.{" "}
        <button className="plain" onClick={open}>
          Open Notification Settings…
        </button>
      </span>
    );
  return null;
}
