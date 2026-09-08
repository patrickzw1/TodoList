const DAY_MS = 24 * 60 * 60 * 1000;
export const UNSCHEDULED_DUE_DATE = "9999-12-31";

function parseIsoDate(value: string) {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const leapYear = year % 4 === 0 && (year % 100 !== 0 || year % 400 === 0);
  const daysInMonth = [31, leapYear ? 29 : 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
  if (year < 1 || month < 1 || month > 12 || day < 1 || day > daysInMonth[month - 1]) return null;
  return { year, month, day };
}

function calendarDateValue(date: { year: number; month: number; day: number }) {
  return date.year * 10_000 + date.month * 100 + date.day;
}

function calendarDateUtc(date: { year: number; month: number; day: number }) {
  const value = new Date(0);
  value.setUTCFullYear(date.year, date.month - 1, date.day);
  value.setUTCHours(0, 0, 0, 0);
  return value.getTime();
}

export function normalizeOptionalDueDate(value: string) {
  return value.trim() || UNSCHEDULED_DUE_DATE;
}

export function isDueDateOnOrBefore(dueDate: string, today: string) {
  if (dueDate === UNSCHEDULED_DUE_DATE) return false;
  const due = parseIsoDate(dueDate);
  const current = parseIsoDate(today);
  return Boolean(due && current && calendarDateValue(due) <= calendarDateValue(current));
}

export function localIsoDate(date = new Date()) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function todayHeading(date = new Date()) {
  const weekday = "日一二三四五六"[date.getDay()];
  return `${date.getMonth() + 1}月${date.getDate()}日 周${weekday}`;
}

export function taskDueLabel(dueDate: string, fallback: string, today = new Date()) {
  if (dueDate === UNSCHEDULED_DUE_DATE) return "未安排";
  const parsed = parseIsoDate(dueDate);
  if (!parsed) return fallback || "未安排";
  const todayUtc = Date.UTC(today.getFullYear(), today.getMonth(), today.getDate());
  const dueUtc = calendarDateUtc(parsed);
  const days = Math.round((dueUtc - todayUtc) / DAY_MS);
  if (days === 0) return "今天";
  if (days === 1) return "明天";
  if (days < 0) return `逾期 ${Math.abs(days)} 天`;
  return `${parsed.month}月${parsed.day}日`;
}

export function isOverdue(dueDate: string, today = new Date()) {
  const parsed = parseIsoDate(dueDate);
  if (!parsed || dueDate === UNSCHEDULED_DUE_DATE) return false;
  const dueUtc = calendarDateUtc(parsed);
  const todayUtc = Date.UTC(today.getFullYear(), today.getMonth(), today.getDate());
  return dueUtc < todayUtc;
}
