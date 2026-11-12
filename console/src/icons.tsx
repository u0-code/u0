// Line icons, drawn on a 20 by 20 grid. The vocabulary is a person's life
// (day and night, a notebook, people, a place) rather than a developer's
// tools; only the window chrome borrows from the usual set.

import type { ReactNode, SVGProps } from "react";

function Icon({ children, ...props }: SVGProps<SVGSVGElement> & { children: ReactNode }) {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 20 20"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      {...props}
    >
      {children}
    </svg>
  );
}

type P = SVGProps<SVGSVGElement>;

/** She is awake. */
export const Sun = (p: P) => (
  <Icon {...p}>
    <circle cx="10" cy="10" r="3.2" />
    <path d="M10 2.6v1.6M10 15.8v1.6M2.6 10h1.6M15.8 10h1.6M4.8 4.8l1.1 1.1M14.1 14.1l1.1 1.1M4.8 15.2l1.1-1.1M14.1 5.9l1.1-1.1" />
  </Icon>
);
/** She is asleep. */
export const Moon = (p: P) => (
  <Icon {...p}>
    <path d="M16 11.6A6.4 6.4 0 1 1 8.4 4a5 5 0 0 0 7.6 7.6z" />
  </Icon>
);
/** Her journal: a bound notebook. */
export const Notebook = (p: P) => (
  <Icon {...p}>
    <path d="M6 3.5h8a1.5 1.5 0 0 1 1.5 1.5v10a1.5 1.5 0 0 1-1.5 1.5H6z" />
    <path d="M6 3.5v13M4.2 6.5H6M4.2 10H6M4.2 13.5H6M9 7.5h3.5M9 10.5h3.5" />
  </Icon>
);
/** The people she knows. */
export const People = (p: P) => (
  <Icon {...p}>
    <circle cx="8" cy="7.4" r="2.5" />
    <path d="M3.4 16c0-2.6 2-4.3 4.6-4.3s4.6 1.7 4.6 4.3M13 5.2a2.4 2.4 0 0 1 0 4.5M14.6 12.1c1.4.6 2.2 1.9 2.2 3.6" />
  </Icon>
);
/** A private conversation with one person. */
export const Person = (p: P) => (
  <Icon {...p}>
    <circle cx="10" cy="7.3" r="2.8" />
    <path d="M4.6 16.4c0-3 2.3-5 5.4-5s5.4 2 5.4 5" />
  </Icon>
);
/** A thread anyone can read. */
export const Public = (p: P) => (
  <Icon {...p}>
    <circle cx="10" cy="10" r="6.5" />
    <path d="M3.5 10h13M10 3.5c2.2 2 2.2 11 0 13M10 3.5c-2.2 2-2.2 11 0 13" />
  </Icon>
);
/** Your own conversation with her. */
export const Talk = (p: P) => (
  <Icon {...p}>
    <path d="M3.5 5.5a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H9l-3.5 3v-3a2 2 0 0 1-2-2z" />
  </Icon>
);
/** Her body: what she is plugged into. */
export const Plug = (p: P) => (
  <Icon {...p}>
    <path d="M7.5 3v4M12.5 3v4M5.5 7h9v2.5a4.5 4.5 0 0 1-9 0zM10 14v3" />
  </Icon>
);
export const Search = (p: P) => (
  <Icon {...p}>
    <circle cx="9" cy="9" r="5" />
    <path d="m13 13 3.5 3.5" />
  </Icon>
);
export const Sidebar = (p: P) => (
  <Icon {...p}>
    <rect x="3" y="4" width="14" height="12" rx="2.5" />
    <path d="M8 4v12" />
  </Icon>
);
export const Panel = (p: P) => (
  <Icon {...p}>
    <rect x="3" y="4" width="14" height="12" rx="2.5" />
    <path d="M12 4v12" />
  </Icon>
);
export const ArrowUp = (p: P) => (
  <Icon strokeWidth="1.8" {...p}>
    <path d="M10 15.5v-11M5.5 9 10 4.5 14.5 9" />
  </Icon>
);
export const Eye = (p: P) => (
  <Icon {...p}>
    <path d="M2.5 10s2.8-5 7.5-5 7.5 5 7.5 5-2.8 5-7.5 5-7.5-5-7.5-5z" />
    <circle cx="10" cy="10" r="2" />
  </Icon>
);
export const Seal = (p: P) => (
  <Icon {...p}>
    <circle cx="10" cy="8.5" r="4.5" />
    <path d="m7.5 12.3-1 4.7 3.5-1.8 3.5 1.8-1-4.7" />
  </Icon>
);
