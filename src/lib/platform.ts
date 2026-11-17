// The one place the UI asks which system it runs on. Screens use these helpers instead of
// checking the platform themselves.

const ua = typeof navigator === "undefined" ? "" : `${navigator.platform} ${navigator.userAgent}`.toLowerCase();

export const isMac = ua.includes("mac");
export const isWindows = ua.includes("win");

/** The modifier used for app shortcuts: Command on macOS, Control elsewhere. */
export const modPressed = (e: { metaKey: boolean; ctrlKey: boolean }) => (isMac ? e.metaKey : e.ctrlKey);

/** How a shortcut is written for people on this system: "⌘K" or "Ctrl+K". */
export const shortcut = (key: string) => (isMac ? `⌘${key}` : `Ctrl+${key}`);

/** A file or shell name from a path written with either kind of slash. */
export const baseName = (path: string) => path.split(/[\\/]/).pop() ?? path;

/** True when a key event started inside a terminal, where Control combinations belong to the shell. */
export const fromTerminal = (e: Event) => e.target instanceof Element && e.target.closest(".xterm") !== null;
