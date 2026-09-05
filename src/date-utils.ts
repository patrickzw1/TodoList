const DAY_MS = 24 * 60 * 60 * 1000;

function parseIsoDate(value: string) {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  const year = Number(match[1]);
  const month = Number(match[2]);
  const day = Number(match[3]);
  const date = new Date(year, month - 1, day);
  if (date.getFullYear() !== year || date.getMonth() !== month - 1 || date.getDate() !== day) return null;
  return { year, month, day };
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
  if (dueDate === "9999-12-31") return "未安排";
  const parsed = parseIsoDate(dueDate);
  if (!parsed) return fallback || "未安排";
  const todayUtc = Date.UTC(today.getFullYear(), today.getMonth(), today.getDate());
  const dueUtc = Date.UTC(parsed.year, parsed.month - 1, parsed.day);
  const days = Math.round((dueUtc - todayUtc) / DAY_MS);
  if (days === 0) return "今天";
  if (days === 1) return "明天";
  if (days < 0) return `逾期 ${Math.abs(days)} 天`;
  return `${parsed.month}月${parsed.day}日`;
}

export function isOverdue(dueDate: string, today = new Date()) {
  const parsed = parseIsoDate(dueDate);
  if (!parsed || dueDate === "9999-12-31") return false;
  const dueUtc = Date.UTC(parsed.year, parsed.month - 1, parsed.day);
  const todayUtc = Date.UTC(today.getFullYear(), today.getMonth(), today.getDate());
  return dueUtc < todayUtc;
}
