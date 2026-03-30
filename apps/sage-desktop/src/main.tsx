import React from "react";
import ReactDOM from "react-dom/client";
import { open } from "@tauri-apps/plugin-shell";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/jetbrains-mono/500.css";
import "@fontsource/jetbrains-mono/700.css";
import App from "./App";
import "./App.css";
import "./layouts/layouts.css";

// 禁用 WebView 默认右键菜单（Back / Reload）
document.addEventListener("contextmenu", (e) => e.preventDefault());

// 全局拦截外部链接，用系统浏览器打开
document.addEventListener("click", (e) => {
  const anchor = (e.target as HTMLElement).closest("a");
  if (!anchor) return;
  const href = anchor.getAttribute("href");
  if (!href) return;
  // 内部路由（hash 或相对路径）不拦截
  if (href.startsWith("#") || href.startsWith("/")) return;
  // 外部 URL：阻止 WebView 导航，用系统浏览器打开
  if (href.startsWith("http://") || href.startsWith("https://") || href.startsWith("mailto:")) {
    e.preventDefault();
    e.stopPropagation();
    open(href).catch(console.error);
  }
});

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
