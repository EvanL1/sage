import { createContext, useContext, useEffect, useState, type ReactNode } from "react";

type Theme = "light" | "dark";
type ColorScheme = "default" | "longbridge" | "bloomberg" | "military" | "arctic" | "mono";

interface ThemeContextType {
  theme: Theme;
  colorScheme: ColorScheme;
  toggle: () => void;
  setColorScheme: (cs: ColorScheme) => void;
}

const ThemeContext = createContext<ThemeContextType>({
  theme: "light",
  colorScheme: "default",
  toggle: () => {},
  setColorScheme: () => {},
});

const COLOR_SCHEMES: ColorScheme[] = ["default", "longbridge", "bloomberg", "military", "arctic", "mono"];

export const COLOR_SCHEME_META: Record<ColorScheme, { name: string; accent: string }> = {
  default:    { name: "Default",    accent: "#6366f1" },
  longbridge: { name: "Longbridge", accent: "#00b07c" },
  bloomberg:  { name: "Bloomberg",  accent: "#ff9500" },
  military:   { name: "Military",   accent: "#39ff14" },
  arctic:     { name: "Arctic",     accent: "#5bc8f5" },
  mono:       { name: "Mono",       accent: "#ffffff" },
};

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>(() => {
    const saved = localStorage.getItem("sage-theme");
    if (saved === "dark" || saved === "light") return saved;
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  });

  const [colorScheme, setColorSchemeState] = useState<ColorScheme>(() => {
    const saved = localStorage.getItem("sage-color-scheme") as ColorScheme;
    return COLOR_SCHEMES.includes(saved) ? saved : "default";
  });

  useEffect(() => {
    const el = document.documentElement;
    // Non-default color schemes force dark mode
    const effectiveTheme = colorScheme !== "default" ? "dark" : theme;
    el.setAttribute("data-theme", effectiveTheme);
    el.setAttribute("data-color-scheme", colorScheme);
    localStorage.setItem("sage-theme", theme);
    localStorage.setItem("sage-color-scheme", colorScheme);
  }, [theme, colorScheme]);

  const toggle = () => setTheme((t) => (t === "light" ? "dark" : "light"));

  const setColorScheme = (cs: ColorScheme) => {
    setColorSchemeState(cs);
    // Also sync to ControlLayout's legacy key so it stays consistent
    localStorage.setItem("control_theme", cs);
  };

  return (
    <ThemeContext.Provider value={{ theme, colorScheme, toggle, setColorScheme }}>
      {children}
    </ThemeContext.Provider>
  );
}

export const useTheme = () => useContext(ThemeContext);
