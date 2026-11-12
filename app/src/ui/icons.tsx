// The window's icons: one weight, one grid, drawn here so the page loads no
// icon font and makes no request for one.

const PATHS: Record<string, string[]> = {
  close: ["M6 6l12 12", "M18 6L6 18"],
  back: ["M15 5l-7 7 7 7"],
  down: ["M6 9l6 6 6-6"],
  up: ["M6 15l6-6 6 6"],
  right: ["M9 5l7 7-7 7"],
  plus: ["M12 5v14", "M5 12h14"],
  check: ["M5 12.5l4.5 4.5L19 7.5"],
  send: ["M12 19V5", "M6 11l6-6 6 6"],
  stop: ["M7.5 7.5h9v9h-9z"],
  clip: ["M20 11.5l-8.2 8.2a5 5 0 01-7.1-7.1l8.6-8.6a3.4 3.4 0 014.8 4.8l-8.5 8.5a1.7 1.7 0 01-2.4-2.4l7.8-7.8"],
  sigma: ["M17 6V5H7l6 7-6 7h10v-1"],
  folder: ["M3.5 7.5a2 2 0 012-2h4l2 2.5h7a2 2 0 012 2v7.5a2 2 0 01-2 2h-13a2 2 0 01-2-2z"],
  search: ["M11 18a7 7 0 100-14 7 7 0 000 14z", "M20 20l-4-4"],
  flame: ["M12 21c3.6 0 6.5-2.6 6.5-6.3 0-2.6-1.5-4.4-2.8-6-.6 1.2-1.3 1.9-2.2 2.3.2-3-1-5.6-3.5-7.5.2 2.6-.9 4.300-2.200 5.900C6.500 11 5.500 12.600 5.500 14.700 5.500 18.400 8.400 21 12 21z"],
  gear: ["M4 7h9", "M17 7h3", "M15 9.200a2.200 2.200 0 100-4.400 2.200 2.200 0 000 4.400z", "M4 17h3", "M11 17h9", "M9 19.200a2.200 2.200 0 100-4.400 2.200 2.200 0 000 4.400z"],
  paper: ["M7 3.500h7l4 4V20a.5.5 0 01-.5.500h-10.500a.5.5 0 01-.5-.5V4a.5.5 0 01.5-.5z", "M14 3.500V8h4", "M9.500 12.500h5", "M9.500 16h5"],
  // The proof mark: one dot earned, two to earn. A path that starts "!" is filled.
  proof: ["!M1.900 12a2.600 2.600 0 105.200 0 2.600 2.600 0 10-5.200 0", "M9.400 12a2.600 2.600 0 105.200 0 2.600 2.600 0 10-5.200 0", "M16.900 12a2.600 2.600 0 105.200 0 2.600 2.600 0 10-5.200 0"],
  more: ["M6 12h.01", "M12 12h.01", "M18 12h.01"],
  history: ["M4 12a8 8 0 108-8 8 8 0 00-6.300 3", "M4 4.500V8h3.500", "M12 8v4.500l3 1.800"],
  flag: ["M6 21V4", "M6 5h11l-2 3.500 2 3.500H6"],
  play: ["M8 5.500v13l10.500-6.500z"],
  camera: ["M4 8.500a2 2 0 012-2h1.500l1.500-2h6l1.500 2H18a2 2 0 012 2V17a2 2 0 01-2 2H6a2 2 0 01-2-2z", "M12 16a3.200 3.200 0 100-6.400 3.200 3.200 0 000 6.400z"],
  external: ["M14 5h5v5", "M19 5l-8 8", "M18 14v4a1 1 0 01-1 1H6a1 1 0 01-1-1V7a1 1 0 011-1h4"],
  // A fault: an exclamation mark on its own, no triangle and no disc.
  bang: ["!M10.900 5.200a1.100 1.100 0 012.200 0v7.600a1.100 1.100 0 01-2.200 0z", "!M12 20.200a1.500 1.500 0 100-3 1.500 1.500 0 000 3z"],
  warn: ["M12 4l9 15.500H3z", "M12 10v4.500", "M12 17.300h.01"],
  lock: ["M7 11V8a5 5 0 0110 0v3", "M6 11h12v9H6z"],
  trash: ["M5 7h14", "M10 7V5h4v2", "M7 7l1 12h8l1-12", "M10.500 11v5", "M13.500 11v5"],
  timer: ["M12 20a7 7 0 100-14 7 7 0 000 14z", "M12 9.500V13l2.200 1.300", "M10 3h4"],
  keys: ["M3.500 7h17v10h-17z", "M7 10.500h.01", "M10.500 10.500h.01", "M14 10.500h.01", "M17.500 10.500h.01", "M8 14h8"],
  dash: ["M7 12h10"],
  // Due: a clock face. A line glyph, never a filled disc, so it is not taken for a proof.
  clock: ["M12 20a8 8 0 100-16 8 8 0 000 16z", "M12 8v4.500l3 1.800"],
  calendar: ["M5 6.500h14a1 1 0 011 1V19a1 1 0 01-1 1H5a1 1 0 01-1-1V7.500a1 1 0 011-1z", "M4 10.500h16", "M8.500 4v4", "M15.500 4v4"],
  list: ["M9 7h11", "M9 12h11", "M9 17h11", "M4.500 7h.01", "M4.500 12h.01", "M4.500 17h.01"],
};

export type IconName = keyof typeof PATHS;

export function Icon({ name, size = 18, fill = false }: { name: string; size?: number; fill?: boolean }) {
  const paths = PATHS[name] ?? [];
  return (
    <svg
      className="icon"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={fill ? "currentColor" : "none"}
      stroke="currentColor"
      strokeWidth={fill ? 0 : 1.7}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {paths.map((d) =>
        d.startsWith("!") ? <path key={d} d={d.slice(1)} fill="currentColor" /> : <path key={d} d={d} />,
      )}
    </svg>
  );
}
