/** Turn 3DMigoto binding tokens into compact labels for keyboard badges. */

const NAMED: Record<string, string> = {
  CTRL: "Ctrl",
  CONTROL: "Ctrl",
  VK_CONTROL: "Ctrl",
  ALT: "Alt",
  MENU: "Alt",
  VK_MENU: "Alt",
  SHIFT: "Shift",
  VK_SHIFT: "Shift",
  NO_MODIFIERS: "No modifiers",
  NO_MODIFIER: "No modifiers",
  NO_CTRL: "No Ctrl",
  NO_CONTROL: "No Ctrl",
  NO_ALT: "No Alt",
  NO_SHIFT: "No Shift",
  VK_ESCAPE: "Esc",
  ESC: "Esc",
  VK_RETURN: "Enter",
  RETURN: "Enter",
  ENTER: "Enter",
  VK_SPACE: "Space",
  SPACE: "Space",
  VK_TAB: "Tab",
  TAB: "Tab",
  VK_BACK: "Backspace",
  BACKSPACE: "Backspace",
  VK_DELETE: "Del",
  DELETE: "Del",
  VK_INSERT: "Ins",
  INSERT: "Ins",
  VK_PRIOR: "PgUp",
  PAGEUP: "PgUp",
  VK_NEXT: "PgDn",
  PAGEDOWN: "PgDn",
  VK_HOME: "Home",
  VK_END: "End",
  VK_CAPITAL: "Caps",
  CAPSLOCK: "Caps",
  VK_SNAPSHOT: "PrtSc",
  VK_SCROLL: "Scroll",
  VK_PAUSE: "Pause",
  VK_NUMLOCK: "Num Lock",
  VK_CLEAR: "Clear",
  VK_HELP: "Help",
  VK_APPS: "Menu",
  VK_LWIN: "Win",
  VK_RWIN: "Win",
  VK_UP: "↑",
  UP: "↑",
  VK_DOWN: "↓",
  DOWN: "↓",
  VK_LEFT: "←",
  LEFT: "←",
  VK_RIGHT: "→",
  RIGHT: "→",
  VK_OEM_1: ";",
  VK_OEM_2: "/",
  VK_OEM_3: "`",
  VK_OEM_4: "[",
  VK_OEM_5: "\\",
  VK_OEM_6: "]",
  VK_OEM_7: "'",
  VK_OEM_COMMA: ",",
  VK_OEM_PERIOD: ".",
  VK_OEM_MINUS: "-",
  VK_OEM_PLUS: "+",
  VK_MULTIPLY: "Num *",
  VK_ADD: "Num +",
  VK_SUBTRACT: "Num -",
  VK_DIVIDE: "Num /",
  VK_DECIMAL: "Num .",
};

export function formatHotkeyToken(token: string): string {
  const upper = token.trim().toUpperCase();
  if (!upper) return "";

  const named = NAMED[upper];
  if (named) return named;

  if (/^VK_F([1-9]|1\d|2[0-4])$/.test(upper)) {
    return upper.slice(3);
  }

  const numpad = upper.match(/^VK_NUMPAD(\d)$/);
  if (numpad) return `Num ${numpad[1]}`;

  const letterOrDigit = upper.match(/^VK_([A-Z0-9])$/);
  if (letterOrDigit) return letterOrDigit[1];

  return upper.startsWith("VK_") ? upper.slice(3) : token;
}

/** `KeyToggleHair` → `Toggle Hair`; a written-out description wins. */
export function describeHotkey(
  section: string,
  description: string | null,
): string {
  const explicit = description?.trim();
  if (explicit) return explicit;

  const withoutPrefix = section.replace(/^Key/i, "").trim();
  if (!withoutPrefix) return section;

  return withoutPrefix
    .replace(/([a-z0-9])([A-Z])/g, "$1 $2")
    .replace(/[_-]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}
