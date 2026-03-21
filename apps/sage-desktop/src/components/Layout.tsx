import { useState, useEffect } from "react";
import { NavLink, Outlet, useLocation } from "react-router-dom";
import { invoke } from "@tauri-apps/api/core";
// removed: getCurrentWindow — using only data-tauri-drag-region for window drag
import { useTheme, COLOR_SCHEME_META } from "../contexts/ThemeContext";
import ModelSwitcher from "./ModelSwitcher";

const pageTitles: Record<string, string> = {
  "/": "Dashboard",
  "/chat": "Chat",
  "/tasks": "Tasks",
  "/about": "About You",
  "/graph": "Knowledge",
  "/feed": "Feed Intelligence",
  "/pages": "Pages",
  "/history": "History",
  "/settings": "Settings",
};

const COLOR_SCHEMES = Object.entries(COLOR_SCHEME_META) as [keyof typeof COLOR_SCHEME_META, typeof COLOR_SCHEME_META[keyof typeof COLOR_SCHEME_META]][];

function Layout() {
  const { theme, colorScheme, toggle, setColorScheme } = useTheme();
  const location = useLocation();
  const title = location.pathname.startsWith("/pages")
    ? (pageTitles["/pages"] ?? "Pages")
    : (pageTitles[location.pathname] ?? "Sage");
  const [daemonOnline, setDaemonOnline] = useState(false);


  useEffect(() => {
    const check = () => {
      invoke<{ status: string }>("get_system_status")
        .then(() => setDaemonOnline(true))
        .catch(() => setDaemonOnline(false));
    };
    check();
    const interval = setInterval(check, 15_000);
    return () => clearInterval(interval);
  }, []);

  return (
    <div className="app-layout">
      <aside className="sidebar">
        <div className="sidebar-drag" data-tauri-drag-region />

        <div className="sidebar-top">
          <div className="sidebar-logo" title="Sage">
            <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
              <path d="M12 2L2 7l10 5 10-5-10-5z" />
              <path d="M2 17l10 5 10-5" />
              <path d="M2 12l10 5 10-5" />
            </svg>
            <div className={`daemon-pulse ${daemonOnline ? "online" : "offline"}`} />
          </div>

          <nav className="sidebar-nav">
            <NavLink to="/" end className={({ isActive }) => `sidebar-link${isActive ? " active" : ""}`} title="Dashboard">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <rect x="3" y="3" width="7" height="7" rx="1" />
                <rect x="14" y="3" width="7" height="7" rx="1" />
                <rect x="3" y="14" width="7" height="7" rx="1" />
                <rect x="14" y="14" width="7" height="7" rx="1" />
              </svg>
            </NavLink>

            <NavLink to="/chat" className={({ isActive }) => `sidebar-link${isActive ? " active" : ""}`} title="Chat">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M21 15a2 2 0 01-2 2H7l-4 4V5a2 2 0 012-2h14a2 2 0 012 2z" />
              </svg>
            </NavLink>

            <NavLink to="/tasks" className={({ isActive }) => `sidebar-link${isActive ? " active" : ""}`} title="Tasks">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M9 11l3 3L22 4" />
                <path d="M21 12v7a2 2 0 01-2 2H5a2 2 0 01-2-2V5a2 2 0 012-2h11" />
              </svg>
            </NavLink>

            <NavLink to="/about" className={({ isActive }) => `sidebar-link${isActive ? " active" : ""}`} title="About You">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M20 21v-2a4 4 0 00-4-4H8a4 4 0 00-4 4v2" />
                <circle cx="12" cy="7" r="4" />
              </svg>
            </NavLink>

            <NavLink to="/graph" className={({ isActive }) => `sidebar-link${isActive ? " active" : ""}`} title="Memory Graph">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="6" cy="6" r="3" />
                <circle cx="18" cy="6" r="3" />
                <circle cx="6" cy="18" r="3" />
                <circle cx="18" cy="18" r="3" />
                <line x1="8.5" y1="7.5" x2="15.5" y2="16.5" />
                <line x1="15.5" y1="7.5" x2="8.5" y2="16.5" />
              </svg>
            </NavLink>

            <NavLink to="/feed" className={({ isActive }) => `sidebar-link${isActive ? " active" : ""}`} title="Feed Intelligence">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M4 11a9 9 0 019 9" />
                <path d="M4 4a16 16 0 0116 16" />
                <circle cx="5" cy="19" r="1" />
              </svg>
            </NavLink>

            <NavLink to="/pages" className={({ isActive }) => `sidebar-link${isActive ? " active" : ""}`} title="Pages">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M14 2H6a2 2 0 00-2 2v16a2 2 0 002 2h12a2 2 0 002-2V8z" />
                <polyline points="14 2 14 8 20 8" />
                <line x1="16" y1="13" x2="8" y2="13" />
                <line x1="16" y1="17" x2="8" y2="17" />
                <polyline points="10 9 9 9 8 9" />
              </svg>
            </NavLink>

            <NavLink to="/history" className={({ isActive }) => `sidebar-link${isActive ? " active" : ""}`} title="History">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="10" />
                <polyline points="12 6 12 12 16 14" />
              </svg>
            </NavLink>

            <NavLink to="/settings" className={({ isActive }) => `sidebar-link${isActive ? " active" : ""}`} title="Settings">
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="3" />
                <path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 01-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06A1.65 1.65 0 004.68 15a1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06A1.65 1.65 0 009 4.68a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06A1.65 1.65 0 0019.4 9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z" />
              </svg>
            </NavLink>
          </nav>
        </div>

        <div className="sidebar-bottom">
          <div className="sidebar-schemes">
            {COLOR_SCHEMES.map(([key, meta]) => (
              <button key={key}
                className={`sidebar-scheme-dot${colorScheme === key ? " active" : ""}`}
                style={{ "--dot-color": meta.accent } as React.CSSProperties}
                onClick={() => setColorScheme(key)}
                title={meta.name} />
            ))}
          </div>
          <button className="sidebar-link theme-toggle" onClick={toggle} title={theme === "light" ? "Dark" : "Light"}>
            {theme === "light" ? (
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <path d="M21 12.79A9 9 0 1111.21 3 7 7 0 0021 12.79z" />
              </svg>
            ) : (
              <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round">
                <circle cx="12" cy="12" r="5" />
                <line x1="12" y1="1" x2="12" y2="3" />
                <line x1="12" y1="21" x2="12" y2="23" />
                <line x1="4.22" y1="4.22" x2="5.64" y2="5.64" />
                <line x1="18.36" y1="18.36" x2="19.78" y2="19.78" />
                <line x1="1" y1="12" x2="3" y2="12" />
                <line x1="21" y1="12" x2="23" y2="12" />
                <line x1="4.22" y1="19.78" x2="5.64" y2="18.36" />
                <line x1="18.36" y1="5.64" x2="19.78" y2="4.22" />
              </svg>
            )}
          </button>
        </div>
      </aside>

      <main className="main-content">
        <div className="titlebar" data-tauri-drag-region>
          <span className="titlebar-text">{title} <span style={{fontSize:9,opacity:0.4,marginLeft:8}}>v0.1.9</span></span>
          <ModelSwitcher />
        </div>
        <div className="main-scroll">
          <Outlet />
        </div>
      </main>
    </div>
  );
}

export default Layout;
