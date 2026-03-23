import { ThemeProvider } from "./contexts/ThemeContext";
import { LangProvider } from "./LangContext";
import { HashRouter, Routes, Route, Navigate } from "react-router-dom";
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
import MessageFlow from "./pages/MessageFlow";
import Mail from "./pages/Mail";
import PagesList from "./pages/PagesList";
import DynamicPage from "./pages/DynamicPage";

function App() {
  return (
    <ThemeProvider>
      <LangProvider>
      <HashRouter>
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
            <Route path="/mail" element={<Mail />} />
            <Route path="/messages" element={<MessageFlow />} />
            <Route path="/pages" element={<PagesList />} />
            <Route path="/pages/:id" element={<DynamicPage />} />
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
