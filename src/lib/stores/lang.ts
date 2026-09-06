import { getLocale, setLocale } from "$lib/i18n";

/**
 * Supported UI locales, keyed by BCP-47 language tag. The native name is shown
 * in the language selector. Keep this in sync with `languageTags` in
 * `src/i18n/project.inlang/settings.json` — paraglide generates message modules
 * only for the tags listed there.
 */
export const SUPPORTED_LOCALES = {
  en: "English",
  es: "Español",
  pt: "Português",
  fr: "Français",
  it: "Italiano",
  de: "Deutsch",
  ru: "Русский",
  uk: "Українська",
  be: "Беларуская",
  pl: "Polski",
  cs: "Čeština",
} as const;

export type Locale = keyof typeof SUPPORTED_LOCALES;
export type LanguageSetting = "system" | Locale;

const STORAGE_KEY = "aiassistant.lang";
// Marks that a locale switch has already been attempted in this tab session.
// sessionStorage survives a location.reload() within the same session, so it
// lets us detect when paraglide's persistence failed to take effect and break
// what would otherwise be an infinite reload loop.
const SWITCH_ATTEMPT_KEY = "aiassistant.lang.switchAttempt";


function isLocale(v: string): v is Locale {
  return v in SUPPORTED_LOCALES;
}

export function loadLanguageSetting(): LanguageSetting {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    return v && isLocale(v) ? v : "system";
  } catch {
    return "system";
  }
}

export function saveLanguageSetting(setting: LanguageSetting) {
  try {
    localStorage.setItem(STORAGE_KEY, setting);
  } catch {
    // localStorage may be unavailable; the choice then applies for this session only
  }
}

/**
 * Resolves a language setting to a concrete locale. When set to "system", the
 * browser/OS language (navigator.language) is matched against the supported
 * locales by primary subtag; unmatched inputs fall back to English.
 */
export function resolveLanguage(setting: LanguageSetting): Locale {
  if (setting === "system") {
    try {
      const primary = navigator.language?.toLowerCase().split("-")[0];
      if (primary && isLocale(primary)) return primary;
    } catch {
      // navigator.language unavailable — fall back to base locale
    }
    return "en";
  }
  return setting;
}

/**
 * Applies the language setting via paraglide. `setLocale` reloads the document
 * when the locale actually changes (paraglide client runtime default), so the
 * whole UI re-renders in the new language.
 * Returns true when a reload has been triggered.
 *
 * A reload-loop guard prevents the UI from bricking if paraglide's configured
 * strategy fails to persist the locale across reloads: sessionStorage survives
 * `location.reload()` within a session, so a repeated attempt to switch to the
 * same target in the same session means persistence is broken and we abort
 * instead of reloading forever.
 */
export async function applyLanguageSetting(setting: LanguageSetting): Promise<boolean> {
  const target = resolveLanguage(setting);
  if (getLocale() === target) {
    try {
      sessionStorage.removeItem(SWITCH_ATTEMPT_KEY);
    } catch {
      // sessionStorage unavailable — nothing to clear
    }
    return false;
  }
  try {
    if (sessionStorage.getItem(SWITCH_ATTEMPT_KEY) === target) {
      console.error(
        `Locale switch to "${target}" did not persist after reload; aborting to avoid a reload loop. ` +
          "Check the Paraglide strategy in vite.config.ts (a persistent browser strategy such as \"localStorage\" is required).",
      );
      return false;
    }
    sessionStorage.setItem(SWITCH_ATTEMPT_KEY, target);
  } catch {
    // sessionStorage unavailable — proceed without the guard
  }
  try {
    await setLocale(target);
  } catch (e) {
    console.error("setLocale failed", e);
    return false;
  }
  return true;
}