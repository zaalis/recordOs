/** What a PC offers a session: its displays (absolute input is mapped to one) and input kinds. */

export interface DisplayInfo {
  id: string;
  name: string;
  width: number;
  height: number;
  x: number;
  y: number;
  refreshHz: number;
  primary: boolean;
}

export interface AgentCaps {
  displays: DisplayInfo[];
  activeDisplayId: string;
  pen: boolean;
  touch: boolean;
}
