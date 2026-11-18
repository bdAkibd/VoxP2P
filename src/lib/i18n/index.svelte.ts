import type { SupportedLocale, LocaleInfo, TranslationSchema } from "./types";
import { en } from "./locales/en";
import { uk } from "./locales/uk";
import { ru } from "./locales/ru";
import { de } from "./locales/de";
import { fr } from "./locales/fr";
import { es } from "./locales/es";
import { ja } from "./locales/ja";
import { api } from "../api";

export const SUPPORTED_LOCALES: LocaleInfo[] = [
  { code: "en", name: "English", englishName: "English", flag: "🇬🇧" },
  { code: "uk", name: "Українська", englishName: "Ukrainian", flag: "🇺🇦" },
  { code: "ru", name: "Русский", englishName: "Russian", flag: "🇷🇺" },
  { code: "de", name: "Deutsch", englishName: "German", flag: "🇩🇪" },
  { code: "fr", name: "Français", englishName: "French", flag: "🇫🇷" },
  { code: "es", name: "Español", englishName: "Spanish", flag: "🇪🇸" },
  { code: "ja", name: "日本語", englishName: "Japanese", flag: "🇯🇵" },
];

export const DICTIONARIES: Record<SupportedLocale, TranslationSchema> = {
  en,
  uk,
  ru,
  de,
  fr,
  es,
  ja,
};

class I18nManager {
  currentLocale = $state<SupportedLocale>("en");
  isInitialized = $state<boolean>(false);

  constructor() {
    if (typeof window !== "undefined") {
      const stored = localStorage.getItem("voxp2p_lang");
      if (stored && this.isValidLocale(stored)) {
        this.currentLocale = stored as SupportedLocale;
      }
    }
  }

  isValidLocale(code: string): code is SupportedLocale {
    return ["en", "uk", "ru", "de", "fr", "es", "ja"].includes(code);
  }

  translate(locale: SupportedLocale, key: string, params?: Record<string, string | number>): string {
    const dict = DICTIONARIES[locale] || DICTIONARIES.en;
    const parts = key.split(".");
    let curr: any = dict;

    for (const part of parts) {
      if (curr && typeof curr === "object" && part in curr) {
        curr = curr[part];
      } else {
        curr = undefined;
        break;
      }
    }

    if (curr === undefined) {
      let fallbackCurr: any = DICTIONARIES.en;
      for (const part of parts) {
        if (fallbackCurr && typeof fallbackCurr === "object" && part in fallbackCurr) {
          fallbackCurr = fallbackCurr[part];
        } else {
          fallbackCurr = undefined;
          break;
        }
      }
      curr = fallbackCurr !== undefined ? fallbackCurr : key;
    }

    if (typeof curr !== "string") {
      return key;
    }

    if (params) {
      return curr.replace(/\{(\w+)\}/g, (_, k) => {
        return params[k] !== undefined ? String(params[k]) : `{${k}}`;
      });
    }

    return curr;
  }

  get dictionary(): TranslationSchema {
    return DICTIONARIES[this.currentLocale] || DICTIONARIES.en;
  }

  async initLocale(): Promise<SupportedLocale> {
    if (this.isInitialized) return this.currentLocale;

    let targetLocale: SupportedLocale = "en";

    // 1. Saved in SQLite via api.getAppSettings()
    try {
      const settings = await api.getAppSettings();
      const dbLang = settings["app_language"];
      if (dbLang && this.isValidLocale(dbLang)) {
        targetLocale = dbLang;
        this.applyLocale(targetLocale);
        this.isInitialized = true;
        return targetLocale;
      }
    } catch (e) {
      console.warn("i18n: Failed to read app_language from SQLite settings:", e);
    }

    // 2. localStorage
    if (typeof window !== "undefined") {
      const localLang = localStorage.getItem("voxp2p_lang");
      if (localLang && this.isValidLocale(localLang)) {
        targetLocale = localLang;
        this.applyLocale(targetLocale);
        this.isInitialized = true;
        return targetLocale;
      }

      // 3. System locale (navigator.language)
      const navLang = navigator.language.slice(0, 2).toLowerCase();
      if (this.isValidLocale(navLang)) {
        targetLocale = navLang;
        this.applyLocale(targetLocale);
        this.isInitialized = true;
        return targetLocale;
      }
    }

    // 4. Fallback: en
    this.applyLocale("en");
    this.isInitialized = true;
    return "en";
  }

  setLocale(lang: SupportedLocale) {
    if (!this.isValidLocale(lang)) return;
    this.applyLocale(lang);

    // Asynchronously save to SQLite
    api.setAppSetting("app_language", lang).catch((err) => {
      console.warn("i18n: Failed to persist app_language to SQLite:", err);
    });
  }

  private applyLocale(lang: SupportedLocale) {
    this.currentLocale = lang;
    if (typeof document !== "undefined") {
      document.documentElement.lang = lang;
      if (lang === "ja") {
        document.documentElement.classList.add("locale-cjk");
      } else {
        document.documentElement.classList.remove("locale-cjk");
      }
    }
    if (typeof localStorage !== "undefined") {
      try {
        localStorage.setItem("voxp2p_lang", lang);
      } catch {}
    }
  }
}

export const i18n = new I18nManager();

/**
 * Global reactive translation helper.
 * When called inside a Svelte 5 component template, Svelte automatically tracks
 * i18n.currentLocale and re-renders when the language changes.
 */
export function t(key: string, params?: Record<string, string | number>): string {
  const current = i18n.currentLocale;
  return i18n.translate(current, key, params);
}

export const setLocale = (lang: SupportedLocale) => i18n.setLocale(lang);
export const initLocale = () => i18n.initLocale();
export function getLocale(): SupportedLocale {
  return i18n.currentLocale;
}
