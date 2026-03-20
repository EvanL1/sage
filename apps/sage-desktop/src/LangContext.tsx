import { createContext, useContext, useState, useEffect, ReactNode } from "react";
import { invoke } from "@tauri-apps/api/core";
import { createT, detectLang, Lang } from "./i18n";

interface LangContextValue {
  lang: Lang;
  t: ReturnType<typeof createT>;
  setLang: (lang: Lang) => void;
}

const LangContext = createContext<LangContextValue>({
  lang: "en",
  t: createT("en"),
  setLang: () => {},
});

export function LangProvider({ children }: { children: ReactNode }) {
  const [lang, setLang] = useState<Lang>(detectLang);

  useEffect(() => {
    invoke<{ identity: { prompt_language?: string } } | null>("get_profile")
      .then(profile => {
        if (profile?.identity?.prompt_language) {
          setLang(profile.identity.prompt_language === "en" ? "en" : "zh");
        }
      })
      .catch(() => {});
  }, []);

  const t = createT(lang);

  return (
    <LangContext.Provider value={{ lang, t, setLang }}>
      {children}
    </LangContext.Provider>
  );
}

export function useLang() {
  return useContext(LangContext);
}
