// Mirrors the Rust payloads in src-tauri/src/lib.rs (camelCase).

export type PaceStatus = 'under' | 'near' | 'over' | 'unlimited' | 'unknown';

export interface Settings {
  initialized: boolean;
  resetDay: number;
  /** 0 = Sunday … 6 = Saturday */
  workDays: boolean[];
  workStart: string;
  workEnd: string;
  /** e.g. "IL", "GB-ENG", "US-CA" */
  holidayRegions: string[];
  holidayTypes: string[];
  customDaysOff: string[];
  excludedDays: string[];
  refreshMinutes: number;
  notifyOverPace: boolean;
  overMargin: number;
  proxyMode: 'auto' | 'manual' | 'none';
  proxyUrl: string;
  showPercentInMenuBar: boolean;
}

export interface Usage {
  usedPct: number;
  unlimited: boolean;
  entitlement: number | null;
  remaining: number | null;
  overageCount: number | null;
  quotaName: string;
  plan: string | null;
  login: string | null;
  githubResetDate: string | null;
  fetchedAt: number;
}

export interface PaceView {
  expectedPct: number;
  diffPct: number | null;
  status: PaceStatus;
  cycleStart: string;
  cycleEnd: string;
  totalWorkingDays: number;
  elapsedWorkingDays: number;
  remainingWorkingDays: number;
  dailyBudgetPct: number | null;
  projectedPct: number | null;
  todayIsWorkingDay: boolean;
  calendarFallback: boolean;
  nowX: number;
  cycleMinutes: number;
}

export interface Status {
  usage: Usage | null;
  pace: PaceView;
  history: [number, number][];
  curve: [number, number][];
  auth: { source: 'device' | 'ghCli' | null; login: string | null };
  error: string | null;
  fetching: boolean;
  settings: Settings;
  network: { mode: string; proxy: string | null; source: string | null };
}

export interface DeviceCode {
  deviceCode: string;
  userCode: string;
  verificationUri: string;
  expiresIn: number;
  interval: number;
}
