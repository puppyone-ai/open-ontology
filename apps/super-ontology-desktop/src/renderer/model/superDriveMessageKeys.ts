import english from "../../locales/en/super-drive.json";

/** Message ids (with the `super-drive.` prefix) that exist, for error fallbacks. All locales carry the same keys. */
const ids = new Set(Object.keys(english).map((key) => `super-drive.${key}`));

export const hasSuperDriveMessage = (messageId: string): boolean => ids.has(messageId);
