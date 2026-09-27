/** Line icon set — 24px grid, 1.75px stroke, round caps. Drawn for Vision-Pro. */
import type { ReactElement, SVGProps } from 'react';

export type IconName =
  | 'home'
  | 'display'
  | 'displays'
  | 'pointer'
  | 'trackpad'
  | 'touch'
  | 'pencil'
  | 'keyboard'
  | 'command'
  | 'gauge'
  | 'wifi'
  | 'gear'
  | 'power'
  | 'close'
  | 'chevron-right'
  | 'chevron-left'
  | 'chevron-down'
  | 'plus'
  | 'minus'
  | 'qr'
  | 'lock'
  | 'shield'
  | 'check'
  | 'check-circle'
  | 'alert'
  | 'info'
  | 'eye'
  | 'eye-off'
  | 'grip'
  | 'collapse'
  | 'expand'
  | 'fullscreen'
  | 'refresh'
  | 'trash'
  | 'edit'
  | 'stats'
  | 'computer'
  | 'tablet'
  | 'link'
  | 'clock'
  | 'user'
  | 'key'
  | 'layers'
  | 'eraser'
  | 'undo'
  | 'redo'
  | 'zoom-in'
  | 'zoom-out'
  | 'rotate'
  | 'sparkle'
  | 'network'
  | 'globe'
  | 'devices'
  | 'sliders'
  | 'terminal'
  | 'camera'
  | 'hand'
  | 'arrow-up'
  | 'cable'
  | 'dots';

