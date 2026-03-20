import { useEffect } from "react";
import { ThemeProvider } from "./contexts/ThemeContext";
import { LangProvider } from "./LangContext";
import { HashRouter, Routes, Route, Navigate, useNavigate } from "react-router-dom";
import { listen } from "@tauri-apps/api/event";
import Layout from "./components/Layout";
import Dashboard from "./pages/Dashboard";
import Onboarding from "./pages/Onboarding";
import Welcome from "./pages/Welcome";
import History from "./pages/History";
import Settings from "./pages/Settings";
import Chat from "./pages/Chat";
import AboutYou from "./pages/AboutYou";
import MemoryGraph from "./pages/MemoryGraph";
import Tasks from "./pages/Tasks";
import FeedIntelligence from "./pages/FeedIntelligence";

/** 监听 Rust 端 navigate-to 事件，自动跳转到对应页面 */
function NavigationListener() {
  const navigate = useNavigate();

  useEffect(() => {
    const unlisten = listen<string>("navigate-to", (event) => {
      const route = event.payload;
      if (route && typeof route === "string") {
        navigate(route);
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [navigate]);

  return null;
}

function App() {
  return (
    <ThemeProvider>
      <LangProvider>
      <HashRouter>
        <NavigationListener />
        <Routes>
          <Route path="/onboarding" element={<Onboarding />} />
          <Route path="/welcome" element={<Welcome />} />
          <Route element={<Layout />}>
            <Route path="/" element={<Dashboard />} />
            <Route path="/chat" element={<Chat />} />
            <Route path="/about" element={<AboutYou />} />
            <Route path="/graph" element={<MemoryGraph />} />
            <Route path="/tasks" element={<Tasks />} />
            <Route path="/feed" element={<FeedIntelligence />} />
            <Route path="/history" element={<History />} />
            <Route path="/settings" element={<Settings />} />
          </Route>
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </HashRouter>
      </LangProvider>
    </ThemeProvider>
  );
}

export default App;
