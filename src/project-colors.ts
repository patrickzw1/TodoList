export const PROJECT_COLOR_PRESETS = [
  { value: "#1665e8", label: "蓝色" },
  { value: "#7c5ce7", label: "紫色" },
  { value: "#269b58", label: "绿色" },
  { value: "#db7b22", label: "橙色" },
  { value: "#d84b6b", label: "玫红色" },
  { value: "#258ca6", label: "青色" },
  { value: "#ff0000", label: "大红色" },
] as const;

export const PROJECT_COLORS = PROJECT_COLOR_PRESETS.map((preset) => preset.value);

export function normalizeProjectColor(value: string): string | null {
  const trimmed = value.trim().toLocaleLowerCase();
  const short = /^#([0-9a-f]{3})$/.exec(trimmed);
  if (short) return `#${[...short[1]].map((digit) => digit.repeat(2)).join("")}`;
  return /^#[0-9a-f]{6}$/.test(trimmed) ? trimmed : null;
}

export function isPresetProjectColor(value: string) {
  return PROJECT_COLORS.some((preset) => preset === value);
}