const paths: Record<IconName, ReactElement> = {
  home: <path d="M4 10.5 12 4l8 6.5V19a1 1 0 0 1-1 1h-4.5v-5.5h-5V20H5a1 1 0 0 1-1-1z" />,
  display: (
    <>
      <rect x="3" y="4" width="18" height="12.5" rx="2" />
      <path d="M9 20h6M12 16.5V20" />
    </>
  ),
  displays: (
    <>
      <rect x="2.5" y="6" width="13" height="9.5" rx="1.6" />
      <path d="M8.5 3.5h11a2 2 0 0 1 2 2V13M6.5 19h5M9 15.5V19" />
    </>
  ),
  pointer: <path d="m6 3.5 12.5 7.2-5.6 1.4-2.6 5.3z" />,
  trackpad: (
    <>
      <rect x="3.5" y="4.5" width="17" height="15" rx="3" />
      <path d="M3.5 15.5h17M12 15.5v4" />
    </>
  ),
  touch: (
    <path d="M9 11V5.5a1.75 1.75 0 0 1 3.5 0V10m0-.5V8.75a1.75 1.75 0 0 1 3.5 0V11m0-1a1.75 1.75 0 0 1 3.5 0v4.5a6.5 6.5 0 0 1-6.5 6.5h-1a6 6 0 0 1-4.6-2.15L4.2 14.5a1.6 1.6 0 0 1 2.4-2.1L9 14.5" />
  ),
  pencil: <path d="m15.5 5.5 3 3M4 20l1-4.5L16.2 4.3a1.8 1.8 0 0 1 2.5 0l1 1a1.8 1.8 0 0 1 0 2.5L8.5 19z" />,
  keyboard: (
    <>
      <rect x="2.5" y="6" width="19" height="12" rx="2" />
      <path d="M6.5 10h.01M10 10h.01M13.5 10h.01M17 10h.01M6.5 13.5h.01M17 13.5h.01M9.5 14h5" />
    </>
  ),
  command: <path d="M9 9V6.5A2.5 2.5 0 1 0 6.5 9H9zm0 0h6m-6 0v6m6-6V6.5A2.5 2.5 0 1 1 17.5 9H15zm0 0v6m0 0h-6m6 0v2.5a2.5 2.5 0 1 0 2.5-2.5H15zm-6 0H6.5A2.5 2.5 0 1 0 9 17.5V15z" />,
  gauge: (
    <>
      <path d="M4.5 17.5a8.5 8.5 0 1 1 15 0" />
      <path d="m12 13.5 4-4.5" />
      <circle cx="12" cy="14" r="1.3" />
    </>
  ),
  wifi: <path d="M2.5 9a14 14 0 0 1 19 0M5.5 12.2a9.5 9.5 0 0 1 13 0M8.6 15.3a5 5 0 0 1 6.8 0M12 18.6h.01" />,
  gear: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 13.5a7.7 7.7 0 0 0 0-3l2-1.6-2-3.4-2.4.9a7.6 7.6 0 0 0-2.6-1.5L14 2.4h-4l-.4 2.5A7.6 7.6 0 0 0 7 6.4l-2.4-.9-2 3.4 2 1.6a7.7 7.7 0 0 0 0 3l-2 1.6 2 3.4 2.4-.9a7.6 7.6 0 0 0 2.6 1.5l.4 2.5h4l.4-2.5a7.6 7.6 0 0 0 2.6-1.5l2.4.9 2-3.4z" />
    </>
  ),
  power: <path d="M12 3v8M6.4 6.5a8 8 0 1 0 11.2 0" />,
  close: <path d="M6 6l12 12M18 6 6 18" />,
  'chevron-right': <path d="m9.5 5.5 6.5 6.5-6.5 6.5" />,
  'chevron-left': <path d="M14.5 5.5 8 12l6.5 6.5" />,
  'chevron-down': <path d="m5.5 9.5 6.5 6.5 6.5-6.5" />,
  plus: <path d="M12 5v14M5 12h14" />,
  minus: <path d="M5 12h14" />,
  qr: (
    <>
      <rect x="4" y="4" width="6" height="6" rx="1" />
      <rect x="14" y="4" width="6" height="6" rx="1" />
      <rect x="4" y="14" width="6" height="6" rx="1" />
      <path d="M14 14h2.5v2.5M20 14v.01M14 20h.01M17 20h3v-3" />
    </>
  ),
  lock: (
    <>
      <rect x="4.5" y="10.5" width="15" height="10" rx="2" />
      <path d="M8 10.5V8a4 4 0 0 1 8 0v2.5" />
    </>
  ),
  shield: <path d="M12 3 4.5 6v5.5c0 4.6 3.2 8.3 7.5 9.5 4.3-1.2 7.5-4.9 7.5-9.5V6z" />,
  check: <path d="m5 12.5 4.5 4.5L19 7.5" />,
  'check-circle': (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="m8 12.3 2.8 2.8L16.2 9.5" />
    </>
  ),
  alert: (
    <>
      <path d="M10.3 4.2 2.8 17.5A2 2 0 0 0 4.5 20.5h15a2 2 0 0 0 1.7-3L13.7 4.2a2 2 0 0 0-3.4 0z" />
      <path d="M12 9.5v4M12 17h.01" />
    </>
  ),
  info: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 11v5.5M12 7.8h.01" />
    </>
  ),
  eye: (
    <>
      <path d="M2.5 12S6 5.5 12 5.5 21.5 12 21.5 12 18 18.5 12 18.5 2.5 12 2.5 12z" />
      <circle cx="12" cy="12" r="3" />
    </>
  ),
  'eye-off': <path d="M4 4l16 16M10.6 5.6A10 10 0 0 1 12 5.5c6 0 9.5 6.5 9.5 6.5a17 17 0 0 1-2.9 3.7M6.6 6.7C3.9 8.5 2.5 12 2.5 12S6 18.5 12 18.5a9.6 9.6 0 0 0 4.2-.9M9.9 9.9a3 3 0 0 0 4.2 4.2" />,
  grip: <path d="M9 6h.01M15 6h.01M9 12h.01M15 12h.01M9 18h.01M15 18h.01" strokeWidth="3" />,
  collapse: <path d="M4 14h6v6M20 10h-6V4M14 10l7-7M3 21l7-7" />,
  expand: <path d="M15 3h6v6M9 21H3v-6M21 3l-7 7M3 21l7-7" />,
  fullscreen: <path d="M4 9V5a1 1 0 0 1 1-1h4M15 4h4a1 1 0 0 1 1 1v4M20 15v4a1 1 0 0 1-1 1h-4M9 20H5a1 1 0 0 1-1-1v-4" />,
  refresh: <path d="M20 11a8 8 0 0 0-14.3-4.9L4 8M4 4v4h4M4 13a8 8 0 0 0 14.3 4.9L20 16m0 4v-4h-4" />,
  trash: <path d="M4.5 6.5h15M9.5 6.5V4.5h5v2M6.5 6.5l1 13a1 1 0 0 0 1 .9h7a1 1 0 0 0 1-.9l1-13M10 10.5v6M14 10.5v6" />,
  edit: <path d="M12 20h8.5M16.5 3.8a2 2 0 0 1 2.8 2.8L7.5 18.4 3.5 19.5l1.1-4z" />,
  stats: <path d="M4 20V10M10 20V4M16 20v-7M21 20H3" />,
  computer: (
    <>
      <rect x="3" y="4" width="18" height="12" rx="1.8" />
      <path d="M2 20h20" />
    </>
  ),
  tablet: (
    <>
      <rect x="4.5" y="2.5" width="15" height="19" rx="2.5" />
      <path d="M11 18.5h2" />
    </>
  ),
  link: <path d="M10 14a4.5 4.5 0 0 0 6.4 0l3-3a4.5 4.5 0 0 0-6.4-6.4l-1.2 1.2M14 10a4.5 4.5 0 0 0-6.4 0l-3 3a4.5 4.5 0 0 0 6.4 6.4l1.2-1.2" />,
  clock: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M12 7v5l3 2" />
    </>
  ),
  user: (
    <>
      <circle cx="12" cy="8" r="4" />
      <path d="M4.5 20.5a7.5 7.5 0 0 1 15 0" />
    </>
  ),
  key: (
    <>
      <circle cx="8" cy="15" r="4.5" />
      <path d="m11.2 11.8 8.8-8.8M16.5 6.5l2.5 2.5M14 9l2 2" />
    </>
  ),
  layers: <path d="m12 3 9 5-9 5-9-5zm-9 9 9 5 9-5M3 16l9 5 9-5" />,
  eraser: <path d="M8 20h12M5.4 15.4l9.2-9.2a2 2 0 0 1 2.8 0l2.4 2.4a2 2 0 0 1 0 2.8L12 19.2A2.8 2.8 0 0 1 8 19.2l-2.6-2.6a.85.85 0 0 1 0-1.2zM9 11.8l5 5" />,
  undo: <path d="M9 14 4 9l5-5M4 9h10.5a5.5 5.5 0 0 1 0 11H11" />,
  redo: <path d="m15 14 5-5-5-5M20 9H9.5a5.5 5.5 0 0 0 0 11H13" />,
  'zoom-in': (
    <>
      <circle cx="10.5" cy="10.5" r="6.5" />
      <path d="m20 20-4.8-4.8M10.5 8v5M8 10.5h5" />
    </>
  ),
  'zoom-out': (
    <>
      <circle cx="10.5" cy="10.5" r="6.5" />
      <path d="m20 20-4.8-4.8M8 10.5h5" />
    </>
  ),
  rotate: <path d="M20 12a8 8 0 1 1-2.3-5.7L20 8.5M20 4v4.5h-4.5" />,
  sparkle: <path d="M12 3.5c.6 4.4 2.6 6.9 8.5 8.5-5.9 1.6-7.9 4.1-8.5 8.5-.6-4.4-2.6-6.9-8.5-8.5 5.9-1.6 7.9-4.1 8.5-8.5z" />,
  network: (
    <>
      <rect x="9" y="3" width="6" height="5" rx="1" />
      <rect x="3" y="16" width="6" height="5" rx="1" />
      <rect x="15" y="16" width="6" height="5" rx="1" />
      <path d="M12 8v4M6 16v-2h12v2" />
    </>
  ),
  globe: (
    <>
      <circle cx="12" cy="12" r="9" />
      <path d="M3 12h18M12 3a14 14 0 0 1 0 18M12 3a14 14 0 0 0 0 18" />
    </>
  ),
  devices: (
    <>
      <rect x="2.5" y="4" width="13" height="10" rx="1.5" />
      <path d="M1.5 17.5h11" />
      <rect x="16" y="8" width="6" height="12" rx="1.5" />
    </>
  ),
  sliders: <path d="M4 7h9M17 7h3M4 17h3M11 17h9M15 4.5v5M9 14.5v5" />,
  terminal: (
    <>
      <rect x="3" y="4" width="18" height="16" rx="2" />
      <path d="m7 9 3 3-3 3M12.5 15H17" />
    </>
  ),
  camera: (
    <>
      <path d="M4 7.5h3l1.5-2.5h7L17 7.5h3a1 1 0 0 1 1 1V18a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1V8.5a1 1 0 0 1 1-1z" />
      <circle cx="12" cy="13" r="3.5" />
    </>
  ),
  hand: <path d="M8 12.5V6a1.5 1.5 0 0 1 3 0v5.5-7a1.5 1.5 0 0 1 3 0v7-5a1.5 1.5 0 0 1 3 0v8a7 7 0 0 1-7 7h-.5A6.5 6.5 0 0 1 4 16l-.8-3a1.5 1.5 0 0 1 2.7-1.2L8 14.5" />,
  'arrow-up': <path d="M12 19V5M5.5 11.5 12 5l6.5 6.5" />,
  cable: <path d="M10 2.5h4v3h-4zM8.5 5.5h7v4.5a2 2 0 0 1-2 2h-3a2 2 0 0 1-2-2zM12 12v3a3 3 0 0 1-3 3H7.5a2 2 0 0 0-2 2v1.5" />,
  dots: <path d="M5 12h.01M12 12h.01M19 12h.01" strokeWidth="3" />,
};

export interface IconProps extends SVGProps<SVGSVGElement> {
  name: IconName;
  size?: number;
}

export function Icon({ name, size = 22, strokeWidth = 1.75, ...rest }: IconProps) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth={strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
      {...rest}
    >
      {paths[name]}
    </svg>
  );
}
